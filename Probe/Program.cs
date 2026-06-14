/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 611/2026 1:04:18 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib;
using LiteNetLib.Utils;
using Networking.Tcp;
using Shared.Networking;
using Shared.Networking.Packets.Auth;
using Shared.Networking.Packets.LifeCycle;
using System.Buffers.Binary;
using System.Net.Security;
using System.Net.Sockets;
using System.Security.Authentication;
using System.Security.Cryptography.X509Certificates;
using System.Threading;
using SystemTools.Security;

namespace Probe;

internal static class Program
{
    private const string ServerHost = "10.0.0.84";

    // MUST match the "Port" in the LoginServer's network.json.
    private const int ServerPort = 9997;

    // SNI target. With accept-all validation the value only drives the SNI
    // field and never has to match the dev cert's CN.
    private const string TargetHost = "Stratum";

    // Sentinel's UDP endpoint for testing. Host matches the TCP target; port
    // is Sentinel's listener. The advertised UdpEndpoint from the auth
    // response carries the same value once its placeholder host is real.
    private const string SentinelHost = "10.0.0.84";
    private const int SentinelPort = 9998;

    private static readonly TimeSpan UdpAuthTimeout = TimeSpan.FromSeconds(3);

    private static async Task Main()
    {
        Console.WriteLine("Stratum auth probe");
        Console.WriteLine($"Target: {ServerHost}:{ServerPort}");
        Console.WriteLine();

        Console.Write("Account id [testuser]: ");
        string accountId = ReadLineOrDefault("testuser");

        // Plain-text entry: this is a local test tool, not the real client.
        Console.Write("Password: ");
        string password = ReadLineOrDefault(string.Empty);

        try
        {
            // Leg 1 - password auth. On success the server mints a fresh
            // keypair, rewrites the account, and returns the private seed.
            Console.WriteLine();
            Console.WriteLine("[1] Password auth ...");

            if (await PasswordAuthAsync(accountId, password)
                    .ConfigureAwait(false) is not { } response)
                return;

            Console.WriteLine(
                $"    OK  token={Truncate(response.SessionToken)}  "
                + $"udp={response.UdpEndpoint}");

            if (string.IsNullOrEmpty(response.IssuedPrivateKey))
            {
                Console.WriteLine(
                    "    Server issued no private key; skipping key auth.");
                return;
            }

            byte[] seed = Convert.FromBase64String(response.IssuedPrivateKey);
            Console.WriteLine($"    Issued seed: {seed.Length} bytes.");

            // Leg 2 - key auth using the seed leg 1 just minted. This closes
            // the loop: the key issued above must now authenticate on its own.
            Console.WriteLine();
            Console.WriteLine("[2] Key auth with issued seed ...");

            if (await KeyAuthAsync(accountId, seed).ConfigureAwait(false)
                    is not { } keyResponse)
                return;

            // Leg 3 - UDP auth against Sentinel using the token leg 2 issued.
            // Proves the TCP-minted token validates over UDP and the ack
            // round-trips with correct big-endian framing.
            Console.WriteLine();
            Console.WriteLine("[3] UDP auth with leg-2 token ...");

            UdpAuth(keyResponse.SessionToken);
        }
        catch (Exception ex)
        {
            Console.WriteLine($"Probe failed: {ex.Message}");
        }
    }

    private static async Task<AuthResponsePacket?> PasswordAuthAsync(
        string accountId, string password)
    {
        var packet = new AuthByPasswordPacket(accountId, password);

        var (typeId, payload) =
            await SendAndReceiveAsync(packet).ConfigureAwait(false);

        return InterpretResponse(typeId, payload);
    }

    private static async Task<AuthResponsePacket?> KeyAuthAsync(
        string accountId, byte[] seed)
    {
        long now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        byte[] signature = SignTimestamp(seed, now);

        // The same timestamp is both signed and sent: the server re-derives
        // the signed bytes from the packet's UnixTimestampMs field.
        var packet = new AuthByKeyPacket(accountId, now, signature);

        var (typeId, payload) =
            await SendAndReceiveAsync(packet).ConfigureAwait(false);

        AuthResponsePacket? response = InterpretResponse(typeId, payload);

        if (response is { } ok)
            Console.WriteLine(
                $"    OK  token={Truncate(ok.SessionToken)}  "
                + $"udp={ok.UdpEndpoint}");

        return response;
    }

    // Leg 3: stand up a LiteNetLib client, present the session token as the
    // connection-request data, and wait for Sentinel's ack. A rejected token
    // never establishes a peer, so connection failure or timeout is the fail
    // path; only a received Authenticated ack passes.
    private static void UdpAuth(string token)
    {
        var listener = new EventBasedNetListener();
        var client = new NetManager(listener);

        UdpAuthResult result = UdpAuthResult.None;
        bool acked = false;
        bool disconnected = false;

        listener.PeerConnectedEvent += peer =>
            Console.WriteLine("    Connected; awaiting ack.");

        listener.PeerDisconnectedEvent += (peer, info) =>
        {
            disconnected = true;
            Console.WriteLine($"    Disconnected: {info.Reason}.");
        };

        listener.NetworkReceiveEvent += (peer, reader, channel, method) =>
        {
            byte[] data = reader.GetRemainingBytes();
            reader.Recycle();

            if (data.Length < sizeof(uint) + 1)
            {
                Console.WriteLine(
                    $"    Ack too short: {data.Length} bytes.");
                return;
            }

            uint typeId = BinaryPrimitives.ReadUInt32BigEndian(data);

            if (typeId != PacketIds.Auth.UdpAuthAck)
            {
                Console.WriteLine(
                    $"    Unexpected ack type 0x{typeId:X8}.");
                return;
            }

            result = (UdpAuthResult)data[sizeof(uint)];
            acked = true;
        };

        client.Start();
        client.Connect(SentinelHost, SentinelPort, token);

        var deadline = DateTime.UtcNow + UdpAuthTimeout;

        while (!acked && !disconnected && DateTime.UtcNow < deadline)
        {
            client.PollEvents();
            Thread.Sleep(15);
        }

        client.Stop();

        if (acked && result == UdpAuthResult.Authenticated)
            Console.WriteLine("    OK  UDP session authenticated.");
        else if (acked)
            Console.WriteLine($"    FAIL  ack result was {result}.");
        else
            Console.WriteLine("    FAIL  no ack (rejected or timed out).");
    }

    // Sync helper: stackalloc is illegal in an async method, and the signed
    // message is the 8-byte big-endian timestamp the server verifies against.
    private static byte[] SignTimestamp(byte[] seed, long unixTimestampMs)
    {
        Span<byte> message = stackalloc byte[sizeof(long)];
        BinaryPrimitives.WriteInt64BigEndian(message, unixTimestampMs);

        return Ed25519MessageSigner.Sign(seed, message);
    }

    // Opens a fresh TLS connection, performs the client handshake, sends one
    // framed packet, and reads exactly one framed response. The server closes
    // after every auth attempt, so each leg is its own connection.
    private static async Task<(uint typeId, byte[] payload)>
        SendAndReceiveAsync<T>(T packet)
        where T : struct, IPacketWritable
    {
        using var tcp = new TcpClient();
        await tcp.ConnectAsync(ServerHost, ServerPort).ConfigureAwait(false);

        using var ssl = new SslStream(
            tcp.GetStream(),
            leaveInnerStreamOpen: false,
            userCertificateValidationCallback: AcceptAnyServerCertificate);

        var sslOptions = new SslClientAuthenticationOptions
        {
            TargetHost = TargetHost,
            EnabledSslProtocols = SslProtocols.Tls12 | SslProtocols.Tls13,
        };

        await ssl.AuthenticateAsClientAsync(sslOptions).ConfigureAwait(false);

        // Frame and send the request, mirroring TcpHost.SendAsync.
        var writer = new NetDataWriter();
        packet.Serialize(writer);

        int payloadLength = writer.Length;
        int frameSize = PacketFramer.FrameSize(payloadLength);
        byte[] frame = new byte[frameSize];

        PacketFramer.WriteFrame(
            packet.TypeId, writer.Data.AsSpan(0, payloadLength), frame);

        await ssl.WriteAsync(frame.AsMemory(0, frameSize)).ConfigureAwait(false);
        await ssl.FlushAsync().ConfigureAwait(false);

        // Read the single response frame: 8-byte header, then payload.
        byte[] header = new byte[PacketFramer.HeaderSize];
        await ssl.ReadExactlyAsync(header.AsMemory()).ConfigureAwait(false);

        if (!PacketFramer.TryReadHeader(
                header, out uint typeId, out int responseLength))
            throw new InvalidOperationException(
                "Malformed response header from server.");

        byte[] payload = new byte[responseLength];

        if (responseLength > 0)
            await ssl.ReadExactlyAsync(payload.AsMemory())
                .ConfigureAwait(false);

        return (typeId, payload);
    }

    // Returns the parsed response on success, or null after reporting either a
    // rejection (DisconnectPacket) or an unexpected packet type.
    private static AuthResponsePacket? InterpretResponse(
        uint typeId, byte[] payload)
    {
        var reader = new NetDataReader();
        reader.SetSource(payload, 0, payload.Length);

        if (typeId == AuthResponsePacket.TypeId)
            return AuthResponsePacket.Deserialize(reader);

        if (typeId == DisconnectPacket.TypeId)
        {
            DisconnectPacket disconnect = DisconnectPacket.Deserialize(reader);
            Console.WriteLine($"    REJECTED: {disconnect.Reason}");
            return null;
        }

        Console.WriteLine($"    Unexpected response type 0x{typeId:X8}.");
        return null;
    }

    // Loopback probe against a self-signed dev certificate: trust is
    // unconditional here. The real client must validate or pin instead.
    private static bool AcceptAnyServerCertificate(
        object sender,
        X509Certificate? certificate,
        X509Chain? chain,
        SslPolicyErrors errors) => true;

    private static string ReadLineOrDefault(string fallback)
    {
        string? line = Console.ReadLine();
        return string.IsNullOrWhiteSpace(line) ? fallback : line.Trim();
    }

    private static string Truncate(string value) =>
        value.Length <= 12 ? value : value[..12] + "...";
}

/*
 *------------------------------------------------------------
 * (Program.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */