/*
 * (Ed25519Verifier.cs)
 *------------------------------------------------------------
 * Created - 5/20/2026 11:03:23 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Org.BouncyCastle.Crypto.Parameters;
using Org.BouncyCastle.Crypto.Signers;
using SystemTools.Logger;

namespace SystemTools.Security;

/// <summary>
/// Provides Ed25519 signature verification functionality.
/// </summary>
public static class Ed25519Verifier
{
    private const int PublicKeyLength = 32;
    private const int SignatureLength = 64;

    /// <summary>
    /// Verifies an Ed25519 signature against a message using the provided public key.
    /// </summary>
    /// <param name="publicKey">The Ed25519 public key used for verification.</param>
    /// <param name="message">The message that was signed.</param>
    /// <param name="signature">The Ed25519 signature to verify.</param>
    /// <returns><see langword="true"/> if the signature is valid; otherwise, <see langword="false"/>.</returns>
    public static bool Verify(
        ReadOnlySpan<byte> publicKey,
        ReadOnlySpan<byte> message,
        ReadOnlySpan<byte> signature)
    {
        if (publicKey.Length != PublicKeyLength) return false;
        if (signature.Length != SignatureLength) return false;

        try
        {
            var keyParams = new Ed25519PublicKeyParameters(publicKey.ToArray(), 0);

            var signer = new Ed25519Signer();
            signer.Init(forSigning: false, keyParams);

            var msgBytes = message.ToArray();
            signer.BlockUpdate(msgBytes, 0, msgBytes.Length);

            return signer.VerifySignature(signature.ToArray());
        }
        catch(Exception ex)
        {
            Scribe.Pump(new ScribeMessage(
                ScribeSeverity.Warn,
                "Ed25519 signature verification failed due to an exception.", ex));

            return false;
        }
    }
}



/*
 *------------------------------------------------------------
 * (Ed25519Verifier.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */