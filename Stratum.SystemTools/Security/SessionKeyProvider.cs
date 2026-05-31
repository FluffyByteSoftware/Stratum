/*
 * (SessionKeyProvider.cs)
 *------------------------------------------------------------
 * Created - 5/21/2026 12:25:45 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Stratum.SystemTools.Storage;
using System.Security.Cryptography;

namespace Stratum.SystemTools.Security;

/// <summary>
/// Provides persistent session cryptographic keys with automatic creation and 
/// storage.
/// </summary>
/// <remarks>Keys are stored at a fixed location on disk and reused across sessions. 
/// If no valid key exists, a new cryptographically secure key is automatically 
/// generated.</remarks>
public static class SessionKeyProvider
{
    private const string KeyRelativePath = "keys/session_token.key";
    private const int KeyLength = 32;

    /// <summary>
    /// Loads an existing cryptographic key from disk or creates a new one if not found.
    /// </summary>
    /// <returns>A byte array containing the cryptographic key.</returns>
    public static byte[] LoadOrCreate()
    {
        var disk = DiskManager.Instance;

        try
        {
            var existing = disk.ReadBinFile(KeyRelativePath);
            if (existing.Length == KeyLength)
                return existing;
        }
        catch (FileNotFoundException)
        {
            // No existing key, we'll create one.
        }
        catch (DirectoryNotFoundException)
        {
            // No existing key, we'll create one.
        }

        var fresh = new byte[KeyLength];
        RandomNumberGenerator.Fill(fresh);
        
        disk.WriteBinFile(KeyRelativePath, fresh);

        return fresh;
    }
}

/*
 *------------------------------------------------------------
 * (SessionKeyProvider.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */