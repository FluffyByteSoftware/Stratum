/*
 * (VersionResult.cs)
 *------------------------------------------------------------
 * Created - 6/14/2026 3:38:31 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;

namespace Shared.Networking.Packets.Auth
{
    /// <summary>
    /// Server-to-client packet reporting the outcome of the
    /// version-check exchange. On
    /// <see cref="VersionResult.Mismatch"/> the server
    /// disconnects the client immediately after sending.
    /// </summary>
    public struct VersionResultPacket : IPacketWritable
    {
        /// <summary>
        /// The outcome of the server's version comparison.
        /// </summary>
        public VersionResult Result;

        /// <inheritdoc/>
        public readonly uint TypeId => PacketIds.Auth.VersionResult;

        /// <inheritdoc/>
        public readonly void Serialize(NetDataWriter writer)
        {
            writer.Put((byte)Result);
        }

        /// <summary>
        /// Reads a <see cref="VersionResultPacket"/> from
        /// <paramref name="reader"/> into this instance.
        /// </summary>
        /// <param name="reader">
        /// The reader positioned immediately after the 4-byte
        /// type header has been consumed.
        /// </param>
        public void Deserialize(NetDataReader reader)
        {
            Result = (VersionResult)reader.GetByte();
        }
    }
}

/*
 *------------------------------------------------------------
 * (VersionResult.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */
