/*
 * (DisconnectPacket.cs)
 *------------------------------------------------------------
 * Created - 5/29/2026 9:22:14 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using LiteNetLib.Utils;
using Shared.Networking;

namespace Shared.Networking.Packets.LifeCycle;

/// <summary>
/// Represents a disconnect packet containing a reason for the disconnection.
/// </summary>
/// <remarks>
/// Initializes a new instance of the <see cref="DisconnectPacket"/> 
/// class with the specified reason.
/// </remarks>
/// <param name="reason">The disconnect reason.</param>
public readonly struct DisconnectPacket(SecureDisconnectReason reason) : IPacketWritable
{
    public const uint TypeId = PacketIds.Auth.Disconnect;

    /// <summary>
    /// The reason for the disconnect request.
    /// </summary>
    public SecureDisconnectReason Reason { get; } = reason;

    uint IPacketWritable.TypeId => TypeId;

    /// <summary>
    /// Serializes the disconnect reason to the specified writer.
    /// </summary>
    /// <param name="writer">The data writer to serialize to.</param>
    public void Serialize(NetDataWriter writer)
    {
        writer.Put((byte)Reason);
    }

    /// <summary>
    /// Deserializes a <see cref="DisconnectPacket"/> from the specified data reader.
    /// </summary>
    /// <param name="reader">The data reader containing the serialized packet data.</param>
    /// <returns>The deserialized disconnect packet.</returns>
    /// <exception cref="InvalidPacketException">Thrown when the packet data is invalid 
    /// or cannot be deserialized.</exception>
    public static DisconnectPacket Deserialize(NetDataReader reader)
    {
        try
        {
            var b = reader.GetByte();
            return new DisconnectPacket((SecureDisconnectReason)b);
        }
        catch (InvalidPacketException)
        {
            throw;
        }
        catch(Exception ex)
        {
            throw new InvalidPacketException(
                TypeId,
                "Failed to deserialize DisconnectPacket.",
                ex);
        }
    }
}


/*
*------------------------------------------------------------
* (DisconnectPacket.cs)
* See License.txt for licensing information.
*-----------------------------------------------------------
*/