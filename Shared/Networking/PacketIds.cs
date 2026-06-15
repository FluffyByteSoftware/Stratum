/*
 * (PacketIds.cs)
 *------------------------------------------------------------
 * Created - 5/23/2026 10:47:17 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Shared.Networking
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

            /// <summary>
            /// Packet Id for Udp authentication acknowledgement
            /// </summary>
            public const uint UdpAuthAck        = 0x00_00_00_05;

            /// <summary>
            /// Server-to-client challenge carrying the server's current
            /// protocol version string.
            /// </summary>
            public const uint VersionChallenge = 0x00_00_00_06;

            /// <summary>
            /// Client-to-server response carrying the client's reported
            /// protocol version string.
            /// </summary>
            public const uint VersionResponse = 0x00_00_00_07;

            /// <summary>
            /// Server-to-client result indicating whether the reported
            /// version matched. Followed by disconnect on
            /// <see cref="Shared.Networking.Packets.Auth.VersionResultPacket.Mismatch"/>.
            /// </summary>
            public const uint VersionResult = 0x00_00_00_08;
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
            public const uint Pong              = 0x01_00_00_02;
        }
    }
}


/*
 *------------------------------------------------------------
 * (PacketIds.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */