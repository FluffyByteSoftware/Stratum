/*
 * (UdpAuthPacket.cs)
 *------------------------------------------------------------
 * Created - 6/13/2026 10:26:07 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;

namespace Shared.Networking.Packets.Auth;


/// <summary>
/// Result of a UDP session-authentication attempt, as reported by
/// Sentinel to the client after the connection request has been 
/// accepted and a peer established.
/// </summary>
public enum UdpAuthResult : byte
{
    /// <summary>
    /// Unset / invalid.  A correctly issued ack never carries this
    /// value; its presence signals a zeroed buffer or a malformed
    /// packet.
    /// </summary>
    None = 0,

    /// <summary>
    /// The session token validated and the UDP end point is now 
    /// bound to the authenticated account.
    /// </summary>
    Authenticated = 1,
}

/// <summary>
/// Authentication packet for Udp Acknowledgement of session 
/// receipt.
/// </summary>
public struct UdpAuthPacket : IPacketWritable
{
    /// <summary>
    /// Result of the Udp authentication request.
    /// </summary>
    public UdpAuthResult Result;


    /// <summary>
    /// UdpAuthPacket's TypeId.
    /// UdpAuthAck
    /// </summary>
    public readonly uint TypeId => MessagePacketIds.AuthMessage.UdpAuthAck;

    /// <summary>
    /// Serialize the data.
    /// </summary>
    /// <param name="writer"></param>
    public readonly void Serialize(NetDataWriter writer)
    {
        writer.Put((byte)Result);
    }

    public void Deserialize(NetDataReader reader)
    {
        Result = (UdpAuthResult)reader.GetByte();
    }
}

/*
*------------------------------------------------------------
* (UdpAuthPacket.cs)
* See License.txt for licensing information.
*-----------------------------------------------------------
*/