/*
 * (GameDiagnosticPingPacket.cs)
 *------------------------------------------------------------
 * Created - 7/7/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;
using System;

namespace Shared.Networking.Packets.Diagnostics;

/// <summary>
/// Client-to-server diagnostic probe on the gameplay channel
/// (<c>0x03</c>), carrying an opaque nonce the server echoes back
/// verbatim in a <see cref="GameDiagnosticPongPacket"/>.
/// </summary>
/// <remarks>
/// This packet exists to prove the gameplay channel's wire path —
/// high-byte routing, dispatch registration, and round-trip
/// serialization — before any real zone traffic exists. It carries a
/// <see cref="Nonce"/> rather than a timestamp deliberately: liveness
/// and RTT are already owned by the LifeCycle Ping/Pong exchange, and
/// naming the field for what it is keeps this from being mistaken for
/// a second keep-alive. The nonce is meaningful only to the sender;
/// the server treats it as an opaque token.
/// </remarks>
/// <param name="nonce">An arbitrary value chosen by the sender,
/// returned unchanged so the sender can correlate the response.</param>
public readonly struct GameDiagnosticPingPacket(long nonce)
    : IPacketWritable
{
    /// <summary>
    /// The type identifier for the gameplay diagnostic ping packet.
    /// </summary>
    public const uint TypeId =
        MessagePacketIds.ZoneDataMessage.Ping;

    /// <summary>
    /// The sender-chosen value to be echoed back verbatim.
    /// </summary>
    public long Nonce { get; } = nonce;

    uint IPacketWritable.TypeId => TypeId;

    /// <summary>
    /// Serializes the nonce to the specified writer.
    /// </summary>
    /// <param name="writer">The data writer to write to.</param>
    public void Serialize(NetDataWriter writer)
    {
        writer.Put(Nonce);
    }

    /// <summary>
    /// Deserializes a <see cref="GameDiagnosticPingPacket"/> from the
    /// specified data reader.
    /// </summary>
    /// <param name="reader">The data reader to read the packet data
    /// from.</param>
    /// <returns>A deserialized <see cref="GameDiagnosticPingPacket"/>
    /// instance.</returns>
    /// <exception cref="InvalidPacketException">Thrown when the packet
    /// data is invalid or deserialization fails.</exception>
    public static GameDiagnosticPingPacket Deserialize(
        NetDataReader reader)
    {
        try
        {
            var nonce = reader.GetLong();
            return new GameDiagnosticPingPacket(nonce);
        }
        catch (InvalidPacketException)
        {
            throw;
        }
        catch (Exception ex)
        {
            throw new InvalidPacketException(
                TypeId,
                "Failed to deserialize GameDiagnosticPingPacket.",
                ex);
        }
    }
}

/*
 *------------------------------------------------------------
 * (GameDiagnosticPingPacket.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */