/*
 * (VersionResult.cs)
 *------------------------------------------------------------
 * Created - 6/14/2026 4:21:51 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Shared.Networking.Packets.Auth;

/// <summary>
/// Outcome codes carried by a
/// <see cref="VersionResultPacket"/> in response to a
/// client's <see cref="VersionResponsePacket"/>.
/// </summary>
public enum VersionResult : byte
{
    /// <summary>
    /// Default value; should never appear on the wire.
    /// A received packet carrying this value indicates a
    /// serialization error on the sender's side.
    /// </summary>
    None = 0,

    /// <summary>
    /// The client's reported protocol version matches the
    /// server's <see cref="Shared.Networking.Packets.Comparable
    /// .GameProtocolVersion.Current"/>. The session may proceed.
    /// </summary>
    Ok = 1,

    /// <summary>
    /// The client's reported protocol version does not match
    /// the server's expected version. The server will disconnect
    /// the client immediately after sending this result.
    /// </summary>
    Mismatch = 2,
}



/*
*------------------------------------------------------------
* (VersionResult.cs)
* See License.txt for licensing information.
*-----------------------------------------------------------
*/
