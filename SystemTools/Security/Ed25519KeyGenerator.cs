/*
 * (Ed25519KeyGenerator.cs)
 *------------------------------------------------------------
 * Created - 5/20/2026 11:11:51 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Org.BouncyCastle.Crypto;
using Org.BouncyCastle.Crypto.Generators;
using Org.BouncyCastle.Crypto.Parameters;
using Org.BouncyCastle.Security;
using SystemTools.Logger;

namespace SystemTools.Security;

public readonly record struct Ed25519KeyPair(
    byte[] PrivateSeed,
    byte[] PublicKey);

/// <summary>
/// Provides Ed25519 key generation functionality.
/// </summary>
public static class Ed25519KeyGenerator
{
    /// <summary>
    /// Utilizes Bouncy Castle's Ed25519KeyPairGenerator to create the key pair, 
    /// and logs any exceptions that occur during generation.
    /// </summary>
    /// <returns>Key pair to be used</returns>
    public static Ed25519KeyPair Generate()
    {
        try
        {
            var generator = new Ed25519KeyPairGenerator();
            generator.Init(new Ed25519KeyGenerationParameters(new SecureRandom()));

            AsymmetricCipherKeyPair pair = generator.GenerateKeyPair();
            var priv = (Ed25519PrivateKeyParameters)pair.Private;
            var pub = (Ed25519PublicKeyParameters)pair.Public;

            return new Ed25519KeyPair(priv.GetEncoded(), pub.GetEncoded());
        }
        catch(Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error, $"Exception generating keypair.", ex));

            throw;
        }
    }
}



/*
 *------------------------------------------------------------
 * (Ed25519KeyGenerator.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */