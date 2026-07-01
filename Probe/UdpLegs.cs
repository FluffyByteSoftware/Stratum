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

            if (typeId == PacketIds.Auth.UdpAuthAck)
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

            if (typeId == PacketIds.Auth.VersionChallenge)
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

            if (typeId == PacketIds.Auth.VersionResult)
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

            if (typeId == PacketIds.LifeCycle.Pong)
            {
                if (payload.AvailableBytes < sizeof(long))
                {
                    Console.WriteLine(
                        "    Pong payload too short; dropping.");
                    return;
                }
                echoedTimestampMs = payload.GetLong();
                pongReceived = true;
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

            if (pongReceived)
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