/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 6/13/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib;
using LiteNetLib.Utils;
using Networking.Dispatch;
using Networking.Udp;
using Shared.Networking;
using Shared.Networking.Packets.Auth;
using Shared.Networking.Packets.Comparable;
using Stratum.Networking.Dispatch;
using System;
using System.Buffers.Binary;
using System.Threading.Tasks;
using SystemTools.Logger;
using SystemTools.Security;
using SystemTools.Storage;

namespace Sentinel;


/// <summary>
/// Entry point for the Sentinel process: the UDP front door clients
/// connect to after authenticating over TCP against the LoginServer.
/// Validates the TCP-issued session token presented in each connection
/// request, tracks authenticated sessions, acknowledges admission, and
/// enforces protocol version compatibility before the session is live.
/// </summary>
internal static class Program
{
    private const string DataRoot = @"E:\Stratum\data";
    private const int UdpPort = 9998;

    private static readonly ClientSessionRegistry _registry = new();
    private static readonly PacketDispatcher<UdpConnection> _dispatcher = new();
    private static UdpHost? _host;

    private static async Task Main()
    {
        DiskManager.Initialize(DataRoot);

        try
        {
            byte[] signingKey = SessionKeyProvider.LoadOrCreate();
            SessionTokenIssuer.Initialize(signingKey);

            _dispatcher.Register(
                PacketIds.Auth.VersionResponse,
                static reader =>
                {
                    var p = new VersionResponsePacket();
                    p.Deserialize(reader);
                    return p;
                },
                OnVersionResponse);
            _dispatcher.Freeze();

            _host = new UdpHost(UdpPort);
            _host.ConnectionRequested += OnConnectionRequested;
            _host.PeerConnected += OnPeerConnected;
            _host.PeerDisconnected += OnPeerDisconnected;
            _host.PacketReceived += OnPacketReceived;

            _host.Start();

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Sentinel ready. UDP auth listening on port {UdpPort}."));

            await WaitForShutdownAsync();
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                "Sentinel terminated by an unhandled exception.", ex));
        }
        finally
        {
            if (_host is not null)
                await _host.StopAsync();

            await Scribe.ShutdownAsync();

            if (DiskManager.IsRunning)
                await DiskManager.Instance.ShutdownAsync();
        }
    }

    /// <summary>
    /// Validates the session token carried in a connection request and
    /// either accepts and registers the resulting session or rejects the
    /// request.
    /// </summary>
    private static void OnConnectionRequested(ConnectionRequest request)
    {
        string token = request.Data.GetString();

        if (!SessionTokenIssuer.TryValidate(
                token, out string accountId, out _))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Rejected UDP connection from {request.RemoteEndPoint}: " +
                "invalid or expired token."));
            request.Reject();
            return;
        }

        NetPeer peer = request.Accept();

        // Accept must precede registration because the registry is keyed
        // by peer id, which only exists once the peer is accepted. On the
        // single poll thread no other connection can interleave between
        // accept and register, so the rare double-connect rejection below
        // simply disconnects the peer we just accepted.
        if (!_registry.TryRegister(peer, accountId))
        {
            peer.Disconnect();
        }
    }

    /// <summary>
    /// Sends the authentication acknowledgement and version challenge
    /// once a peer is fully connected, but only if the peer holds a
    /// registered session. A peer that was accepted and then dropped
    /// during registration (a double-connect rejection) can still
    /// surface a connected event; the registry check ensures it
    /// receives neither packet.
    /// </summary>
    private static void OnPeerConnected(NetPeer peer)
    {
        if (!_registry.TryGet(peer.Id, out _))
            return;

        UdpHost.Send(peer, new UdpAuthPacket
        {
            Result = UdpAuthResult.Authenticated,
        });

        UdpHost.Send(peer, new VersionChallengePacket
        {
            Version = GameProtocolVersion.Current,
        });
    }

    /// <summary>
    /// Removes the session bound to a peer when it disconnects.
    /// </summary>
    private static void OnPeerDisconnected(
        NetPeer peer, DisconnectInfo disconnectInfo)
    {
        _registry.Remove(peer.Id);
    }

    /// <summary>
    /// Strips the 4-byte big-endian type header from an incoming
    /// datagram and dispatches the remainder to the registered handler.
    /// Drops the packet and logs a warning if the peer has no registered
    /// session or the datagram is too short to carry a type header.
    /// </summary>
    /// <remarks>
    /// All handlers fire on the single poll thread. <see cref="ValueTask"/>
    /// completion is awaited synchronously via <c>GetAwaiter().GetResult()</c>
    /// — safe here because there is no <see cref="System.Threading.
    /// SynchronizationContext"/> and handlers perform no genuine async I/O.
    /// </remarks>
    private static void OnPacketReceived(NetPeer peer, NetPacketReader reader)
    {
        if (!_registry.TryGet(peer.Id, out _))
            return;

        byte[] raw = reader.GetRemainingBytes();

        if (raw.Length < sizeof(uint))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Received undersized UDP packet from peer {peer.Id}; " +
                "dropping."));
            return;
        }

        uint typeId = BinaryPrimitives.ReadUInt32BigEndian(raw);

        var payloadReader = new NetDataReader();
        payloadReader.SetSource(raw, sizeof(uint), raw.Length - sizeof(uint));

        var connection = new UdpConnection(peer);

        ValueTask<DispatchResult> dispatch = _dispatcher
            .DispatchAsync(connection, typeId, payloadReader);

        if (!dispatch.IsCompleted)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                $"UDP dispatch for type 0x{typeId:X8} returned an incomplete " +
                "ValueTask; handlers must complete synchronously on the poll " +
                "thread."));
            return;
        }

        DispatchResult result = dispatch.GetAwaiter().GetResult();

        if (result.IsSuccess)
            return;

        if (result.Outcome == DispatchOutcome.UnknownType)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Unknown UDP packet type 0x{result.TypeId:X8} " +
                $"from peer {peer.Id}; dropping."));
            return;
        }

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
            $"Dispatch failure for UDP type 0x{result.TypeId:X8} " +
            $"(peer {peer.Id}): {result.Outcome}.",
            result.Exception));
    }

    /// <summary>
    /// Compares the version string reported by the client against
    /// <see cref="GameProtocolVersion.Current"/>. Sends
    /// <see cref="VersionResult.Ok"/> on match; sends
    /// <see cref="VersionResult.Mismatch"/> and disconnects on mismatch.
    /// </summary>
    private static ValueTask OnVersionResponse(
        UdpConnection connection, VersionResponsePacket packet)
    {
        if (packet.Version == GameProtocolVersion.Current)
        {
            connection.Send(new VersionResultPacket
            {
                Result = VersionResult.Ok,
            });

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Peer {connection.PeerId} passed version check " +
                $"({packet.Version})."));

            return ValueTask.CompletedTask;
        }

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
            $"Peer {connection.PeerId} version mismatch: " +
            $"expected {GameProtocolVersion.Current}, got {packet.Version}."));

        connection.Send(new VersionResultPacket
        {
            Result = VersionResult.Mismatch,
        });

        connection.Disconnect();

        return ValueTask.CompletedTask;
    }

    private static Task WaitForShutdownAsync()
    {
        var tcs = new TaskCompletionSource(
            TaskCreationOptions.RunContinuationsAsynchronously);

        Console.CancelKeyPress += (_, e) =>
        {
            // Suppress the default kill so the finally teardown can run.
            e.Cancel = true;
            tcs.TrySetResult();
        };

        return tcs.Task;
    }
}

/*
 *------------------------------------------------------------
 * (Program.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */