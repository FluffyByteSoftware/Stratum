/*
 * (PongPacket.cs)
 *------------------------------------------------------------
 * Created - 5/29/2026 10:48:47 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;
using System;

namespace Shared.Networking.Packets.LifeCycle;

/// <summary>
/// Represents a Pong packet containing an echoed timestamp from a Ping request.
/// </summary>
/// <remarks>
/// Initializes a new instance of the <see cref="PongPacket"/> class.
/// </remarks>
/// <param name="echoedTimestampMs">The echoed timestamp in milliseconds.</param>
public readonly struct PongPacket(long echoedTimestampMs) : IPacketWritable
{
    /// <summary>
    /// The type identifier for the Pong packet.
    /// </summary>
    public const uint TypeId = MessagePacketIds.LifeCycleMessage.Pong;

    public long EchoedTimestampMs { get; } = echoedTimestampMs;

    uint IPacketWritable.TypeId => TypeId;

    /// <summary>
    /// Serializes the echoed timestamp to the specified writer.
    /// </summary>
    /// <param name="writer">The data writer to write to.</param>
    public void Serialize(NetDataWriter writer)
    {
        writer.Put(EchoedTimestampMs);
    }

    /// <summary>
    /// Deserializes a PongPacket from the specified data reader.
    /// </summary>
    /// <param name="reader">The data reader to read the packet data from.
    /// </param>
    /// <returns>A deserialized PongPacket instance.</returns>
    /// <exception cref="InvalidPacketException">Thrown when the packet data 
    /// is invalid or deserialization fails.</exception>
    public static PongPacket Deserialize(NetDataReader reader)
    {
        try
        {
            var ts = reader.GetLong();
            return new PongPacket(ts);
        }
        catch (InvalidPacketException)
        {
            throw;
        }
        catch(Exception ex)
        {
            throw new InvalidPacketException(
                TypeId,
                "Failed to deserialize PongPacket.",
                ex);
        }
    }
}


/*
*------------------------------------------------------------
* (PongPacket.cs)
* See License.txt for licensing information.
*-----------------------------------------------------------
*/