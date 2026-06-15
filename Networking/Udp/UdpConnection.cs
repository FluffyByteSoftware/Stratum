/*
 * (UdpConnection.cs)
 *------------------------------------------------------------
 * Created - 6/14/2026 6:49:42 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib;
using Shared.Networking;

namespace Networking.Udp;

/// <summary>
/// Represents an authenticated UDP peer connection. Transport-only
/// wrapper around a <see cref="NetPeer"/>; account identity is held
/// separately by <c>Sentinel.ClientSessionRegistry</c>.
/// </summary>
/// <remarks>
/// Mirrors <c>TcpConnection</c> in role — the <c>TConnection</c> the
/// dispatcher hands to packet handlers — but deliberately omits the
/// send lock and disconnect-reason machinery that belong to the TCP
/// secure-disconnect flow. All handlers fire on the single
/// <see cref="UdpHost"/> poll thread, so sends are inherently
/// serialized.
/// </remarks>
/// <param name="peer">The underlying LiteNetLib peer.</param>
public sealed class UdpConnection(NetPeer peer)
{
    private readonly NetPeer _peer = peer;

    /// <summary>
    /// Gets the peer identifier. Matches <see cref="NetPeer.Id"/> and
    /// is used as the key into <c>ClientSessionRegistry</c>.
    /// </summary>
    public int PeerId { get; } = peer.Id;

    /// <summary>
    /// Serializes and sends <paramref name="packet"/> to this peer
    /// reliably on channel 0.
    /// </summary>
    /// <typeparam name="T">
    /// A value type implementing <see cref="IPacketWritable"/>.
    /// </typeparam>
    /// <param name="packet">The packet to send.</param>
    public void Send<T>(T packet) where T : struct, IPacketWritable
    {
        UdpHost.Send(_peer, packet);
    }

    /// <summary>
    /// Disconnects the underlying peer immediately. Called by post-auth
    /// handlers that need to terminate a session — e.g. on version
    /// mismatch after the result packet has been sent.
    /// </summary>
    public void Disconnect()
    {
        _peer.Disconnect();
    }
}

/*
*------------------------------------------------------------
* (UdpConnection.cs)
* See License.txt for licensing information.
*-----------------------------------------------------------
*/