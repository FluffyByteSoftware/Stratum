/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 7/4/2026 11:12:50 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Buffers.Binary;
using LiteNetLib;
using Networking.Udp;
using SystemTools.Logger;
using SystemTools.Security;
using SystemTools.Storage;
using ZoneManager.Registration;
using ZoneManager.Zones;

namespace ZoneManager;

/// <summary>
/// Entry point for the ZoneManager process: the star-topology hub the
/// Zone processes dial outward to and register against over UDP. This
/// brick verifies dialing zones against the zone-registration public key,
/// confirms each accepted zone with a registration-outcome packet, tracks
/// live zones in the registry, and evicts a zone's entry when its peer
/// disconnects so a crashed zone can relaunch and re-register; the death
/// clock arrives in a later brick.
/// </summary>
/// <remarks>
/// Boot order mirrors LoginServer and Sentinel: <see cref="DiskManager"/>
/// is initialized first so every downstream storage call has a live disk
/// layer, and teardown in the <c>finally</c> reverses it — stop the host,
/// drain <see cref="Scribe"/>, then flush and shut down the disk layer
/// last so any final writes are persisted.
/// </remarks>
internal static class Program
{
    /// <summary>
    /// The zone-registration public key ZoneManager verifies signatures
    /// against. Loaded at boot to exercise the provider's first real call
    /// — which generates and clones the keypair on genuine first boot —
    /// and held for the verify path that reads it in the registration
    /// round trip.
    /// </summary>
    private static byte[]? _registrationPublicKey;

    /// <summary>
    /// Registration marker layout: a 4-byte big-endian zone id and an 8-byte
    /// big-endian timestamp form the signed message; the 64-byte Ed25519
    /// signature follows — 76 bytes total, carried raw as connection data.
    /// The zone id is inside the signed region, so a verified signature
    /// authenticates the identity that then keys the registry.
    /// </summary>
    private const int ZoneIdLength = sizeof(uint);
    private const int TimestampLength = sizeof(long);
    private const int SignedLength = ZoneIdLength + TimestampLength;
    private const int SignatureLength = 64;
    private const int RegistrationMarkerLength =
        SignedLength + SignatureLength;

    private static UdpHost? _host;
    private static readonly ZoneRegistry _registry = new();

    private static async Task Main()
    {
        DiskManager.Initialize();

        ZoneStore.Initialize();

        try
        {
            _registrationPublicKey =
                ZoneRegistrationKeyProvider.LoadOrCreatePublic();

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                "Zone registration public key loaded "
                + $"({_registrationPublicKey.Length} bytes)."));

            _host = new UdpHost(RegistrationTransport.Port, UdpBindMode.LoopbackOnly);
            _host.ConnectionRequested += OnConnectionRequested;
            _host.PeerConnected += OnPeerConnected;
            _host.PeerDisconnected += OnPeerDisconnected;
            _host.Start();

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                "ZoneManager ready. UDP registration listening on "
                + $"loopback port {RegistrationTransport.Port}."));

            await WaitForShutdownAsync();
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                "ZoneManager terminated by an unhandled exception.", ex));
        }
        finally
        {
            if (_host is not null)
                await _host.StopAsync();

            if (ZoneStore.IsRunning)
                await ZoneStore.Instance.ShutdownAsync();

            await Scribe.ShutdownAsync();

            if (DiskManager.IsRunning)
                await DiskManager.Instance.ShutdownAsync();
        }
    }

    /// <summary>
    /// Admits or rejects a dialing zone by verifying its registration marker
    /// against the zone-registration public key. Fires on the host poll
    /// thread. A peer is established only if the signature over
    /// [zoneId][timestamp] verifies — so a live peer structurally means
    /// "passed the gate," and the zone id it registered under is
    /// authenticated. On accept, the verified zone id is stashed on the
    /// peer's <see cref="NetPeer.Tag"/> so <see cref="OnPeerConnected"/> can
    /// confirm it once the connection is established.
    /// </summary>
    private static void OnConnectionRequested(ConnectionRequest request)
    {
        byte[] marker = request.Data.GetRemainingBytes();

        if (marker.Length != RegistrationMarkerLength)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Rejected registration: marker was {marker.Length} bytes, "
                + $"expected {RegistrationMarkerLength}."));
            request.Reject();
            return;
        }

        ReadOnlySpan<byte> markerSpan = marker;
        ReadOnlySpan<byte> signedMessage = markerSpan[..SignedLength];
        ReadOnlySpan<byte> signature = markerSpan[SignedLength..];

        // Signature-only this session: proves the dialer holds the seed and
        // authenticates the embedded zone id. Timestamp-freshness (replay
        // window) is the deferred hardening pass.
        if (_registrationPublicKey is null
            || !Ed25519Verifier.Verify(
                _registrationPublicKey, signedMessage, signature))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                "Rejected registration: signature failed verification."));
            request.Reject();
            return;
        }

        uint zoneId = BinaryPrimitives.ReadUInt32BigEndian(
            signedMessage[..ZoneIdLength]);

        // Accept() returns the established peer. Carry the authenticated zone
        // id forward on the peer Tag — the outcome send happens later, in
        // OnPeerConnected, once the connection actually exists.
        NetPeer peer = request.Accept();
        peer.Tag = zoneId;

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
            $"Accepted zone {zoneId} registration (signature verified)."));
    }

    /// <summary>
    /// Fires once an accepted peer's connection is established. Reads the
    /// authenticated zone id off the peer Tag and sends the registration
    /// outcome packet back to the zone. Fires on the host poll thread.
    /// </summary>
    private static void OnPeerConnected(NetPeer peer)
    {
        if (peer.Tag is not uint zoneId)
        {
            // No Tag means the peer reached connected without passing through
            // the accept path that sets it — treat as anomalous and drop.
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Peer {peer.Id} connected without a zone id tag; "
                + "dropping."));
            peer.Disconnect();
            return;
        }

        // First-wins: a duplicate live zone id is rejected here, before any
        // acceptance is sent. The dialer's signature verified, but the id is
        // already registered — drop it rather than overwrite the live entry.
        if (!_registry.TryRegister(zoneId, new UdpConnection(peer)))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Rejected zone {zoneId}: id already registered; "
                + "dropping duplicate dialer."));
            peer.Disconnect();
            return;
        }

        UdpHost.Send(peer, new ZoneRegistrationAcceptedPacket(zoneId));

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
            $"Registered and confirmed zone {zoneId}."));
    }

    /// <summary>
    /// Fires when a peer disconnects for any reason — graceful close, our
    /// own duplicate-reject drop, or crash timeout. Evicts the disconnecting
    /// peer's registry entry, keyed by peer id, so a crashed or relaunched
    /// zone can re-register. Fires on the host poll thread.
    /// </summary>
    /// <remarks>
    /// Eviction is keyed on <see cref="NetPeer.Id"/>, not the zone id on the
    /// peer Tag: a rejected duplicate dialer disconnects carrying the same
    /// zone id as the live incumbent, so evicting by zone id would drop the
    /// wrong entry. The duplicate was never stored, so its peer id matches
    /// nothing and the incumbent survives — no guard needed. The no-match
    /// case is logged at Debug so a quiet non-eviction is visible evidence,
    /// not just an absent line.
    /// </remarks>
    private static void OnPeerDisconnected(
        NetPeer peer, DisconnectInfo disconnectInfo)
    {
        if (_registry.TryUnregister(peer.Id, out uint zoneId))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Evicted zone {zoneId}: peer {peer.Id} disconnected "
                + $"({disconnectInfo.Reason})."));
            return;
        }

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Debug,
            $"Peer {peer.Id} disconnected ({disconnectInfo.Reason}) "
            + "with no live registry entry."));
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