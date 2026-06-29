/*
 * (SecureDisconnectReason.cs)
 *------------------------------------------------------------
 * Created - 5/28/2026 2:51:03 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Shared.Networking
{
    /// <summary>
    /// Represents the reason for a secure disconnect operation.
    /// </summary>
    public enum SecureDisconnectReason : byte
    {
        /// <summary>
        /// No reason provided
        /// </summary>
        None = 0x00,
        /// <summary>
        /// Credentials were invalid.
        /// </summary>
        InvalidUserCredentials = 0x01,
        /// <summary>
        /// Account has been locked.
        /// </summary>
        AccountLocked = 0x02,
        /// <summary>
        /// The session key for connecting has expired.
        /// </summary>
        KeyExpired = 0x03,
        /// <summary>
        /// A malformed packet was sent or received.
        /// </summary>
        MalformedPacket = 0x04,
        /// <summary>
        /// The connection has timed out.
        /// </summary>
        Timeout = 0x05,
        /// <summary>
        /// The server is currently locked.
        /// </summary>
        ServerLockout = 0x06,
        /// <summary>
        /// The server is currently locked, and is shutting down.
        /// </summary>
        ServerShuttingDown = 0x07,
        /// <summary>
        /// An internal error has occurred.
        /// </summary>
        InternalError = 0x08,
        /// <summary>
        /// This is used to keep the session alive.
        /// </summary>
        KeepAliveTimeout = 0x09,

        /// <summary>
        /// The character creation failed.
        /// </summary>
        CharacterCreateFailed = 0x0A,

    }
}


/*
 *------------------------------------------------------------
 * (SecureDisconnectReason.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */