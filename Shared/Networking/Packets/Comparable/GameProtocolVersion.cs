/*
 * (ProtocolVersion.cs)
 *------------------------------------------------------------
 * Created - 6/14/2026 1:37:58 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Shared.Networking.Packets.Comparable
{
    /// <summary>
    /// Defines the current wire protocol version advertised by the
    /// server and expected by compatible clients.
    /// </summary>
    /// <remarks>
    /// Compared as a plain string equality check — no parsing, no
    /// arithmetic. Increment on any breaking wire-format or auth-flow
    /// change. Format: "major.minor.patch".
    /// </remarks>
    public static class GameProtocolVersion
    {
        /// <summary>
        /// The current protocol version string sent in every
        /// <see cref="Shared.Networking.Packets.Auth.VersionChallengePacket"/>.
        /// </summary>
        public const string Current = "0.1.0";
    }
}

/*
 *------------------------------------------------------------
 * (ProtocolVersion.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */