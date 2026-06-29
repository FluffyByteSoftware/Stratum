/*
 * (AuthLegs.cs)
 *------------------------------------------------------------
 * Created - 6/29/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;
using Shared.Networking;
using Shared.Networking.Packets.Auth;
using Shared.Networking.Packets.LifeCycle;
using System.Buffers.Binary;
using SystemTools.Security;

namespace Probe;

/// <summary>
/// Legs 1 and 2 of the probe: password authentication, then key authentication
/// with the seed the password leg minted, plus the helpers that report each
/// leg's login→world outcome.
/// </summary>
/// <remarks>Both legs run over <see cref="ProbeTransport"/>'s one-shot framed
/// send/receive - each is its own TLS connection, because the server closes
/// after every auth attempt. The two legs deliberately mirror each other's
/// outcome for the same account: the key minted in leg 1 must authenticate on
/// its own in leg 2, and the server re-applies the same login→world decision,
/// so a character-less account answers NeedsCharacter on both.</remarks>
internal static class AuthLegs
{
    // Reports an auth leg's login→world outcome and returns whether the probe
    // may continue. None is the loud-failure sentinel: the server never writes
    // it, so reading it back means a corrupt or uninitialized packet. Ok and
    // NeedsCharacter are both valid continuations - the caller decides what the
    // absence of a token means for the legs that follow.
    internal static bool ReportAuthLeg(int leg, AuthResponsePacket response)
    {
        switch (response.Outcome)
        {
            case AuthOutcome.Ok:
                Console.WriteLine(
                    $"    OK    [{leg}] Ok  token={Truncate(response.SessionToken)}"
                    + $"  udp={response.UdpEndpoint}");
                return true;

            case AuthOutcome.NeedsCharacter:
                Console.WriteLine(
                    $"    OK    [{leg}] NeedsCharacter (no token; create "
                    + "required).");
                return true;

            default:
                Console.WriteLine(
                    $"    FAIL  [{leg}] Outcome was {response.Outcome}; "
                    + "expected Ok or NeedsCharacter.");
                return false;
        }
    }

    internal static async Task<AuthResponsePacket?> PasswordAuthAsync(
        string accountId, string password)
    {
        var packet = new AuthByPasswordPacket(accountId, password);

        var (typeId, payload) =
            await ProbeTransport.SendAndReceiveAsync(packet)
                .ConfigureAwait(false);

        return InterpretResponse(typeId, payload);
    }

    internal static async Task<AuthResponsePacket?> KeyAuthAsync(
            string accountId, byte[] seed)
    {
        AuthByKeyPacket packet = BuildKeyAuthPacket(accountId, seed);

        var (typeId, payload) =
            await ProbeTransport.SendAndReceiveAsync(packet)
                .ConfigureAwait(false);

        return InterpretResponse(typeId, payload);
    }

    // Builds a signed key-auth packet for an account from its seed: stamps the
    // current time, signs it, and packs both. Shared by KeyAuthAsync and the
    // held-open create leg, which must re-auth its own connection with the same
    // seed - so the timestamp/sign/construct sequence lives in exactly one place
    // and the two callers cannot drift in how they build the packet. The same
    // timestamp is both signed and sent: the server re-derives the signed bytes
    // from the packet's UnixTimestampMs field.
    internal static AuthByKeyPacket BuildKeyAuthPacket(
        string accountId, byte[] seed)
    {
        long now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        byte[] signature = SignTimestamp(seed, now);

        return new AuthByKeyPacket(accountId, now, signature);
    }

    // Sync helper: stackalloc is illegal in an async method, and the signed
    // message is the 8-byte big-endian timestamp the server verifies against.
    private static byte[] SignTimestamp(byte[] seed, long unixTimestampMs)
    {
        Span<byte> message = stackalloc byte[sizeof(long)];
        BinaryPrimitives.WriteInt64BigEndian(message, unixTimestampMs);

        return Ed25519MessageSigner.Sign(seed, message);
    }

    // Returns the parsed response on success, or null after reporting either a
    // rejection (DisconnectPacket) or an unexpected packet type. The response's
    // leading AuthOutcome byte is consumed by AuthResponsePacket.Deserialize;
    // the caller inspects Outcome to decide how to proceed.
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

    private static string Truncate(string value) =>
        value.Length <= 12 ? value : value[..12] + "...";
}

/*
 *------------------------------------------------------------
 * (AuthLegs.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */