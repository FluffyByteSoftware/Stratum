/*
 * (IPacketWritable.cs)
 *------------------------------------------------------------
 * Created - 5/28/2026 2:21:37 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;

namespace Shared.Networking
{
    /// <summary>
    /// A writable packet interface.
    /// </summary>
    public interface IPacketWritable
    {
        /// <summary>
        /// Represents the packet identifier
        /// </summary>
        uint TypeId { get; }

        /// <summary>
        /// Serializes the object to the specified writer.
        /// </summary>
        /// <param name="writer">The writer to serialize to.</param>
        void Serialize(NetDataWriter writer);
    }
}


/*
 *------------------------------------------------------------
 * (IPacketWritable.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */