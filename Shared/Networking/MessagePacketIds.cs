/*
 * (MessagePacketIds.cs)
 *------------------------------------------------------------
 * Created - 5/23/2026 10:47:17 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Shared.Networking;

/// <summary>
/// Defines packet identifier constants for network protocol messages, 
/// organized by functional category.
/// </summary>

public static class MessagePacketIds
{
    /// <summary>
    /// Defines constants for authentication protocol packets.
    /// </summary>
    public static class AuthMessage
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
    public static class LifeCycleMessage
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

    /// <summary>
    /// Provides constants for character-related request and response identifiers.
    /// </summary>
    public static class CharacterMessage
    {
        /// <summary>
        /// Represents the message identifier for a character creation request.
        /// </summary>
        public const uint CharacterCreateRequest    = 0x02_00_00_01;

        /// <summary>
        /// Represents the response code for character creation.
        /// </summary>
        public const uint CharacterCreateResponse   = 0x02_00_00_02;
    }

    /// <summary>
    /// Provides constants for world persistence and loading.
    /// </summary>
    public static class ZoneDataMessage
    {
        /// <summary>
        /// Identifies the zone load data request from the client.
        /// </summary>
        public const uint ZoneDataRequest   = 0x03_00_00_01;

        /// <summary>
        /// Identifies the Zone Data Response message type from the server.
        /// This will carry the payload with where all objects inside the zone are currently located,
        /// their current rotation, and their current state.
        /// May need to refine further down into a LocalToPlayerDataRequest to avoid pulling information 
        /// about the entire zone, when the player only needs their immediate surroundings.
        /// </summary>
        public const uint ZoneDataResponse  = 0x03_00_00_02;

        /// <summary>
        /// Client is requesting to spawn in this zone at these coordinates.
        /// </summary>
        public const uint ZoneSpawnRequest  = 0x03_00_00_03;
        /// <summary>
        /// Server response to the client request to spawn at X,Y,Z coordinates rotated at X,Y,Z.
        /// </summary>
        public const uint ZoneSpawnResponse = 0x03_00_00_04;
        /// <summary>
        /// Client is requesting to exit the zone at these coordinates.
        /// </summary>
        public const uint ZoneExitRequest   = 0x03_00_00_05;
        /// <summary>
        /// Server is approving or denying the client's exit.
        /// </summary>
        public const uint ZoneExitResponse  = 0x03_00_00_06;
    }   
    
}


/*
*------------------------------------------------------------
* (MessagePacketIds.cs)
* See License.txt for licensing information.
*-----------------------------------------------------------
*/