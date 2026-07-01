/*
 * (PingPacket.cs)
 *------------------------------------------------------------
 * Created - 5/29/2026 10:44:30 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;
using System;

namespace Shared.Networking.Packets.LifeCycle;

public readonly struct PingPacket(long senderTimestampMs) : IPacketWritable
{
    public const uint TypeId = PacketIds.LifeCycle.Ping;

    public long SenderTimestampMs { get; } = senderTimestampMs;

    uint IPacketWritable.TypeId => TypeId;

    public void Serialize(NetDataWriter writer)
    {
        writer.Put(SenderTimestampMs);
    }

    public static PingPacket Deserialize(NetDataReader reader)
    {
        try
        {
            var ts = reader.GetLong();

            return new PingPacket(ts);
        }
        catch (InvalidPacketException)
        {
            throw;
        }
        catch(Exception ex)
        {
            throw new InvalidPacketException(
                TypeId, 
                "Failed to deserialize PingPacket.",
                ex);
        }
    }
}


/*
*------------------------------------------------------------
* (PingPacket.cs)
* See License.txt for licensing information.
*-----------------------------------------------------------
*/