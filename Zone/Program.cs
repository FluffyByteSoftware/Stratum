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
using SystemTools.Clock;
using SystemTools.Logger;
using SystemTools.Security;
using SystemTools.Storage;
using ZoneManager.Registration;

namespace Zone;

/// <summary>
/// Entry point for a Zone process: the dial side of the star topology.
/// Loads its registration seed, signs a zone-identity marker, dials
/// ZoneManager to register, and starts the 60 Hz simulation clock only
/// once the registration-outcome packet confirms the registry entry. A
/// zone that fails to register terminates: no confirmation, no clock,
/// no process.
/// </summary>
/// <remarks>
/// Boot order mirrors ZoneManager: <see cref="DiskManager"/> is initialized
/// first so <see cref="ZoneRegistrationKeyProvider.LoadSeed"/> has a live
/// storage layer. The Heartbeat is initialized and its systems registered
/// in <c>Main</c> (registration must precede <see cref="Heartbeat.Start"/>
/// by contract), but <c>Start</c> is called from the connector's poll
/// thread inside the confirmed-outcome branch — the clock thread does not
/// exist until the zone is registered. A disconnect before confirmation
/// (rejected marker, dial timeout, duplicate drop) signals the shutdown
/// task, routing through the same <c>finally</c> teardown as Ctrl+C. A
/// drop after confirmation is a Warn only; reconnect-versus-terminate for
/// a lost ZoneManager is a deliberate later fork. Teardown: Heartbeat
/// first (foreground thread; producer stops before the sink drains), then
/// connector, Scribe, DiskManager.
/// </remarks>
internal static class Program
{
    /// <summary>
    /// Loopback host the Zone dials to reach ZoneManager. Zone-local — the
    /// dial target, not part of the shared registration contract. The port
    /// comes from <see cref="RegistrationTransport.Port"/>: ZoneManager owns
    /// it, Zone reads it through the one-way reference.
    /// </summary>
    private const string RemoteHost = "127.0.0.1";

    /// <summary>
    /// The zone simulation master tick rate. 60 Hz per the simulation
    /// design; passed explicitly because <see cref="Heartbeat.Initialize"/>
    /// defaults to 30.
    /// </summary>
    private const int TickRateHz = 60;

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
    private static readonly TickWitness _tickWitness = new();

    /// <summary>
    /// Completed by Ctrl+C or by a pre-confirmation disconnect; Main awaits
    /// it and falls into teardown. Static so the connector callbacks can
    /// signal termination through the same path as an operator stop.
    /// </summary>
    private static readonly TaskCompletionSource _shutdownSignal = new(
        TaskCreationOptions.RunContinuationsAsynchronously);

    /// <summary>
    /// True once the registration-outcome packet has confirmed this zone's
    /// registry entry. Written and read only on the connector's poll thread
    /// (all three peer callbacks fire there), so no synchronization is
    /// needed. Discriminates a fatal pre-confirmation disconnect from a
    /// post-confirmation drop.
    /// </summary>
    private static bool _confirmed;

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

            // Clock is initialized and its systems registered here — Register
            // must precede Start by Heartbeat's contract — but Start happens
            // in OnPacketReceived, only on a confirmed registration.
            Heartbeat.Initialize(TickRateHz);
            Heartbeat.Instance.Register(_tickWitness);

            _connector = new UdpConnector(
                RemoteHost, RegistrationTransport.Port, connectionData);

            _connector.PeerConnected += OnPeerConnected;
            _connector.PeerDisconnected += OnPeerDisconnected;
            _connector.PacketReceived += OnPacketReceived;

            _connector.Start();

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Zone {zoneId} dialing ZoneManager at "
                + $"{RemoteHost}:{RegistrationTransport.Port} to register."));

            await WaitForShutdownAsync();
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                "Zone terminated by an unhandled exception.", ex));
        }
        finally
        {
            // Heartbeat first: its thread is foreground (skip this and the
            // process hangs) and it produces log traffic (stop the producer
            // before draining the sink). IsRunning only says Initialize ran;
            // StopAsync on a never-started Heartbeat is a safe no-op.
            if (Heartbeat.IsRunning)
            {
                await Heartbeat.Instance.StopAsync();

                Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                    $"Clock stopped. Ticks={Heartbeat.Instance.CurrentTick}, "
                    + $"witnessed={_tickWitness.TickCount}, "
                    + $"measured={Heartbeat.Instance.MeasuredHz:F2} Hz."));
            }

            if (_connector is not null)
                await _connector.StopAsync();

            await Scribe.ShutdownAsync();

            if (DiskManager.IsRunning)
                await DiskManager.Instance.ShutdownAsync();
        }
    }

    // Peer established == the marker passed ZoneManager's gate. Not yet
    // registered: the registry insert is confirmed by the outcome packet,
    // and first-wins can still reject a duplicate after this fires.
    private static void OnPeerConnected(NetPeer peer)
    {
        Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
            $"Registration accepted by ZoneManager (peer {peer.Id})."));
    }

    // Discriminates on _confirmed: a disconnect before the outcome packet
    // (rejected marker, dial timeout, duplicate drop) means this zone has no
    // registry entry and no purpose — terminate. A drop after confirmation
    // is logged only; reconnect-vs-terminate is a deliberate later fork.
    private static void OnPeerDisconnected(
        NetPeer peer, DisconnectInfo disconnectInfo)
    {
        if (!_confirmed)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                "Registration failed: disconnected before confirmation "
                + $"({disconnectInfo.Reason}). Terminating."));
            _shutdownSignal.TrySetResult();
            return;
        }

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
            "Disconnected from ZoneManager "
            + $"({disconnectInfo.Reason})."));
    }

    // Receives the registration-outcome packet from ZoneManager. Reads the
    // 4-byte big-endian type header raw (never reader.GetUInt), matches the
    // control-plane RegistrationAccepted id, reads back the echoed zone id —
    // and starts the simulation clock: registered is what licenses ticking.
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

        if (_confirmed)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                "Duplicate registration outcome from ZoneManager; "
                + "ignoring."));
            return;
        }

        _confirmed = true;

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
            $"Registration confirmed by ZoneManager for zone "
            + $"{confirmedZoneId}."));

        Heartbeat.Instance.Start();

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
            $"Zone {confirmedZoneId} simulation clock started at "
            + $"{TickRateHz} Hz."));
    }

    // Heartbeat exposes no "was Start called" query; track it locally. Poll
    // thread only, like _confirmed.
    private static bool _heartbeatStarted;

    private static bool HeartbeatStarted()
    {
        if (_heartbeatStarted)
            return true;

        _heartbeatStarted = true;
        return false;
    }

    private static Task WaitForShutdownAsync()
    {
        Console.CancelKeyPress += (_, e) =>
        {
            e.Cancel = true;
            _shutdownSignal.TrySetResult();
        };

        return _shutdownSignal.Task;
    }
}

/*
 *------------------------------------------------------------
 * (Program.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */