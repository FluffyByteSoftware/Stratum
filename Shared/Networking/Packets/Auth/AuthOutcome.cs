/*
 * (AuthOutcome.cs)
 *------------------------------------------------------------
 * Created - 6/26/2026 4:29:44 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Shared.Networking.Packets.Auth
{
    /// <summary>
    /// The result of the login→world decision, carried on every
    /// <see cref="AuthResponsePacket"/> so the client knows how to interpret the
    /// rest of the packet.
    /// </summary>
    /// <remarks>
    /// After TLS authentication succeeds, LoginServer reads the account's
    /// <c>CharacterName</c> back-reference to decide whether the player already owns a
    /// playable character. That single decision is what this enum encodes, and it is
    /// the discriminant the client switches on before reading the conditional fields:
    /// on <see cref="Ok"/> the session token and UDP endpoint are meaningful and the
    /// player proceeds to Sentinel; on <see cref="NeedsCharacter"/> they are empty and
    /// the client must drive character creation first. <see cref="None"/> is the
    /// zero-value loud-failure sentinel in keeping with project convention — the server
    /// never writes it, so reading it back means an uninitialized or corrupt packet,
    /// never a valid outcome.
    /// </remarks>
    public enum AuthOutcome : byte
    {
        /// <summary>
        /// Unset sentinel. Never written by the server; its presence on the wire
        /// indicates an uninitialized or corrupt packet.
        /// </summary>
        None = 0,

        /// <summary>
        /// Authentication succeeded and the account owns a playable character. The
        /// session token and UDP endpoint are populated and the client proceeds to
        /// Sentinel.
        /// </summary>
        Ok = 1,

        /// <summary>
        /// Authentication succeeded but the account has no character yet. The session
        /// token and UDP endpoint are empty; the client must complete character
        /// creation before a token will be issued. Any freshly issued private key
        /// still rides along so a brand-new password account keeps its minted key.
        /// </summary>
        NeedsCharacter = 2,
    }
}

/*
 *------------------------------------------------------------
 * (AuthOutcome.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */