/*
 * (CharacterCreateRequestPacket.cs)
 *------------------------------------------------------------
 * Created - 6/27/2026 3:16:16 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using LiteNetLib.Utils;

namespace Shared.Networking.Packets.Character
{
    /// <summary>
    /// Represents a packet for requesting character creation.
    /// </summary>
    public readonly struct CharacterCreateRequestPacket : IPacketWritable
    {
        /// <summary>
        /// Unique identifier for the CharacterCreateRequest packet type.
        /// </summary>
        public const uint TypeId = PacketIds.Character.CharacterCreateRequest;

        /// <summary>
        /// Gets the requested name associated with the operation.
        /// </summary>
        public string RequestedName { get; }

        /// <summary>
        /// Initializes a new instance of the CharacterCreateRequestPacket class with the specified requested 
        /// name.
        /// </summary>
        /// <param name="requestedName">The name requested for the character.</param>
        public CharacterCreateRequestPacket(string requestedName)
        {
            RequestedName = requestedName;
        }

        uint IPacketWritable.TypeId => TypeId;

        /// <summary>
        /// Serializes the requested name to the specified NetDataWriter.
        /// </summary>
        /// <param name="writer">The NetDataWriter to which the requested name is written.</param>
        public void Serialize(NetDataWriter writer)
        {
            writer.Put(RequestedName);
        }

        /// <summary>
        /// Deserializes a CharacterCreateRequestPacket from the specified NetDataReader.
        /// </summary>
        /// <param name="reader">The NetDataReader containing the packet data.</param>
        /// <returns>A deserialized CharacterCreateRequestPacket instance.</returns>
        /// <exception cref="InvalidPacketException">Thrown when the data is invalid or deserialization 
        /// fails.</exception>
        public static CharacterCreateRequestPacket Deserialize(NetDataReader reader)
        {
            try
            {
                var requestedName = reader.GetString();

                return new CharacterCreateRequestPacket(requestedName);
            }
            catch (InvalidPacketException)
            {
                throw;
            }
            catch(Exception ex)
            {
                throw new InvalidPacketException(
                    TypeId,
                    "Failed to deserialize CharacterCreateRequestPacket.",
                    ex);
            }
        }
    }
}

/*
 *------------------------------------------------------------
 * (CharacterCreateRequestPacket.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */