/*
 * (CreateLeg.cs)
 *------------------------------------------------------------
 * Created - 6/29/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;
using Networking.Tcp;
using Shared.Networking;
using Shared.Networking.Packets.Auth;
using Shared.Networking.Packets.Character;
using Shared.Networking.Packets.LifeCycle;
using System.Net.Security;
using System.Net.Sockets;
using System.Security.Authentication;

namespace Probe;

/// <summary>
/// Leg 5 of the probe: character creation over the wire. Stands up its own
/// held-open TLS connection, re-authenticates it with the seed the earlier legs
/// minted, and - on the NeedsCharacter outcome that holds the connection open -
/// sends a 0x02 create request on the same stream and reports whether the
/// server created the character.
/// </summary>
/// <remarks>This is the one leg that cannot use
/// <see cref="ProbeTransport.SendAndReceiveAsync"/>: that helper is one
/// connection per call, but create is two exchanges on a single stream - auth,
/// then create - because the server resolves the owning account from the
/// connection the NeedsCharacter outcome registered, never from the packet. The
/// connection is therefore stood up here and its framing carried locally rather
/// than touching the verified one-shot path the four passing legs depend on.
/// The leg is entered only after an earlier auth leg returned NeedsCharacter, so
/// a clean Created is the only pass; any other outcome stops the run rather than
/// looping an interactive retry.</remarks>
internal static class CreateLeg
{
    // Auths a fresh held-open connection, confirms NeedsCharacter, then sends
    // the create request on the same stream and reports the outcome. Returns
    // true only on Created, signalling Main to re-auth for its token.
    internal static async Task<bool> CreateCharacterAsync(
        string accountId, byte[] seed, string name)
    {
        using var tcp = new TcpClient();
        await tcp.ConnectAsync(
            ProbeTransport.ServerHost, ProbeTransport.ServerPort)
            .ConfigureAwait(false);

        using var ssl = new SslStream(
            tcp.GetStream(),
            leaveInnerStreamOpen: false,
            userCertificateValidationCallback:
                ProbeTransport.AcceptAnyServerCertificate);

        var sslOptions = new SslClientAuthenticationOptions
        {
            TargetHost = ProbeTransport.TargetHost,
            EnabledSslProtocols = SslProtocols.Tls12 | SslProtocols.Tls13,
        };

        await ssl.AuthenticateAsClientAsync(sslOptions).ConfigureAwait(false);

        // AuthMessage on the held connection. A character-less account answers
        // NeedsCharacter and the server registers this connection for the
        // create that follows - so, unlike the one-shot legs, the stream stays
        // open instead of closing after the response.
        AuthByKeyPacket authPacket =
            AuthLegs.BuildKeyAuthPacket(accountId, seed);
        await SendFramedAsync(ssl, authPacket).ConfigureAwait(false);

        var (authTypeId, authPayload) =
            await ReadFramedAsync(ssl).ConfigureAwait(false);

        if (!ExpectNeedsCharacter(authTypeId, authPayload))
            return false;

        Console.WriteLine(
            $"    NeedsCharacter confirmed; creating \"{name}\".");

        // Same stream: the server already bound this connection to the account
        // when it answered NeedsCharacter, so the request names only the
        // character - the account is never carried on the wire.
        var createPacket = new CharacterCreateRequestPacket(name);
        await SendFramedAsync(ssl, createPacket).ConfigureAwait(false);

        var (createTypeId, createPayload) =
            await ReadFramedAsync(ssl).ConfigureAwait(false);

        return ReportCreate(createTypeId, createPayload);
    }

    // Confirms the held connection's auth answered NeedsCharacter - the only
    // outcome that leaves the stream open for a create. Ok would mean the
    // account already owns a character (a race), and the server would have
    // sent-then-disconnected; any other value is the loud sentinel.
    private static bool ExpectNeedsCharacter(uint typeId, byte[] payload)
    {
        var reader = new NetDataReader();
        reader.SetSource(payload, 0, payload.Length);

        if (typeId == DisconnectPacket.TypeId)
        {
            DisconnectPacket disconnect = DisconnectPacket.Deserialize(reader);
            Console.WriteLine(
                "    FAIL  [5] Auth on create connection rejected: "
                + $"{disconnect.Reason}.");
            return false;
        }

        if (typeId != AuthResponsePacket.TypeId)
        {
            Console.WriteLine(
                $"    FAIL  [5] Unexpected auth response type 0x{typeId:X8}.");
            return false;
        }

        AuthResponsePacket response = AuthResponsePacket.Deserialize(reader);

        if (response.Outcome == AuthOutcome.NeedsCharacter)
            return true;

        Console.WriteLine(
            "    FAIL  [5] Expected NeedsCharacter on create connection; "
            + $"got {response.Outcome}.");
        return false;
    }

    // Reports the create outcome. Created is the only pass; every other outcome
    // - a name rejection, an account-state anomaly, or a persist fault - stops
    // the run. A DisconnectPacket as the first frame is defensive only: the
    // handler always sends a response before any disconnect, so seeing one here
    // means the connection dropped unexpectedly.
    private static bool ReportCreate(uint typeId, byte[] payload)
    {
        var reader = new NetDataReader();
        reader.SetSource(payload, 0, payload.Length);

        if (typeId == DisconnectPacket.TypeId)
        {
            DisconnectPacket disconnect = DisconnectPacket.Deserialize(reader);
            Console.WriteLine(
                "    FAIL  [5] Create connection closed before a response: "
                + $"{disconnect.Reason}.");
            return false;
        }

        if (typeId != CharacterCreateResponsePacket.TypeId)
        {
            Console.WriteLine(
                $"    FAIL  [5] Unexpected create response type 0x{typeId:X8}.");
            return false;
        }

        CharacterCreateResponsePacket response =
            CharacterCreateResponsePacket.Deserialize(reader);

        if (response.Outcome == CharacterCreateOutcome.Created)
        {
            Console.WriteLine("    OK    [5] Character created.");
            return true;
        }

        Console.WriteLine(
            $"    FAIL  [5] Create outcome was {response.Outcome}.");
        return false;
    }

    // Self-contained frame write over the held stream - the send half of
    // ProbeTransport.SendAndReceiveAsync, lifted so the verified one-shot path
    // stays untouched. Writes [length][type][payload] via PacketFramer.
    private static async Task SendFramedAsync<T>(SslStream ssl, T packet)
        where T : struct, IPacketWritable
    {
        var writer = new NetDataWriter();
        packet.Serialize(writer);

        int payloadLength = writer.Length;
        int frameSize = PacketFramer.FrameSize(payloadLength);
        byte[] frame = new byte[frameSize];

        PacketFramer.WriteFrame(
            packet.TypeId, writer.Data.AsSpan(0, payloadLength), frame);

        await ssl.WriteAsync(frame.AsMemory(0, frameSize)).ConfigureAwait(false);
        await ssl.FlushAsync().ConfigureAwait(false);
    }

    // Self-contained frame read over the held stream - the receive half of
    // ProbeTransport.SendAndReceiveAsync. Reads the 8-byte header, then exactly
    // the declared payload length.
    private static async Task<(uint typeId, byte[] payload)> ReadFramedAsync(
        SslStream ssl)
    {
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
}

/*
 *------------------------------------------------------------
 * (CreateLeg.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */