/*
 * (UdpLegs.cs)
 *------------------------------------------------------------
 * Created - 6/29/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib;
using LiteNetLib.Utils;
using Shared.Networking;
using Shared.Networking.Packets.Auth;
using Shared.Networking.Packets.Comparable;
using Shared.Networking.Packets.Diagnostics;
using Shared.Networking.Packets.LifeCycle;
using System.Buffers.Binary;

namespace Probe;

/// <summary>
/// Legs 3, 4, and 6 of the probe: UDP session authentication against Sentinel
/// using the token a successful Ok auth minted, the protocol version check that
/// gates the session, and a single keep-alive round trip that proves the echo
/// handler once the session is live.
/// </summary>
/// <remarks>All three legs run over one persistent LiteNetLib connection - the
/// session token is presented as the connection-request data, the version
/// challenge/response/result exchange follows the auth ack on the same peer, and
/// the ping/pong echo follows a passing version check on that same peer before
/// the connection is torn down. Reached only when an auth leg returned Ok; a
/// character-less account stops at NeedsCharacter before a token ever exists.
/// The ping is single-shot by design: this is a regression check that the echo
/// works, not the recurring client-side keep-alive cadence (which LiteNetLib's
/// own internal keepalive largely obviates anyway).</remarks>
internal static class UdpLegs
{
    // Sentinel's UDP endpoint for testing. Host matches the TCP target; port
    // is Sentinel's listener. The advertised UdpEndpoint from the auth
    // response carries the same value once its placeholder host is real.
    private const string SentinelHost = "10.0.0.84";
    private const int SentinelPort = 9998;

    // Bumped from 3 s to accommodate the extra version-check round trips
    // (challenge → response → result) and the ping/pong echo on top of the
    // auth ack.
    private static readonly TimeSpan UdpAuthTimeout =
        TimeSpan.FromSeconds(5);

    // Legs 3, 4, and 6: stand up a LiteNetLib client, present the session
    // token as connection-request data, wait for Sentinel's auth ack, handle
    // the version challenge/response/result exchange, then fire a single
    // keep-alive ping and confirm the echoed pong - all over the same peer.
    internal static void UdpAuth(string token)
    {
        var listener = new EventBasedNetListener();
        var client = new NetManager(listener);

        UdpAuthResult authResult = UdpAuthResult.None;
        VersionResult versionResult = VersionResult.None;
        bool authAcked = false;
        bool versionChecked = false;
        bool disconnected = false;

        // Leg 6 state. The ping is sent inline the moment the version result
        // arrives Ok; sentTimestampMs is the value we expect echoed back, so a
        // mismatch on the pong is a real failure rather than a timeout.
        long sentTimestampMs = 0;
        bool pingSent = false;
        bool pongReceived = false;
        long echoedTimestampMs = 0;
        
        // Leg 7 State.  The gameplay-channel diagnostic ping chains off the pong
        // the same way the ping chained off the version result: fire the moment
        // the prior leg closes, over the same peer.  The nonce is random rather
        // than a timestamp - this leg proves 0x03 routing, not liveness
        // and a random value makes an accidental echo-by-luck (stale buffer,
        // wrong field) vanishingly unlikely.
        long sentNonce = 0;
        bool diagPingSent = false;
        bool diagPongReceived = false;
        long echoedNonce = 0;


        listener.PeerConnectedEvent += _ =>
            Console.WriteLine("    Connected; awaiting UDP auth ack.");

        listener.PeerDisconnectedEvent += (_, info) =>
        {
            disconnected = true;
            Console.WriteLine($"    Disconnected: {info.Reason}.");
        };

        listener.NetworkReceiveEvent += (peer, reader, _, _) =>
        {
            byte[] data = reader.GetRemainingBytes();
            reader.Recycle();

            if (data.Length < sizeof(uint))
            {
                Console.WriteLine(
                    $"    Packet too short ({data.Length} bytes); dropping.");
                return;
            }

            uint typeId = BinaryPrimitives.ReadUInt32BigEndian(data);

            var payload = new NetDataReader();
            // SetSource maxSize is absolute, not a count from offset:
            // AvailableBytes = maxSize - position. Pass data.Length, not
            // data.Length - sizeof(uint), or every read underflows.
            payload.SetSource(data, sizeof(uint), data.Length);

            if (typeId == MessagePacketIds.AuthMessage.UdpAuthAck)
            {
                if (payload.AvailableBytes < 1)
                {
                    Console.WriteLine("    Auth ack payload too short; dropping.");
                    return;
                }
                authResult = (UdpAuthResult)payload.GetByte();
                authAcked = true;
                return;
            }

            if (typeId == MessagePacketIds.AuthMessage.VersionChallenge)
            {
                string serverVersion = payload.GetString();
                Console.WriteLine(
                    $"    Version challenge: server={serverVersion}; " +
                    $"responding with client={GameProtocolVersion.Current}.");

                SendUdpPacket(peer, new VersionResponsePacket
                {
                    Version = GameProtocolVersion.Current,
                });
                return;
            }

            if (typeId == MessagePacketIds.AuthMessage.VersionResult)
            {
                if (payload.AvailableBytes < 1)
                {
                    Console.WriteLine(
                        "    Version result payload too short; dropping.");
                    return;
                }
                versionResult = (VersionResult)payload.GetByte();
                versionChecked = true;

                // Client-initiated: the moment the session is confirmed live,
                // fire one ping and record what we sent so the pong can be
                // checked for an exact echo. No ping on a mismatched version -
                // that session is already being torn down by Sentinel.
                if (versionResult == VersionResult.Ok)
                {
                    sentTimestampMs =
                        DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();

                    SendUdpPacket(peer, new PingPacket(sentTimestampMs));
                    pingSent = true;

                    Console.WriteLine(
                        $"    Ping sent (ts={sentTimestampMs}); " +
                        "awaiting pong echo.");
                }
                return;
            }

            if (typeId == MessagePacketIds.LifeCycleMessage.Pong)
            {
                if (payload.AvailableBytes < sizeof(long))
                {
                    Console.WriteLine(
                        "    Pong payload too short; dropping.");
                    return;
                }
                echoedTimestampMs = payload.GetLong();
                pongReceived = true;

                // Leg 6 closed; fire leg 7 on the same peer.
                sentNonce = Random.Shared.NextInt64();

                SendUdpPacket(peer,
                    new GameDiagnosticPingPacket(sentNonce));

                diagPingSent = true;

                Console.WriteLine(
                    $"    OK    [7] Diagnostic ping sent (nonce={sentNonce}).");
                Console.WriteLine("Awaiting 0x03 echo...");

                return;
            }

            if (typeId == MessagePacketIds.ZoneDataMessage.Pong && diagPingSent)
            {
                if (payload.AvailableBytes < sizeof(long))
                {
                    Console.WriteLine(
                        "    Diagnostic pong payload too short; dropping.");
                    return;
                }

                echoedNonce = payload.GetLong();
                diagPongReceived = true;
                return;
            }

            Console.WriteLine(
                $"    Unexpected type 0x{typeId:X8}; dropping.");
        };

        client.Start();
        client.Connect(SentinelHost, SentinelPort, token);

        var deadline = DateTime.UtcNow + UdpAuthTimeout;

        // Poll until the ping round trip closes (pong received), or - on any
        // path where no ping was sent (auth reject, version mismatch) - until
        // the version check resolves and no ping is pending. The pingSent /
        // pongReceived pair keeps the loop alive across the extra round trip
        // that legs 3-4 alone did not need.
        while (!disconnected && DateTime.UtcNow < deadline)
        {
            client.PollEvents();
            Thread.Sleep(15);

            if (diagPongReceived)
                break;

            if (versionChecked && !pingSent)
                break;
        }

        client.Stop();

        // Leg 3 report
        if (!authAcked)
        {
            Console.WriteLine(
                "    FAIL  [3] No UDP auth ack (rejected or timed out).");
            return;
        }

        if (authResult != UdpAuthResult.Authenticated)
        {
            Console.WriteLine($"    FAIL  [3] Auth result was {authResult}.");
            return;
        }

        Console.WriteLine("    OK    [3] UDP session authenticated.");

        // Leg 4 report
        if (!versionChecked)
        {
            Console.WriteLine(
                "    FAIL  [4] No version result received (timed out).");
            return;
        }

        if (versionResult != VersionResult.Ok)
        {
            Console.WriteLine(
                $"    FAIL  [4] Version result was {versionResult}.");
            return;
        }

        Console.WriteLine("    OK    [4] Protocol version accepted.");

        // Leg 6 report - only meaningful once the session passed version check,
        // which is the only path that sends a ping.
        if (!pongReceived)
        {
            Console.WriteLine(
                "    FAIL  [6] No pong echo received (timed out).");
            return;
        }

        if (echoedTimestampMs != sentTimestampMs)
        {
            Console.WriteLine(
                $"    FAIL  [6] Pong echo mismatch: sent {sentTimestampMs}, " +
                $"got {echoedTimestampMs}.");
            return;
        }

        Console.WriteLine(
            $"    OK    [6] Keep-alive echo verified (ts={echoedTimestampMs}).");

        Console.WriteLine("         [7] Gameplay channel diagnostic echo...");

        if (!diagPongReceived)
        {
            Console.WriteLine(
                "   FAIL [7] No diagnostic echo received (timed out).");
            return;
        }

        if(echoedNonce != sentNonce)
        {
            Console.WriteLine(
                $"    FAIL  [7] Diagnostic echo mismatch: sent {sentNonce}, " +
                $"got {echoedNonce}.");
            return;
        }

        Console.WriteLine("[7] Gameplay Diagnostic successful.");
    }

    // Serializes and sends a single UDP packet to a peer with the 4-byte
    // big-endian type header. Mirrors UdpHost.Send but uses a heap-allocated
    // header array because stackalloc is unavailable inside lambda closures.
    private static void SendUdpPacket<T>(NetPeer peer, T packet)
        where T : struct, IPacketWritable
    {
        var writer = new NetDataWriter();

        byte[] typeHeader = new byte[sizeof(uint)];
        BinaryPrimitives.WriteUInt32BigEndian(typeHeader, packet.TypeId);
        writer.Put(typeHeader);

        packet.Serialize(writer);

        peer.Send(writer, 0, DeliveryMethod.ReliableOrdered);
    }
}

/*
 *------------------------------------------------------------
 * (UdpLegs.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */