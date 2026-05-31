/*
 * (PasswordHasher.cs)
 *------------------------------------------------------------
 * Created - 5/21/2026 7:12:35 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Security.Cryptography;
using System.Text;
using Org.BouncyCastle.Crypto.Parameters;
using Org.BouncyCastle.Crypto.Generators;
using Stratum.SystemTools.Logger;

namespace Stratum.SystemTools.Security;

/// <summary>
/// Provides password hashing and verification using the Argon2id algorithm.
/// </summary>
/// <remarks>Hashes are formatted as PHC (Password Hashing Competition) strings. 
/// Default parameters are 65536 KB memory, 3 iterations, and 
/// parallelism of 1.</remarks>
public static class PasswordHasher
{
    private const int MemoryKb = 65536;
    private const int Iterations = 3;
    private const int Parallelism = 1;
    private const int SaltLength = 16;
    private const int HashLength = 32;
    private const int Version = 19;
    private const string AlgorithmId = "argon2id";

    /// <summary>
    /// Hashes a password using a randomly generated salt.
    /// </summary>
    /// <param name="password">The password to hash.</param>
    /// <returns>The hashed password in PHC string format.</returns>
    public static string Hash(string password)
    {
        ArgumentNullException.ThrowIfNull(password);

        Span<byte> salt = stackalloc byte[SaltLength];
        RandomNumberGenerator.Fill(salt);

        var hash = DeriveHash(password, salt);

        return FormatPhc(salt, hash);
    }

    /// <summary>
    /// Verifies a password against a PHC-formatted hash string.
    /// </summary>
    /// <param name="password">The password to verify.</param>
    /// <param name="phc">The PHC-formatted hash string containing the expected hash and parameters.</param>
    /// <returns>true if the password matches the hash; otherwise, false.</returns>
    public static bool Verify(string password, string phc)
    {
        if (password is null) return false;
        if (string.IsNullOrEmpty(phc)) return false;

        if (!TryParsePhc(
            phc,
            out int memoryKb,
            out int iterations,
            out int parallelism,
            out byte[]? salt,
            out byte[]? expected))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                "Stored password hash is malformed; verification rejected."));
            return false;
        }

        var actual = DeriveHash(
            password,
            salt,
            memoryKb,
            iterations,
            parallelism,
            expected?.Length ?? -1);

        return CryptographicOperations.FixedTimeEquals(actual, expected);
    }

    private static byte[] DeriveHash(string password, ReadOnlySpan<byte> salt)
        => DeriveHash(
            password,
            salt,
            MemoryKb,
            Iterations,
            Parallelism,
            HashLength);

    private static byte[] DeriveHash(
        string password,
        ReadOnlySpan<byte> salt,
        int memoryKb,
        int iterations,
        int parallelism,
        int outputLength)
    {
        var pwBytes = Encoding.UTF8.GetBytes(password);

        var parameters = new Argon2Parameters.Builder(Argon2Parameters.Argon2id)
            .WithVersion(Argon2Parameters.Version13)
            .WithSalt(salt.ToArray())
            .WithMemoryAsKB(memoryKb)
            .WithIterations(iterations)
            .WithParallelism(parallelism)
            .Build();

        var generator = new Argon2BytesGenerator();
        generator.Init(parameters);

        var output = new byte[outputLength];
        generator.GenerateBytes(pwBytes, output, 0, output.Length);

        CryptographicOperations.ZeroMemory(pwBytes);
        return output;
    }

    private static string FormatPhc(
        ReadOnlySpan<byte> salt,
        ReadOnlySpan<byte> hash)
    {
        var saltB64 = Base64NoPad(salt);
        var hashB64 = Base64NoPad(hash);

        return
            $"${AlgorithmId}$v={Version}$" +
            $"m={MemoryKb},t={Iterations},p={Parallelism}$" +
            $"{saltB64}${hashB64}";
    }

    private static bool TryParsePhc(
        string phc,
        out int memoryKb,
        out int iterations,
        out int parallelism,
        out byte[]? salt,
        out byte[]? hash)
    {
        memoryKb = 0;
        iterations = 0;
        parallelism = 0;
        salt = null;
        hash = null;

        var parts = phc.Split('$');
        // Expected layout: ["", "argon2id", "v=19",
        //                   "m=...,t=...,p=...", "<saltB64>", "<hashB64>"]
        if (parts.Length != 6) return false;
        if (parts[0].Length != 0) return false;
        if (parts[1] != AlgorithmId) return false;

        if (!TryParseKeyValue(parts[2], "v", out int version)) return false;
        if (version != Version) return false;

        var paramFields = parts[3].Split(',');
        if (paramFields.Length != 3) return false;

        if (!TryParseKeyValue(paramFields[0], "m", out memoryKb)) return false;
        if (!TryParseKeyValue(paramFields[1], "t", out iterations)) return false;
        if (!TryParseKeyValue(paramFields[2], "p", out parallelism))
            return false;

        if (memoryKb <= 0 || iterations <= 0 || parallelism <= 0)
            return false;

        if (!TryBase64NoPadDecode(parts[4], out salt)) return false;
        if (!TryBase64NoPadDecode(parts[5], out hash)) return false;

        if (salt == null || hash == null || salt.Length < 8
            || hash.Length < 4) return false;

        return true;
    }

    private static bool TryParseKeyValue(
        string field,
        string expectedKey,
        out int value)
    {
        value = 0;
        var eq = field.IndexOf('=');
        if (eq <= 0) return false;
        if (field.AsSpan(0, eq).SequenceEqual(expectedKey) == false)
            return false;
        return int.TryParse(field.AsSpan(eq + 1), out value);
    }

    private static string Base64NoPad(ReadOnlySpan<byte> bytes)
        => Convert.ToBase64String(bytes).TrimEnd('=');

    private static bool TryBase64NoPadDecode(
        string text,
        out byte[]? bytes)
    {
        bytes = null;
        var padding = (4 - text.Length % 4) % 4;
        var padded = padding == 0 ? text : text + new string('=', padding);

        try
        {
            bytes = Convert.FromBase64String(padded);
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
 * (PasswordHasher.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */