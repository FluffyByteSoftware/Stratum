/*
 * (Ed25519KeyGenerator.cs)
 *------------------------------------------------------------
 * Created - 5/20/2026 11:11:51 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Stratum.SystemTools.Security;

public readonly record struct Ed25519KeyPair(
    byte[] PrivateSeed,
    byte[] PublicKey);

/// <summary>
/// Provides Ed25519 key generation functionality.
/// </summary>
public static class Ed25519KeyGenerator
{
}



/*
 *------------------------------------------------------------
 * (Ed25519KeyGenerator.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */