/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 7/6/2026 2:23:18 PM UTC
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Buffers.Binary;
using LiteNetLib;
using LiteNetLib.Utils;
using Networking.Udp;
using SystemTools.Logger;
using SystemTools.Security;
using SystemTools.Storage;
using ZoneManager.Registration;

namespace Zone;

/// <summary>
/// Entry point for a Zone process: the dial side of the star topology.
/// Loads its registration seed, signs a zone-identity marker, dials
/// ZoneManager to register, and receives the registration-outcome packet
/// that confirms the round trip. It then idles; the sim loop, ECS, and
/// ticks arrive later.
/// </summary>
/// <remarks>
/// Boot order mirrors ZoneManager: <see cref="DiskManager"/> is initialized
/// first so <see cref="ZoneRegistrationKeyProvider.LoadSeed"/> has a live
/// storage layer, and the <c>finally</c> reverses it — stop the connector,
/// drain <see cref="Scribe"/>, then flush and shut down the disk layer last.
/// A missing zone-id arg or a missing/malformed seed throws; the catch logs
/// it and the finally still runs a clean teardown.
/// </remarks>
internal static class Program
{
    /// <summary>
    /// Loopback remote ZoneManager dials. Hardcoded to match ZoneManager's
    /// listen port for this brick; graduates to a <c>NetworkConfig</c> field
    /// (shared-tier fork, core project) when both sides must agree by config.
    /// </summary>
    private const string RemoteHost = "127.0.0.1";
    private const int RemotePort = 9050;

    /// <summary>
    /// Registration marker layout: a 4-byte big-endian zone id and an 8-byte
    /// big-endian timestamp form the signed message; the 64-byte Ed25519
    /// signature follows — 76 bytes total, carried raw as connection data.
    /// The zone id is inside the signed region so identity is bound to the
    /// signature and cannot be swapped after signing.
    /// </summary>
    private const int ZoneIdLength = sizeof(uint);
    private const int TimestampLength = sizeof(long);
    private const int SignedLength = ZoneIdLength + TimestampLength;

    /// <summary>
    /// Outcome-packet layout: a 4-byte big-endian type header followed by the
    /// 4-byte big-endian zone id payload — 8 bytes total.
    /// </summary>
    private const int TypeHeaderLength = sizeof(uint);
    private const int OutcomePayloadLength = sizeof(uint);
    private const int OutcomePacketLength =
        TypeHeaderLength + OutcomePayloadLength;

    private static UdpConnector? _connector;

    private static async Task Main(string[] args)
    {
        DiskManager.Initialize();

        try
        {
            // A zone never invents its identity: the id is supplied at launch
            // (Zone.exe <zoneId>). Missing, unparseable, or zero is a fatal
            // launch error, same idiom as a missing registration seed.
            if (args.Length < 1
                || !uint.TryParse(args[0], out uint zoneId)
                || zoneId == 0)
            {
                var ex = new InvalidOperationException(
                    "Zone requires a non-zero uint zone id as its first "
                    + "launch argument (e.g. Zone.exe 5).");

                Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                    "Zone launch argument missing or invalid.", ex));
                throw ex;
            }

            // Load-only: a missing seed is a fatal deploy error, not a cue to
            // generate. Throws here route into the finally teardown.
            byte[] seed = ZoneRegistrationKeyProvider.LoadSeed();

            // Signed message: [zoneId: 4B BE][timestamp: 8B BE]. The signed
            // bytes and the signature ride the wire as distinct fields so
            // ZoneManager can split, verify, and read the id as the key.
            Span<byte> signedMessage = stackalloc byte[SignedLength];
            BinaryPrimitives.WriteUInt32BigEndian(
                signedMessage[..ZoneIdLength], zoneId);
            BinaryPrimitives.WriteInt64BigEndian(
                signedMessage[ZoneIdLength..],
                DateTimeOffset.UtcNow.ToUnixTimeMilliseconds());

            byte[] signature = Ed25519MessageSigner.Sign(seed, signedMessage);

            // Connection data: [signed message: 12B][signature: 64B]. Raw
            // Put — no length prefix, matching the UDP wire convention.
            var connectionData = new NetDataWriter();
            connectionData.Put(signedMessage);
            connectionData.Put(signature);

            _connector = new UdpConnector(
                RemoteHost, RemotePort, connectionData);

            _connector.PeerConnected += OnPeerConnected;
            _connector.PeerDisconnected += OnPeerDisconnected;
            _connector.PacketReceived += OnPacketReceived;

            _connector.Start();

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Zone {zoneId} dialing ZoneManager at "
                + $"{RemoteHost}:{RemotePort} to register."));

            await WaitForShutdownAsync();
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                "Zone terminated by an unhandled exception.", ex));
        }
        finally
        {
            if (_connector is not null)
                await _connector.StopAsync();

            await Scribe.ShutdownAsync();

            if (DiskManager.IsRunning)
                await DiskManager.Instance.ShutdownAsync();
        }
    }

    // Peer established == the marker passed ZoneManager's gate. The outcome
    // packet that confirms registry entry follows on this connection and is
    // handled in OnPacketReceived.
    private static void OnPeerConnected(NetPeer peer)
    {
        Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
            $"Registration accepted by ZoneManager (peer {peer.Id})."));
    }

    // Fires on a rejected marker, a timed-out dial, or a later drop — the
    // connector makes one attempt and does not retry.
    private static void OnPeerDisconnected(
        NetPeer peer, DisconnectInfo disconnectInfo)
    {
        Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
            "Disconnected from ZoneManager "
            + $"({disconnectInfo.Reason})."));
    }

    // Receives the registration-outcome packet from ZoneManager. Reads the
    // 4-byte big-endian type header raw (never reader.GetUInt), matches the
    // control-plane RegistrationAccepted id, and reads back the echoed zone
    // id — closing the register round trip.
    private static void OnPacketReceived(NetPeer peer, NetPacketReader reader)
    {
        byte[] data = reader.GetRemainingBytes();

        if (data.Length != OutcomePacketLength)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Unexpected packet of {data.Length} bytes from "
                + "ZoneManager; ignoring."));
            return;
        }

        ReadOnlySpan<byte> span = data;
        uint typeId = BinaryPrimitives.ReadUInt32BigEndian(
            span[..TypeHeaderLength]);

        if (typeId != RegistrationControlIds.RegistrationAccepted)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Unexpected packet type 0x{typeId:X8} from ZoneManager; "
                + "ignoring."));
            return;
        }

        uint confirmedZoneId = BinaryPrimitives.ReadUInt32BigEndian(
            span[TypeHeaderLength..]);

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
            $"Registration confirmed by ZoneManager for zone "
            + $"{confirmedZoneId}."));
    }

    private static Task WaitForShutdownAsync()
    {
        var tcs = new TaskCompletionSource(
            TaskCreationOptions.RunContinuationsAsynchronously);

        Console.CancelKeyPress += (_, e) =>
        {
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