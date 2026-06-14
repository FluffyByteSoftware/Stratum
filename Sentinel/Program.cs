/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 6/13/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib;
using Networking.Udp;
using Shared.Networking.Packets.Auth;
using System;
using System.Threading.Tasks;
using SystemTools.Logger;
using SystemTools.Security;
using SystemTools.Storage;

namespace Sentinel;

/// <summary>
/// Entry point for the Sentinel process: the UDP front door clients
/// connect to after authenticating over TCP against the LoginServer.
/// Validates the TCP-issued session token presented in each connection
/// request, tracks authenticated sessions, and acknowledges admission.
/// </summary>
internal static class Program
{
    private const string DataRoot = @"E:\Stratum\data";
    private const int UdpPort = 9998;

    private static readonly ClientSessionRegistry _registry = new();
    private static UdpHost? _host;

    private static async Task Main()
    {
        DiskManager.Initialize(DataRoot);

        try
        {
            byte[] signingKey = SessionKeyProvider.LoadOrCreate();
            SessionTokenIssuer.Initialize(signingKey);

            _host = new UdpHost(UdpPort);
            _host.ConnectionRequested += OnConnectionRequested;
            _host.PeerConnected += OnPeerConnected;
            _host.PeerDisconnected += OnPeerDisconnected;

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
    /// Sends the authentication acknowledgement once a peer is fully
    /// connected, but only if the peer holds a registered session. A peer
    /// that was accepted and then dropped during registration (a
    /// double-connect rejection) can still surface a connected event; the
    /// registry check ensures it receives no acknowledgement.
    /// </summary>
    private static void OnPeerConnected(NetPeer peer)
    {
        if (!_registry.TryGet(peer.Id, out _))
            return;

        UdpHost.Send(peer, new UdpAuthPacket
        {
            Result = UdpAuthResult.Authenticated,
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