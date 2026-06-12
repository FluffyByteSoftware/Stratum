/*
 * (TcpHost.cs)
 *------------------------------------------------------------
 * Created - 5/30/2026 12:34:43 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;
using Stratum.Networking.Dispatch;
using Shared.Networking;
using System.Buffers;
using System.Collections.Concurrent;
using System.Net;
using System.Net.Security;
using System.Net.Sockets;
using System.Security.Cryptography.X509Certificates;
using SystemTools.Logger;
using Shared.Networking.Packets.LifeCycle;
using Stratum.Shared.Networking;

namespace Networking.Tcp;

/// <summary>
/// Represents a TCP server host that listens for and accepts incoming connections, 
/// handles packet-based communication with optional TLS encryption, and dispatches 
/// packets to registered handlers.
/// </summary>
/// <remarks>Manages multiple concurrent client connections in a thread-safe manner.
/// Use <see cref="StopAsync"/> for graceful shutdown, which waits for all in-flight
/// operations to complete.</remarks>
public sealed class TcpHost
{
    private enum ReadAction { Proceed, ReturnNow, BreakToTeardown }

    private static readonly TimeSpan HandshakeTimeout = TimeSpan.FromSeconds(10);
    private static readonly TimeSpan ReadTimeout = TimeSpan.FromSeconds(30);
    private static readonly TimeSpan SendTimeout = TimeSpan.FromSeconds(5);

    private readonly int _port;
    private readonly IPAddress _bindAddress;
    private readonly PacketDispatcher<TcpConnection> _dispatcher;
    private readonly X509Certificate2? _certificate;
    private readonly ConcurrentDictionary<long, Task> _handlers = new();
    private readonly CancellationTokenSource _shutdownCts = new();
    private readonly CancellationToken _shutdownToken;

    private TcpListener? _listener;
    private Task? _acceptLoop;
    private long _nextId;
    private int _stopped;

    /// <summary>
    /// Initializes a new instance of the TcpHost class with the specified port, packet 
    /// dispatcher, and optional SSL certificate.
    /// </summary>
    /// <param name="port">The port number to listen on (1-65535).</param>
    /// <param name="dispatcher">The packet dispatcher to handle incoming connections. 
    /// Must be frozen before use.</param>
    /// <param name="certificate">The SSL/TLS certificate for secure connections, or 
    /// <see langword="null"/> for unencrypted connections.</param>
    /// <exception cref="ArgumentOutOfRangeException"><paramref name="port"/> is less than 1 or 
    /// greater than 65535.</exception>
    /// <exception cref="InvalidOperationException"><paramref name="dispatcher"/> is not 
    /// frozen.</exception>
    public TcpHost(
        IPAddress bindAddress,
        int port,
        PacketDispatcher<TcpConnection> dispatcher,
        X509Certificate2? certificate = null)
    {
        if (port is < 1 or > 65535)
            throw new ArgumentOutOfRangeException(
                nameof(port), $"Port must be in the range 1-65535.");

        ArgumentNullException.ThrowIfNull(dispatcher);

        if (!dispatcher.IsFrozen)
            throw new InvalidOperationException(
                "Dispatcher must be frozen before constructing the host.");

        _bindAddress = bindAddress;
        _port = port;
        _dispatcher = dispatcher;
        _certificate = certificate;
        _shutdownToken = _shutdownCts.Token;
    }

    /// <summary>
    /// Starts the TCP host and begins listening for incoming connections.
    /// </summary>
    /// <exception cref="InvalidOperationException">Thrown when the host is
    /// already started.</exception>
    public void Start()
    {
        if (_acceptLoop is not null)
            throw new InvalidOperationException("Host is already started.");

        _listener = new TcpListener(_bindAddress, _port);
        _listener.Start();
        _acceptLoop = AcceptLoopAsync();

        string tls = _certificate is not null ? " (TLS)" : "";
        
        Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
            $"Tcp Host listening on {_bindAddress}, port: {_port}{tls}."));
    }

    /// <summary>
    /// Stops the listener asynchronously and waits for all in-flight
    /// operations to complete.
    /// </summary>
    /// <remarks>This method is safe to call multiple times. Subsequent calls 
    /// return immediately without performing any additional work.</remarks>
    /// <returns>A task that represents the asynchronous stop operation.</returns>
    public async Task StopAsync()
    {
        if(Interlocked.Exchange(ref _stopped, 1) != 0)
            return;

        _shutdownCts.Cancel();

        if(_acceptLoop is not null)
        {
            try
            {
                await _acceptLoop.ConfigureAwait(false);
            }
            catch
            {

            }
        }

        try
        {
            _listener?.Stop();
        }
        catch
        {

        }

        Task[] inflight = [.. _handlers.Values];

        if(inflight.Length > 0)
        {
            try
            {
                await Task.WhenAll(inflight).ConfigureAwait(false);
            }
            catch { }
        }

        _shutdownCts.Dispose();
    }

    /// <summary>
    /// Asynchronously sends a serialized packet over the specified TCP connection.
    /// </summary>
    /// <typeparam name="T">The packet type that implements IPacketWritable.</typeparam>
    /// <param name="connection">The TCP connection to send the packet over.</param>
    /// <param name="packet">The packet to serialize and send.</param>
    /// <returns>true if the packet was sent successfully; otherwise, false.</returns>
#pragma warning disable CA1822 // Mark members as static
    public async ValueTask<bool> SendAsync<T>(
#pragma warning restore CA1822 // Mark members as static
        TcpConnection connection, T packet)
        where T: struct, IPacketWritable
    {
        var writer = new NetDataWriter();
        packet.Serialize(writer);

        int payloadLength = writer.Length;
        int frameSize = PacketFramer.FrameSize(payloadLength);
        byte[] frame = ArrayPool<byte>.Shared.Rent(frameSize);
        bool locked = false;

        using var sendCts = new CancellationTokenSource(SendTimeout);

        try
        {
            PacketFramer.WriteFrame(
                packet.TypeId, writer.Data.AsSpan(0, payloadLength), frame);

            await connection.SendLock.WaitAsync(sendCts.Token).ConfigureAwait(false);

            locked = true;

            await connection.Stream
                .WriteAsync(frame.AsMemory(0, frameSize), sendCts.Token)
                .ConfigureAwait(false);

            await connection.Stream.FlushAsync(sendCts.Token).ConfigureAwait(false);

            return true;
        }
        catch(Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Send to {connection.RemoteEndPoint} failed: {ex.Message}", ex));
            return false;
        }
        finally
        {
            if (locked)
                connection.SendLock.Release();

            ArrayPool<byte>.Shared.Return(frame);
        }
    }

    private long NextId() => Interlocked.Increment(ref _nextId);

    private async Task AcceptLoopAsync()
    {
        while (!_shutdownToken.IsCancellationRequested)
        {
            TcpClient client;
            try
            {
                client = await _listener!.AcceptTcpClientAsync(_shutdownToken)
                    .ConfigureAwait(false);
            }
            catch (OperationCanceledException) { break; }
            catch (ObjectDisposedException) { break; }
            catch(SocketException ex)
            {
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn, 
                    $"Accept failed: {ex.Message}", ex));
                continue;
            }
            catch(Exception ex)
            {
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Error, 
                    $"Accept loop error: {ex.Message}", ex));
                continue;
            }

            long id = NextId();
            _handlers[id] = TrackedHandleAsync(client, id);
        }
    }
    
    private async Task TrackedHandleAsync(TcpClient client, long id)
    {
        // Yield first so the task is registered in _handlers first.
        await Task.Yield();

        try
        {
            await HandleConnectionAsync(client, id).ConfigureAwait(false);
        }
        finally
        {
            _handlers.TryRemove(id, out _);
        }
    }

    private async Task HandleConnectionAsync(TcpClient client, long id)
    {
        string remote = FormatEndpoint(client);
        NetworkStream? networkStream = null;
        SslStream? ssl = null;
        TcpConnection? conn = null;

        try
        {
            networkStream = client.GetStream();
            Stream activeStream = networkStream;

            if(_certificate is not null)
            {
                ssl = new SslStream(networkStream, leaveInnerStreamOpen: false);

                using var hsCts =
                    CancellationTokenSource.CreateLinkedTokenSource(_shutdownToken);
                hsCts.CancelAfter(HandshakeTimeout);

                var options = new SslServerAuthenticationOptions
                {
                    ServerCertificate = _certificate,
                    ClientCertificateRequired = false,
                    EnabledSslProtocols = 
                    System.Security.Authentication.SslProtocols.Tls12 |
                    System.Security.Authentication.SslProtocols.Tls13,
                };

                await ssl.AuthenticateAsServerAsync(options, hsCts.Token)
                    .ConfigureAwait(false);
                activeStream = ssl;
            }

            conn = new TcpConnection(id, client, activeStream, remote);
            await ReadLoopAsync(conn).ConfigureAwait(false);
        }
        catch(OperationCanceledException) when 
            (_shutdownToken.IsCancellationRequested)
        {             
            // Host is shutting down, ignore.
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Connection {remote} error: {ex.Message}", ex));
        }
        finally
        {
            if (conn is not null)
                conn.Close();
            else
            {
                try
                {
                    ssl?.Dispose();
                }
                catch { }
                try { networkStream?.Dispose(); } catch { }
                try { client.Dispose(); } catch { }
            }
        }
    }
    private async Task ReadLoopAsync(TcpConnection conn)
    {
        byte[] header = new byte[PacketFramer.HeaderSize];
        Stream stream = conn.Stream;

        while (true)
        {
            if (conn.DisconnectRequested)
            {
                break;
            }

            ReadAction action =
                await ReadFullyAsync(stream, header, conn).ConfigureAwait(false);
            if (action == ReadAction.ReturnNow) return;
            if (action == ReadAction.BreakToTeardown) break;

            if (!PacketFramer.TryReadHeader(
                    header, out uint typeId, out int payloadLength))
            {
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                    $"Malformed frame header from {conn.RemoteEndPoint}."));

                conn.RequestDisconnect(SecureDisconnectReason.MalformedPacket);
                break;
            }

            byte[] payload = ArrayPool<byte>.Shared.Rent(payloadLength);
            try
            {
                action = await ReadFullyAsync(
                        stream, payload.AsMemory(0, payloadLength), conn)
                    .ConfigureAwait(false);
                if (action == ReadAction.ReturnNow) return;
                if (action == ReadAction.BreakToTeardown) break;

                var reader = new NetDataReader();
                reader.SetSource(payload, 0, payloadLength);

                DispatchResult result = await _dispatcher
                    .DispatchAsync(conn, typeId, reader)
                    .ConfigureAwait(false);

                HandleDispatchResult(conn, result);
                if (conn.DisconnectRequested)
                {
                    break;
                }
            }
            finally
            {
                ArrayPool<byte>.Shared.Return(payload);
            }
        }

        await SendDisconnectIfNeededAsync(conn).ConfigureAwait(false);
    }

    private async ValueTask<ReadAction> ReadFullyAsync(
        Stream stream, Memory<byte> buffer, TcpConnection conn)
    {
        try
        {
            using var readCts =
                CancellationTokenSource.CreateLinkedTokenSource(_shutdownToken);
            
            readCts.CancelAfter(ReadTimeout);

            await stream.ReadExactlyAsync(buffer, readCts.Token)
                .ConfigureAwait(false);

            return ReadAction.Proceed;
        }
        catch (EndOfStreamException)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Debug, 
                $"Connection {conn.Id} closed by peer."));
            return ReadAction.ReturnNow;
        }
        catch (OperationCanceledException)
            when (_shutdownToken.IsCancellationRequested)
        {
            conn.RequestDisconnect(SecureDisconnectReason.ServerShuttingDown);
            
            return ReadAction.BreakToTeardown;
        }
        catch (OperationCanceledException)
        {
            conn.RequestDisconnect(SecureDisconnectReason.Timeout);
            return ReadAction.BreakToTeardown;
        }
        catch(IOException ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Network error reading from {conn.RemoteEndPoint}: " +
                $"{ex.Message}", ex));

            return ReadAction.ReturnNow;
        }
    }

    private static void HandleDispatchResult(
        TcpConnection conn, DispatchResult result)
    {
        switch (result.Outcome)
        {
            case DispatchOutcome.Success:
                break;
            case DispatchOutcome.UnknownType:
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Unknown packet 0x{result.TypeId:X8} " +
                    $"from {conn.RemoteEndPoint}."));
                conn.RequestDisconnect(SecureDisconnectReason.MalformedPacket);
                break;
            case DispatchOutcome.InvalidPacket:
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                    $"Invalid packet 0x{result.TypeId:X8} " +
                    $"from {conn.RemoteEndPoint}: {result.Exception?.Message}", 
                    result.Exception));

                conn.RequestDisconnect(SecureDisconnectReason.MalformedPacket);
                break;
            case DispatchOutcome.HandlerException:
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                    $"Handler for 0x{result.TypeId:X8} threw " +
                    $"on connection {conn.Id}: {result.Exception?.Message}",
                    result.Exception));
                conn.RequestDisconnect(SecureDisconnectReason.InternalError);
                break;
        }
    }

    private async ValueTask SendDisconnectIfNeededAsync(TcpConnection conn)
    {
        SecureDisconnectReason reason = conn.DisconnectReason;
        
        if(reason != SecureDisconnectReason.None)
            await SendAsync(conn, new DisconnectPacket(reason))
                .ConfigureAwait(false);
    }

    private static string FormatEndpoint(TcpClient client)
    {
        try
        {
            return client.Client.RemoteEndPoint?.ToString() ?? "unknown";
        }
        catch
        {
            return "unknown";
        }
    }
}



/*
 *------------------------------------------------------------
 * (TcpHost.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */