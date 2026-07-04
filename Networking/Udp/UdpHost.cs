/*
 * (UdpHost.cs)
 *------------------------------------------------------------
 * Created - 6/13/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib;
using LiteNetLib.Utils;
using Shared.Networking;
using System.Buffers.Binary;
using System.Net;
using System.Net.Sockets;
using SystemTools.Logger;

namespace Networking.Udp;

/// <summary>
/// Represents a UDP server host built on LiteNetLib that listens for
/// incoming connection requests and established-peer traffic, surfacing
/// each as an event for a consumer to apply policy against.
/// </summary>
/// <remarks>
/// The host owns a single dedicated polling thread that drives
/// LiteNetLib's <see cref="NetManager.PollEvents"/> on a fixed cadence;
/// all listener callbacks fire on that thread. The host is transport
/// only: it makes no authentication or dispatch decisions. Connection
/// acceptance, session tracking, and packet handling are the consumer's
/// responsibility, wired through the events this class raises. Use
/// <see cref="StopAsync"/> for graceful shutdown.
/// </remarks>
public sealed class UdpHost : INetEventListener
{
    private const int PollIntervalMs = 15;

    private readonly int _port;
    private readonly UdpBindMode _bindMode;
    private readonly NetManager _net;
    private readonly CancellationTokenSource _shutdownCts = new();
    private readonly CancellationToken _shutdownToken;

    private Thread? _pollThread;
    private int _started;
    private int _stopped;

    /// <summary>
    /// Raised when a remote endpoint requests a connection. The consumer
    /// inspects the request's connection data and calls
    /// <see cref="ConnectionRequest.Accept"/> or
    /// <see cref="ConnectionRequest.Reject()"/> to decide admission.
    /// </summary>
    /// <remarks>
    /// Fires on the polling thread. No peer exists yet; this is the only
    /// place admission can be denied before a peer is established.
    /// </remarks>
    public event Action<ConnectionRequest>? ConnectionRequested;

    /// <summary>
    /// Raised when a peer has been accepted and a connection
    /// established. Fires on the polling thread.
    /// </summary>
    public event Action<NetPeer>? PeerConnected;

    /// <summary>
    /// Raised when a peer disconnects, for any reason. Fires on the
    /// polling thread.
    /// </summary>
    public event Action<NetPeer, DisconnectInfo>? PeerDisconnected;

    /// <summary>
    /// Raised when a packet is received from an established peer. Fires
    /// on the polling thread. The reader is owned by LiteNetLib and is
    /// only valid for the duration of the callback.
    /// </summary>
    /// <remarks>
    /// No packets are dispatched by the host in this version; the path
    /// is surfaced so unexpected traffic on an authenticated peer is
    /// observable. A dispatcher is introduced when real post-auth
    /// packets exist.
    /// </remarks>
    public event Action<NetPeer, NetPacketReader>? PacketReceived;

    /// <summary>
    /// Initializes a new instance of the <see cref="UdpHost"/> class.
    /// </summary>
    /// <param name="port">The port number to listen on (1-65535). The
    /// socket binds on all local interfaces.</param>
    /// <param name="bindMode">Specifies which local interfaces the host should bind to. Defaults to all interfaces.</param>
    /// <exception cref="ArgumentOutOfRangeException"><paramref name="port"/>
    /// is less than 1 or greater than 65535.</exception>
    public UdpHost(int port, UdpBindMode bindMode = UdpBindMode.AllInterfaces)
    {
        if (port is < 1 or > 65535)
            throw new ArgumentOutOfRangeException(
                nameof(port), "Port must be in the range 1-65535.");

        _port = port;
        
        _bindMode = bindMode;

        _net = new NetManager(this);
        _shutdownToken = _shutdownCts.Token;
        _net.IPv6Enabled = false;
    }

    /// <summary>
    /// Starts the UDP host and begins polling for network events on a
    /// dedicated background thread.
    /// </summary>
    /// <exception cref="InvalidOperationException">The host is already
    /// started.</exception>
    /// <exception cref="InvalidOperationException">The underlying socket
    /// failed to start.</exception>
    public void Start()
    {
        if (Interlocked.Exchange(ref _started, 1) != 0)
            throw new InvalidOperationException("Host is already started.");

        bool started = _bindMode switch
        {
            // Loopback-only binds the loopback address pair. The IPv6
            // address is inert here (IPv6Enabled is false, so the v6
            // socket never binds) but the overload requires the slot;
            // the loopback value is passed for honest intent, not effect.
            UdpBindMode.LoopbackOnly => _net.Start(
                IPAddress.Loopback, IPAddress.IPv6Loopback, _port),

            // All-interfaces is the port-only Start — byte-for-byte the
            // pre-bind-mode behavior every existing consumer relies on.
            _ => _net.Start(_port),
        };

        if (!started)
            throw new InvalidOperationException(
                $"Failed to start UDP socket on port {_port}.");

        _pollThread = new Thread(PollLoop)
        {
            Name = $"UdpHost:{_port}",
            IsBackground = true,
        };

        _pollThread.Start();

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
            $"Udp Host listening on port: {_port}."));
    }

    /// <summary>
    /// Stops the host, ending the poll loop and closing the socket.
    /// </summary>
    /// <remarks>This method is safe to call multiple times. Subsequent
    /// calls return immediately.</remarks>
    /// <returns>A task that completes once the poll thread has joined and
    /// the socket is stopped.</returns>
    public async Task StopAsync()
    {
        if (Interlocked.Exchange(ref _stopped, 1) != 0)
            return;

        _shutdownCts.Cancel();

        Thread? thread = _pollThread;

        if (thread is not null)
        {
            try
            {
                await Task.Run(() => thread.Join()).ConfigureAwait(false);
            }
            catch
            {
            }
        }

        try
        {
            _net.Stop();
        }
        catch
        {
        }

        _shutdownCts.Dispose();
    }

    /// <summary>
    /// Serializes and sends a packet to a single peer over UDP.
    /// </summary>
    /// <typeparam name="T">The packet type, a value type implementing
    /// <see cref="IPacketWritable"/>.</typeparam>
    /// <param name="peer">The destination peer.</param>
    /// <param name="packet">The packet to serialize and send.</param>
    /// <remarks>
    /// The datagram is framed as a 4-byte big-endian type identifier
    /// followed by the serialized payload — no length prefix, since the
    /// datagram boundary delimits the message. Sent reliably and ordered
    /// on channel 0.
    /// </remarks>
    public static void Send<T>(NetPeer peer, T packet)
        where T : struct, IPacketWritable
    {
        ArgumentNullException.ThrowIfNull(peer);

        var writer = new NetDataWriter();

        Span<byte> typeHeader = stackalloc byte[sizeof(uint)];
        BinaryPrimitives.WriteUInt32BigEndian(typeHeader, packet.TypeId);
        writer.Put(typeHeader);

        packet.Serialize(writer);

        peer.Send(writer, 0, DeliveryMethod.ReliableOrdered);
    }

    private void PollLoop()
    {
        while (!_shutdownToken.IsCancellationRequested)
        {
            try
            {
                _net.PollEvents();
            }
            catch (Exception ex)
            {
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                    $"UDP poll error: {ex.Message}", ex));
            }

            Thread.Sleep(PollIntervalMs);
        }
    }

    void INetEventListener.OnConnectionRequest(ConnectionRequest request)
        => ConnectionRequested?.Invoke(request);

    void INetEventListener.OnPeerConnected(NetPeer peer)
        => PeerConnected?.Invoke(peer);

    void INetEventListener.OnPeerDisconnected(
        NetPeer peer, DisconnectInfo disconnectInfo)
        => PeerDisconnected?.Invoke(peer, disconnectInfo);

    void INetEventListener.OnNetworkReceive(
        NetPeer peer,
        NetPacketReader reader,
        byte channelNumber,
        DeliveryMethod deliveryMethod)
    {
        try
        {
            PacketReceived?.Invoke(peer, reader);
        }
        finally
        {
            reader.Recycle();
        }
    }

    void INetEventListener.OnNetworkReceiveUnconnected(
        IPEndPoint remoteEndPoint,
        NetPacketReader reader,
        UnconnectedMessageType messageType)
    {
        // Unconnected messages are not used; ignore and recycle.
        reader.Recycle();
    }

    void INetEventListener.OnNetworkError(
        IPEndPoint endPoint, SocketError socketError)
    {
        Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
            $"UDP socket error from {endPoint}: {socketError}."));
    }

    void INetEventListener.OnNetworkLatencyUpdate(NetPeer peer, int latency)
    {
        // No latency handling required for the auth host.
    }
}

/*
 *------------------------------------------------------------
 * (UdpHost.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */