/*
 * (ZoneRegistrationAcceptedPacket.cs)
 *------------------------------------------------------------
 * Created - 7/6/2026 3:17:35 PM UTC
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Buffers.Binary;
using LiteNetLib.Utils;
using Shared.Networking;

namespace ZoneManager.Registration;

/// <summary>
/// ZoneManager → Zone confirmation that a zone's registration was accepted.
/// Sent on the accept path after the peer is established. Carries the zone
/// id the registration was accepted under, so the zone can confirm the id
/// it dialed with is the id ZoneManager registered.
/// </summary>
/// <remarks>
/// The <see cref="UdpHost"/> send path writes the 4-byte type header; this
/// struct serializes only its payload — the zone id as a 4-byte big-endian
/// value, matching the wire's big-endian discipline.
/// </remarks>
public readonly struct ZoneRegistrationAcceptedPacket(uint zoneId)
    : IPacketWritable
{
    /// <summary>The zone id the registration was accepted under.</summary>
    public uint ZoneId { get; } = zoneId;

    /// <inheritdoc />
    public uint TypeId => RegistrationControlIds.RegistrationAccepted;

    /// <inheritdoc />
    public void Serialize(NetDataWriter writer)
    {
        Span<byte> payload = stackalloc byte[sizeof(uint)];
        BinaryPrimitives.WriteUInt32BigEndian(payload, ZoneId);
        writer.Put(payload);
    }
}

/*
 *------------------------------------------------------------
 * (ZoneRegistrationAcceptedPacket.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */