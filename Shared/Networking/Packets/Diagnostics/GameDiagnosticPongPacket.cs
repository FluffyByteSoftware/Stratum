/*
 * (GameDiagnosticPongPacket.cs)
 *------------------------------------------------------------
 * Created - 7/7/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;
using System;

namespace Shared.Networking.Packets.Diagnostics;

/// <summary>
/// Server-to-client diagnostic response on the gameplay channel
/// (<c>0x03</c>), returning the nonce carried by the originating
/// <see cref="GameDiagnosticPingPacket"/> unchanged.
/// </summary>
/// <remarks>
/// The echoed nonce is the whole proof: if it arrives intact, the
/// gameplay channel routed on its high byte, the dispatcher fired the
/// registered handler, and the payload survived serialization both
/// ways. The server never inspects or restamps the value — it is the
/// sender's correlation token and nothing more.
/// </remarks>
/// <param name="echoedNonce">The nonce from the originating ping,
/// returned verbatim.</param>
public readonly struct GameDiagnosticPongPacket(long echoedNonce)
    : IPacketWritable
{
    /// <summary>
    /// The type identifier for the gameplay diagnostic pong packet.
    /// </summary>
    public const uint TypeId =
        MessagePacketIds.ZoneDataMessage.Pong;

    /// <summary>
    /// The nonce from the originating ping, returned verbatim.
    /// </summary>
    public long EchoedNonce { get; } = echoedNonce;

    uint IPacketWritable.TypeId => TypeId;

    /// <summary>
    /// Serializes the echoed nonce to the specified writer.
    /// </summary>
    /// <param name="writer">The data writer to write to.</param>
    public void Serialize(NetDataWriter writer)
    {
        writer.Put(EchoedNonce);
    }

    /// <summary>
    /// Deserializes a <see cref="GameDiagnosticPongPacket"/> from the
    /// specified data reader.
    /// </summary>
    /// <param name="reader">The data reader to read the packet data
    /// from.</param>
    /// <returns>A deserialized <see cref="GameDiagnosticPongPacket"/>
    /// instance.</returns>
    /// <exception cref="InvalidPacketException">Thrown when the packet
    /// data is invalid or deserialization fails.</exception>
    public static GameDiagnosticPongPacket Deserialize(
        NetDataReader reader)
    {
        try
        {
            var nonce = reader.GetLong();
            return new GameDiagnosticPongPacket(nonce);
        }
        catch (InvalidPacketException)
        {
            throw;
        }
        catch (Exception ex)
        {
            throw new InvalidPacketException(
                TypeId,
                "Failed to deserialize GameDiagnosticPongPacket.",
                ex);
        }
    }
}

/*
 *------------------------------------------------------------
 * (GameDiagnosticPongPacket.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */