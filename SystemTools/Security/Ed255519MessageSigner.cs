/*
 * (Ed25519MessageSigner.cs)
 *------------------------------------------------------------
 * Created - 6/5/2026 9:14:02 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Org.BouncyCastle.Crypto.Parameters;
using Org.BouncyCastle.Crypto.Signers;
using SystemTools.Logger;

namespace SystemTools.Security;

/// <summary>
/// Provides Ed25519 signature generation functionality.
/// </summary>
/// <remarks>This is the signing counterpart to <see cref="Ed25519Verifier"/>.
/// A private seed produced by <see cref="Ed25519KeyGenerator"/> (or issued by
/// the server on a password login) is reconstructed into a signing key and used
/// to sign a message. Unlike verification, generation has no meaningful failure
/// value, so invalid input and unexpected cryptographic errors throw rather than
/// returning a sentinel.</remarks>
public static class Ed25519MessageSigner
{
    private const int PrivateSeedLength = 32;
    private const int SignatureLength = 64;

    /// <summary>
    /// Generates an Ed25519 signature over a message using the provided private
    /// seed.
    /// </summary>
    /// <param name="privateSeed">The 32-byte Ed25519 private seed used to sign.
    /// </param>
    /// <param name="message">The message to sign.</param>
    /// <returns>The 64-byte Ed25519 signature.</returns>
    /// <exception cref="ArgumentException">The private seed is not exactly
    /// <see cref="PrivateSeedLength"/> bytes.</exception>
    /// <exception cref="InvalidOperationException">The underlying provider
    /// returned a signature of an unexpected length.</exception>
    public static byte[] Sign(
        ReadOnlySpan<byte> privateSeed,
        ReadOnlySpan<byte> message)
    {
        if (privateSeed.Length != PrivateSeedLength)
            throw new ArgumentException(
                $"Private seed length {privateSeed.Length} "
                + $"!= expected {PrivateSeedLength}.",
                nameof(privateSeed));

        try
        {
            var keyParams =
                new Ed25519PrivateKeyParameters(privateSeed.ToArray(), 0);

            var signer = new Ed25519Signer();
            signer.Init(forSigning: true, keyParams);

            var msgBytes = message.ToArray();
            signer.BlockUpdate(msgBytes, 0, msgBytes.Length);

            var signature = signer.GenerateSignature();

            // A 64-byte result is the only thing the verifier will accept;
            // a deviation means the BC contract changed under us.
            if (signature.Length != SignatureLength)
                throw new InvalidOperationException(
                    $"Generated signature length {signature.Length} "
                    + $"!= expected {SignatureLength}.");

            return signature;
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(
                ScribeSeverity.Error,
                "Ed25519 signature generation failed due to an exception.",
                ex));

            throw;
        }
    }
}



/*
 *------------------------------------------------------------
 * (Ed25519MessageSigner.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */