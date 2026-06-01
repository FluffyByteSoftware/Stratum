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
using Stratum.Networking.Tcp;
using Stratum.LoginServer;
using Stratum.Shared.Networking;

namespace LoginServer;

/// <summary>
/// Handles client authentication using cryptographic keys or password credentials, 
/// issues session tokens, and enforces security policies including key expiration 
/// and account lockouts.
/// </summary>
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
        if(!accounts.TryGet(packet.AccountId, out var record))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Key auth for unknown account '{packet.AccountId}' "
                + $"from {conn.RemoteEndPoint}."));

            conn.RequestDisconnect(SecureDisconnectReason.InvalidUserCredentials);
            return;
        }

        long now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        if(Math.Abs(now - packet.UnixTimestampMs) > MaxClockSkewMs)
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
        catch(FormatException ex)
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
        var issuedUtc = DateTime.SpecifyKind(record.IssuedAt, DateTimeKind.Utc);
        if(DateTime.UtcNow - issuedUtc > TimeSpan.FromDays(KeyLifetimeDays))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Key auth for '{record.Id}' rejected: key expired "
                + $"from {conn.RemoteEndPoint}."));
            conn.RequestDisconnect(SecureDisconnectReason.KeyExpired);
            return;
        }

        var token = SessionTokenIssuer.Issue(record.Id);
        await SendResponseAsync(conn, new AuthResponsePacket(token, udpEndpoint, ""))
            .ConfigureAwait(false);
    }

    /// <summary>
    /// Authenticates a client connection using password-based credentials.
    /// </summary>
    /// <remarks>Generates a new Ed25519 key pair and issues a session token on 
    /// successful authentication. Records authentication failures and enforces 
    /// account lockout after repeated failures. Disconnects the connection
    /// on authentication failure or if the account is locked.</remarks>
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

        if(!accounts.TryGet(packet.AccountId, out var record))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn, 
                $"Password auth for unknown account '{packet.AccountId}' "
                + $"from {conn.RemoteEndPoint}."));
            conn.RequestDisconnect(SecureDisconnectReason.InvalidUserCredentials);
            return;
        }

        if(!PasswordHasher.Verify(packet.Password, record.PasswordHash))
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

        var rekeyed = new AccountRecord
        {
            Id = record.Id,
            PublicKey = Convert.ToBase64String(keyPair.PublicKey),
            PasswordHash = record.PasswordHash,
            IssuedAt = DateTime.UtcNow,
            CreatedAt = record.CreatedAt
        };

        accounts.Update(rekeyed);

        var token = SessionTokenIssuer.Issue(record.Id);
        var issuedPrivate = Convert.ToBase64String(keyPair.PrivateSeed);

        await SendResponseAsync(
            conn, new AuthResponsePacket(token, udpEndpoint, issuedPrivate))
            .ConfigureAwait(false);
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