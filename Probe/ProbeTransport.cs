/*
 * (ProbeTransport.cs)
 *------------------------------------------------------------
 * Created - 6/29/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;
using Networking.Tcp;
using Shared.Networking;
using System.Net.Security;
using System.Net.Sockets;
using System.Security.Authentication;
using System.Security.Cryptography.X509Certificates;

namespace Probe;

/// <summary>
/// Owns the Probe's TLS/TCP transport: the connection endpoint, the one-shot
/// framed send/receive the auth legs share, and the dev-only certificate trust
/// callback.
/// </summary>
/// <remarks>Extracted from the original single-file Program so every auth leg
/// shares one verified framing path. The send/receive helper is deliberately
/// one connection per call - it opens TLS, writes one frame, reads one frame,
/// and closes - because the server closes after every auth attempt. A round
/// trip that must hold a connection open across two exchanges (character
/// create) does not belong here and stands up its own connection.</remarks>
internal static class ProbeTransport
{
    // Internal, not private: Main prints the endpoint at startup and the auth
    // legs frame against it. MUST match the LoginServer's network.json.
    internal const string ServerHost = "10.0.0.84";

    // MUST match the "Port" in the LoginServer's network.json.
    internal const int ServerPort = 9997;

    // SNI target. With accept-all validation the value only drives the SNI
    // field and never has to match the dev cert's CN. Internal so the held-open
    // create leg, which stands up its own TLS connection, reuses one SNI value.
    internal const string TargetHost = "Stratum";

    // Opens a fresh TLS connection, performs the client handshake, sends one
    // framed packet, and reads exactly one framed response. The server closes
    // after every auth attempt, so each leg is its own connection.
    internal static async Task<(uint typeId, byte[] payload)>
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

    // Loopback probe against a self-signed dev certificate: trust is
    // unconditional here. The real client must validate or pin instead.
    // Internal so the create leg's own TLS connection shares one trust policy.
    internal static bool AcceptAnyServerCertificate(
        object sender,
        X509Certificate? certificate,
        X509Chain? chain,
        SslPolicyErrors errors) => true;
}

/*
 *------------------------------------------------------------
 * (ProbeTransport.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */