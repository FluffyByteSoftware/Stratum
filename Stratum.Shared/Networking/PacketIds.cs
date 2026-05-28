/*
 * (PacketIds.cs)
 *------------------------------------------------------------
 * Created - 5/23/2026 10:47:17 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Stratum.Shared.Networking
{
    /// <summary>
    /// Defines packet identifier constants for network protocol messages, 
    /// organized by functional category.
    /// </summary>

    public static class PacketIds
    {
        /// <summary>
        /// Defines constants for authentication protocol packets.
        /// </summary>
        public static class Auth
        {
            /// <summary>
            /// Packet Id for Authentication packet by key
            /// </summary>
            public const uint AuthByKey         = 0x00_00_00_01;
            /// <summary>
            /// Packet Id for Authentication packet by password
            /// </summary>
            public const uint AuthByPassword    = 0x00_00_00_02;
            /// <summary>
            /// Packet Id for Authentication response packet
            /// </summary>
            public const uint AuthResponse      = 0x00_00_00_03;
            /// <summary>
            /// Packet Id for disconnect operation
            /// </summary>
            public const uint Disconnect        = 0x00_00_00_04;
        }

        /// <summary>
        /// Defines lifecycle message type constants for ping and pong operations.
        /// </summary>
        public static class LifeCycle
        {
            /// <summary>
            /// Packet Id for sending a ping request.
            /// </summary>
            public const uint Ping              = 0x01_00_00_01;
            /// <summary>
            /// Packet Id for receiving a pong request.
            /// </summary>
            public const uint POng              = 0x01_00_00_02;
        }
    }
}


/*
 *------------------------------------------------------------
 * (PacketIds.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */