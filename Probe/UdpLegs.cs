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
using System.Buffers.Binary;

namespace Probe;

/// <summary>
/// Legs 3 and 4 of the probe: UDP session authentication against Sentinel using
/// the token a successful Ok auth minted, immediately followed by the protocol
/// version check that gates the session.
/// </summary>
/// <remarks>Both legs run over one persistent LiteNetLib connection - the
/// session token is presented as the connection-request data, and the version
/// challenge/response/result exchange follows the auth ack on the same peer.
/// Reached only when an auth leg returned Ok; a character-less account stops at
/// NeedsCharacter before a token ever exists.</remarks>
internal static class UdpLegs
{
    // Sentinel's UDP endpoint for testing. Host matches the TCP target; port
    // is Sentinel's listener. The advertised UdpEndpoint from the auth
    // response carries the same value once its placeholder host is real.
    private const string SentinelHost = "10.0.0.84";
    private const int SentinelPort = 9998;

    // Bumped from 3 s to accommodate the extra version-check round trips
    // (challenge → response → result) on top of the auth ack.
    private static readonly TimeSpan UdpAuthTimeout =
        TimeSpan.FromSeconds(5);

    // Legs 3 and 4: stand up a LiteNetLib client, present the session token
    // as connection-request data, wait for Sentinel's auth ack, then handle
    // the version challenge/response/result exchange over the same connection.
    internal static void UdpAuth(string token)
    {
        var listener = new EventBasedNetListener();
        var client = new NetManager(listener);

        UdpAuthResult authResult = UdpAuthResult.None;
        VersionResult versionResult = VersionResult.None;
        bool authAcked = false;
        bool versionChecked = false;
        bool disconnected = false;

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
                return;
            }

            Console.WriteLine(
                $"    Unexpected type 0x{typeId:X8}; dropping.");
        };

        client.Start();
        client.Connect(SentinelHost, SentinelPort, token);

        var deadline = DateTime.UtcNow + UdpAuthTimeout;

        while (!versionChecked && !disconnected && DateTime.UtcNow < deadline)
        {
            client.PollEvents();
            Thread.Sleep(15);
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

        if (versionResult == VersionResult.Ok)
            Console.WriteLine("    OK    [4] Protocol version accepted.");
        else
            Console.WriteLine(
                $"    FAIL  [4] Version result was {versionResult}.");
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