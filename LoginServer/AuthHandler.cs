/*
 * (AuthHandler.cs)
 *------------------------------------------------------------
 * Created - 5/31/2026 6:39:39 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Shared.Networking.Packets.Auth;
using Networking.Tcp;
using SystemTools.Accounts;
using SystemTools.Logger;
using SystemTools.Security;
using System;
using System.Buffers.Binary;
using System.Threading.Tasks;
using Stratum.Shared.Networking;

namespace LoginServer;

/// <summary>
/// Handles client authentication using cryptographic keys or password credentials, 
/// applies the login→world decision, issues session tokens, and enforces security 
/// policies including key expiration and account lockouts.
/// </summary>
/// <remarks>
/// On either successful auth path the handler does not unconditionally mint a token.
/// It first reads the account's <c>CharacterName</c> back-reference — the single
/// signal of whether the player already owns a playable character — and only issues a
/// token when one exists. An account with no character is answered with
/// <see cref="AuthOutcome.NeedsCharacter"/> and no token, so a token always implies a
/// playable character downstream at Sentinel. This read happens here, in LoginServer,
/// because LoginServer holds the account after TLS auth and before the token is minted,
/// and because Sentinel is pure transport with no account knowledge.
/// </remarks>
/// <param name="accounts">The account store for retrieving and updating account 
/// records.</param>
/// <param name="lockout">The lockout tracker for recording authentication failures 
/// and managing lockout status.</param>
/// <param name="udpEndpoint">The UDP endpoint provided to authenticated clients 
/// for subsequent communication.</param>
public sealed class AuthHandler(
    AccountStore accounts,
    LockoutTracker lockout,
    string udpEndpoint)
{
    private const long MaxClockSkewMs = 30_000;
    private const int KeyLifetimeDays = 3;

    /// <summary>
    /// The host to which authenticated clients should be directed for subsequent 
    /// communication after successful authentication.
    /// </summary>
    public TcpHost? Host { get; set; }

    /// <summary>
    /// Handles cryptographic key-based authentication for the given connection.
    /// </summary>
    /// <param name="conn">The TCP connection requesting authentication.</param>
    /// <param name="packet">The authentication packet containing account ID, 
    /// timestamp, and cryptographic signature.</param>
    /// <returns>A task representing the asynchronous authentication 
    /// operation.</returns>
    public async ValueTask OnAuthByKey(TcpConnection conn, AuthByKeyPacket packet)
    {
        if (!accounts.TryGet(packet.AccountId, out var record))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Key auth for unknown account '{packet.AccountId}' "
                + $"from {conn.RemoteEndPoint}."));

            conn.RequestDisconnect(SecureDisconnectReason.InvalidUserCredentials);
            return;
        }

        long now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        if (Math.Abs(now - packet.UnixTimestampMs) > MaxClockSkewMs)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Key auth for '{record.Id}' rejected: stale timestamp "
                + $"from {conn.RemoteEndPoint}."));
            conn.RequestDisconnect(SecureDisconnectReason.InvalidUserCredentials);
            return;
        }

        byte[] publicKey;

        try
        {
            publicKey = Convert.FromBase64String(record.PublicKey);
        }
        catch (FormatException ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                $"Account '{record.Id}' has a malformed public key.", ex));
            conn.RequestDisconnect(SecureDisconnectReason.InternalError);
            return;
        }

        if (!VerifySignature(publicKey, packet.UnixTimestampMs, packet.Signature))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Key auth for '{record.Id}' rejected: bad signature "
                + $"from {conn.RemoteEndPoint}."));
            conn.RequestDisconnect(SecureDisconnectReason.InvalidUserCredentials);
            return;
        }
        var issuedUtc = DateTime.SpecifyKind(record.TimeLastKeyIssued, DateTimeKind.Utc);
        if (DateTime.UtcNow - issuedUtc > TimeSpan.FromDays(KeyLifetimeDays))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Key auth for '{record.Id}' rejected: key expired "
                + $"from {conn.RemoteEndPoint}."));
            conn.RequestDisconnect(SecureDisconnectReason.KeyExpired);
            return;
        }

        await BuildAuthOutcomeAsync(conn, record, "").ConfigureAwait(false);
    }

    /// <summary>
    /// Authenticates a client connection using password-based credentials.
    /// </summary>
    /// <remarks>Generates a new Ed25519 key pair and issues a session token on 
    /// successful authentication when the account owns a character. Records 
    /// authentication failures and enforces account lockout after repeated failures. 
    /// Disconnects the connection on authentication failure or if the account is 
    /// locked. The rekey carries the account's existing character back-reference 
    /// forward via <see cref="AccountRecord.WithReissuedKey"/>; building a fresh 
    /// record by hand here would drop <c>CharacterName</c> and sever the link on 
    /// every password login, which — because keys hard-expire after three days — is 
    /// a routine path rather than a rare one.</remarks>
    /// <param name="conn">The TCP connection to authenticate.</param>
    /// <param name="packet">The authentication request containing account ID 
    /// and password.</param>
    /// <returns>A task that represents the asynchronous authentication 
    /// operation.</returns>
    public async ValueTask OnAuthByPassword(
        TcpConnection conn, AuthByPasswordPacket packet)
    {
        if (lockout.IsLocked(packet.AccountId))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Password auth for '{packet.AccountId}' rejected: locked out "
                + $"({conn.RemoteEndPoint})."));
            conn.RequestDisconnect(SecureDisconnectReason.AccountLocked);
            return;
        }

        if (!accounts.TryGet(packet.AccountId, out var record))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Password auth for unknown account '{packet.AccountId}' "
                + $"from {conn.RemoteEndPoint}."));
            conn.RequestDisconnect(SecureDisconnectReason.InvalidUserCredentials);
            return;
        }

        if (!PasswordHasher.Verify(packet.Password, record.PasswordHash))
        {
            bool nowLocked = lockout.RecordFailure(record.Id);

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Password auth for '{record.Id}' failed " +
                $"from {conn.RemoteEndPoint}. " +
                $"Lockout status: {nowLocked}."));

            if (nowLocked)
            {
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                    $"Account '{record.Id}' locked after repeated failures "
                    + $"from {conn.RemoteEndPoint}."));
            }
            conn.RequestDisconnect(
                SecureDisconnectReason.InvalidUserCredentials);
            return;
        }

        lockout.Clear(record.Id);

        var keyPair = Ed25519KeyGenerator.Generate();

        var rekeyed = record.WithReissuedKey(
            Convert.ToBase64String(keyPair.PublicKey),
            DateTime.UtcNow);

        accounts.Update(rekeyed);

        var issuedPrivate = Convert.ToBase64String(keyPair.PrivateSeed);

        await BuildAuthOutcomeAsync(conn, rekeyed, issuedPrivate)
            .ConfigureAwait(false);
    }

    /// <summary>
    /// Applies the login→world decision for an authenticated account and sends the
    /// resulting <see cref="AuthResponsePacket"/>.
    /// </summary>
    /// <param name="conn">The authenticated TCP connection to respond on.</param>
    /// <param name="record">The authenticated account, post-rekey on the password
    /// path, whose <see cref="AccountRecord.CharacterName"/> drives the branch.</param>
    /// <param name="issuedPrivateKey">The freshly minted private seed to return, or an
    /// empty string on the key path where no new key was issued.</param>
    /// <returns>A task representing the asynchronous send operation.</returns>
    /// <remarks>
    /// This is the single shared tail of both auth paths, factored here so the
    /// login→world decision is made in exactly one place rather than duplicated at two
    /// success sites. An empty <see cref="AccountRecord.CharacterName"/> yields
    /// <see cref="AuthOutcome.NeedsCharacter"/> with no token and no endpoint — a token
    /// is deliberately not minted, so possession of a token always implies a playable
    /// character once the session reaches Sentinel. The issued private seed still rides
    /// on the <see cref="AuthOutcome.NeedsCharacter"/> response so a brand-new password
    /// account keeps the key it was just granted and can authenticate by key on the
    /// subsequent character-creation round-trip. A populated name yields
    /// <see cref="AuthOutcome.Ok"/> with a freshly issued token and the UDP endpoint.
    /// The send-then-disconnect behaviour of <see cref="SendResponseAsync"/> is
    /// unchanged for both outcomes; the character-creation round-trip that follows a
    /// <see cref="AuthOutcome.NeedsCharacter"/> response is a separate, player-driven
    /// exchange on the character channel, not this handler's concern.
    /// </remarks>
    private async ValueTask BuildAuthOutcomeAsync(
        TcpConnection conn, AccountRecord record, string issuedPrivateKey)
    {
        AuthResponsePacket response;

        if (string.IsNullOrEmpty(record.CharacterName))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Account '{record.Id}' authenticated with no character; "
                + $"signalling create from {conn.RemoteEndPoint}."));

            response = new AuthResponsePacket(
                AuthOutcome.NeedsCharacter, "", "", issuedPrivateKey);
        }
        else
        {
            var token = SessionTokenIssuer.Issue(record.Id);

            response = new AuthResponsePacket(
                AuthOutcome.Ok, token, udpEndpoint, issuedPrivateKey);
        }

        await SendResponseAsync(conn, response).ConfigureAwait(false);
    }

    private async ValueTask SendResponseAsync(
        TcpConnection conn, AuthResponsePacket response)
    {
        var host = Host ??
            throw new InvalidOperationException("Host not assigned.");

        await host.SendAsync(conn, response).ConfigureAwait(false);
        conn.RequestDisconnect(SecureDisconnectReason.None);
    }

    private static bool VerifySignature(
        byte[] publicKey, long timeStampMs, byte[] signature)
    {
        Span<byte> message = stackalloc byte[sizeof(long)];

        BinaryPrimitives.WriteInt64BigEndian(message, timeStampMs);

        return Ed25519Verifier.Verify(publicKey, message, signature);
    }
}


/*
 *------------------------------------------------------------
 * (AuthHandler.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */