// =============================================================
//  VersionResponsePacket.cs
// =============================================================

using LiteNetLib.Utils;

namespace Shared.Networking.Packets.Auth;

/// <summary>
/// Client-to-server response to a
/// <see cref="VersionChallengePacket"/>. Carries the client's
/// reported protocol version string for the server to compare
/// against <see cref="Shared.Networking.Packets.Comparable
/// .GameProtocolVersion.Current"/>.
/// </summary>
public struct VersionResponsePacket : IPacketWritable
{
    /// <summary>
    /// The client's reported protocol version string.
    /// </summary>
    public string Version;

    /// <inheritdoc/>
    public readonly uint TypeId => PacketIds.Auth.VersionResponse;

    /// <inheritdoc/>
    public readonly void Serialize(NetDataWriter writer)
    {
        writer.Put(Version);
    }

    /// <summary>
    /// Reads a <see cref="VersionResponsePacket"/> from
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

// =============================================================