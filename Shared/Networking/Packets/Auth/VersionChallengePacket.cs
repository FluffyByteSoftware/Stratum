/*
 * (VersionChallengePacket.cs)
 *------------------------------------------------------------
 * Created - 6/14/2026 4:12:14 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib.Utils;

namespace Shared.Networking.Packets.Auth;

/// <summary>
/// Server-to-client packet that opens the version-check
/// exchange. Carries the server's current protocol version
/// string; the client must respond with a
/// <see cref="VersionResponsePacket"/>.
/// </summary>
public struct VersionChallengePacket : IPacketWritable
{
    /// <summary>
    /// The server's current protocol version string.
    /// Populated from <see cref="GameProtocolVersion.Current"/>
    /// before sending; read back verbatim on the receive side.
    /// </summary>
    public string Version;

    /// <inheritdoc/>
    public readonly uint TypeId => MessagePacketIds.AuthMessage.VersionChallenge;

    /// <inheritdoc/>
    public readonly void Serialize(NetDataWriter writer)
    {
        writer.Put(Version);
    }

    /// <summary>
    /// Reads a <see cref="VersionChallengePacket"/> from
    /// <paramref name="reader"/> into this instance.
    /// </summary>
    /// <param name="reader">
    /// The reader positioned immediately after the 4-byte
    /// type header has been consumed.
    /// </param>
    public void Deserialize(NetDataReader reader)
    {
        Version = reader.GetString();
    }
}


/*
*------------------------------------------------------------
* (VersionChallengePacket.cs)
* See License.txt for licensing information.
*-----------------------------------------------------------
*/