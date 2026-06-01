/*
 * (SessionTokenIssuer.cs)
 *------------------------------------------------------------
 * Created - 5/21/2026 12:43:20 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;

namespace SystemTools.Security;

/// <summary>
/// Provides functionality for issuing and validating cryptographically signed, time-limited session tokens using
/// HMAC-SHA256.    
/// </summary>
/// <remarks>Must be initialized with a signing key before issuing or validating tokens. Issued tokens are
/// Base64URL-encoded and have a fixed lifetime of 30 seconds.</remarks>
public static class SessionTokenIssuer
{
    private const int LifetimeSeconds = 30;
    private const int HmacTagLength = 32;
    private const int TimestampLength = 8;
    private const int AccountIdLengthPrefixLength = 1;
    private const int MaxAccountIdBytes = 255;

    private static readonly byte[] _signingKey = new byte[32];
    private static bool _initialized;

    /// <summary>
    /// Is the SessionTokenIssuer initialized and ready?
    /// </summary>
    public static bool IsInitialized => _initialized;

    /// <summary>
    /// Initializes the signing key for cryptographic operations. Can only be called once; subsequent calls are ignored.
    /// </summary>
    /// <param name="signingKey">The signing key bytes to use for cryptographic operations.</param>
    /// <exception cref="ArgumentException">Thrown when <paramref name="signingKey"/> length does not match the required key size.</exception>
    public static void Initialize(ReadOnlySpan<byte> signingKey)
    {
        if (_initialized) return;

        if (signingKey.Length != _signingKey.Length)
            throw new ArgumentException(
                $"Signing key must be {_signingKey.Length} bytes.", nameof(signingKey));

        signingKey.CopyTo(_signingKey);
        _initialized = true;
    }

    /// <summary>
    /// Issues a signed session token for the specified account identifier.
    /// </summary>
    /// <param name="accountId">The account identifier to include in the token.</param>
    /// <returns>A Base64URL-encoded session token containing the account identifier, issued and expiration timestamps, and
    /// HMAC-SHA256 signature.</returns>
    /// <exception cref="InvalidOperationException">The SessionTokenIssuer has not been initialized.</exception>
    /// <exception cref="ArgumentException"><paramref name="accountId"/> is null, empty, or exceeds the maximum allowed UTF-8 byte length.</exception>
    public static string Issue(string accountId)
    {
        if (!_initialized)
            throw new InvalidOperationException(
                $"SessionTokenIssuer not initialized.");
        ArgumentException.ThrowIfNullOrEmpty(accountId);

        var idBytes = Encoding.UTF8.GetBytes(accountId);
        if (idBytes.Length > MaxAccountIdBytes)
            throw new ArgumentException(
                $"Account id exceeds {MaxAccountIdBytes} UTF-8 bytes.",
                nameof(accountId));

        var nowMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        var expiresMs = nowMs + LifetimeSeconds * 1000L;

        var payloadLength =
            AccountIdLengthPrefixLength
            + idBytes.Length
            + TimestampLength
            + TimestampLength;

        var totalLength = payloadLength + HmacTagLength;

        var buffer = new byte[totalLength];
        var span = buffer.AsSpan();

        span[0] = (byte)idBytes.Length;
        idBytes.CopyTo(span[AccountIdLengthPrefixLength..]);

        var cursor = AccountIdLengthPrefixLength + idBytes.Length;

        BinaryPrimitives.WriteInt64BigEndian(
            span.Slice(cursor, TimestampLength),
            nowMs);

        cursor += TimestampLength;
        
        BinaryPrimitives.WriteInt64BigEndian(
            span.Slice(cursor, TimestampLength), expiresMs);

        cursor += TimestampLength;

        var payload = span[..payloadLength];
        var tagDest = span.Slice(cursor, HmacTagLength);

        HMACSHA256.HashData(_signingKey, payload, tagDest);

        return Base64UrlEncode(buffer);
    }

    /// <summary>
    /// Validates a token and extracts the account ID and expiration timestamp if valid.
    /// </summary>
    /// <param name="token">The token string to validate.</param>
    /// <param name="accountId">When this method returns, contains the account ID if the token is valid; otherwise, an empty string.</param>
    /// <param name="expiresAtMs">When this method returns, contains the expiration timestamp in milliseconds since Unix epoch if the token is
    /// valid; otherwise, zero.</param>
    /// <returns><c>true</c> if the token is valid and not expired; otherwise, <c>false</c>.</returns>
    public static bool TryValidate(
        string token,
        out string accountId,
        out long expiresAtMs)
    {
        accountId = string.Empty;
        expiresAtMs = 0;

        if (!_initialized) return false;
        if (string.IsNullOrEmpty(token)) return false;
        if (!TryBase64UrlDecode(token, out var bytes)) return false;
        
        if (bytes is null) return false;

        var span = bytes.AsSpan();
        
        if (span.Length < AccountIdLengthPrefixLength + HmacTagLength)
            return false;

        int idLen = span[0];
        var expectedLength =
            AccountIdLengthPrefixLength
            + idLen
            + TimestampLength
            + TimestampLength
            + HmacTagLength;

        if (span.Length != expectedLength) return false;

        var payloadLength = expectedLength - HmacTagLength;
        var payload = span[..payloadLength];
        var providedTag = span.Slice(payloadLength, HmacTagLength);

        Span<byte> computedTag = stackalloc byte[HmacTagLength];
        
        HMACSHA256.HashData(_signingKey, payload, computedTag);

        if (!CryptographicOperations.FixedTimeEquals(computedTag, providedTag))
            return false;

        var cursor = AccountIdLengthPrefixLength + idLen;
        var issuedMs = BinaryPrimitives.ReadInt64BigEndian(
            span.Slice(cursor, TimestampLength));
        cursor += TimestampLength;

        var parsedExpiresMs = BinaryPrimitives.ReadInt64BigEndian(
            span.Slice(cursor, TimestampLength));

        var nowMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();

        if (nowMs >= parsedExpiresMs) return false;
        if (issuedMs > nowMs) return false;

        accountId = Encoding.UTF8.GetString(
            span.Slice(AccountIdLengthPrefixLength, idLen));

        expiresAtMs = parsedExpiresMs;
        
        return true;
    }

    private static string Base64UrlEncode(ReadOnlySpan<byte> bytes)
    {
        var b64 = Convert.ToBase64String(bytes);

        return b64.Replace('+', '-').Replace('/', '_').TrimEnd('=');
    }

    private static bool TryBase64UrlDecode(string text, out byte[]? bytes)
    {
        bytes = null;
        var standard = text.Replace('-', '+').Replace('_', '/');
        var padding = (4 - standard.Length % 4) % 4;
        if (padding != 0) standard += new string('=', padding);

        try
        {
            bytes = Convert.FromBase64String(standard);
            return true;
        }
        catch (FormatException)
        {
            return false;
        }
    }
}

/*
 *------------------------------------------------------------
 * (SessionTokenIssuer.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */