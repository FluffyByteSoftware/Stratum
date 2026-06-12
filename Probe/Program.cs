/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 611/2026 1:04:18 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

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

            await KeyAuthAsync(accountId, seed).ConfigureAwait(false);
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

    private static async Task KeyAuthAsync(string accountId, byte[] seed)
    {
        long now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        byte[] signature = SignTimestamp(seed, now);

        // The same timestamp is both signed and sent: the server re-derives
        // the signed bytes from the packet's UnixTimestampMs field.
        var packet = new AuthByKeyPacket(accountId, now, signature);

        var (typeId, payload) =
            await SendAndReceiveAsync(packet).ConfigureAwait(false);

        if (InterpretResponse(typeId, payload) is { } response)
            Console.WriteLine(
                $"    OK  token={Truncate(response.SessionToken)}  "
                + $"udp={response.UdpEndpoint}");
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