/*
 * (KeepAliveHandler.cs)
 *------------------------------------------------------------
 * Created - 7/1/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Threading.Tasks;
using Networking.Udp;
using Shared.Networking.Packets.LifeCycle;

namespace Sentinel.Handlers;

/// <summary>
/// Handles client-initiated keep-alive probes over UDP by echoing the
/// sender's timestamp straight back. The client owns the probe cadence and
/// computes its own round-trip time from the returned <see cref="PongPacket"/>;
/// Sentinel holds no per-peer state for this exchange.
/// </summary>
/// <remarks>
/// This handler exists to give the client an application-level round-trip
/// measurement and to exercise the UDP dispatch path — <em>not</em> to keep
/// the connection alive. LiteNetLib already runs its own internal keepalive
/// (verified against 2.1.4 source: <c>PingInterval</c> 1000&#160;ms,
/// <c>DisconnectTimeout</c> 5000&#160;ms), so a silent peer is torn down by
/// the transport and surfaces as <c>OnPeerDisconnected</c> without any help
/// from this echo. The handler is therefore deliberately stateless: no
/// outbound-ping tracking, no timeout sweep, no per-peer counters.
///
/// A <see cref="static"/> class rather than an instance one because the echo
/// carries no injected state — unlike LoginServer's stateful handlers, there
/// is no host, registry, or lockout to hold. The single reachable failure
/// mode (a malformed datagram) is caught by the dispatcher before this method
/// is ever invoked, so the body itself cannot fault.
/// </remarks>
internal static class KeepAliveHandler
{
    /// <summary>
    /// Echoes the timestamp carried by an inbound <see cref="PingPacket"/>
    /// back to the originating peer as a <see cref="PongPacket"/>.
    /// </summary>
    /// <param name="connection">The transport wrapper for the originating
    /// peer; the pong is sent back over this same connection.</param>
    /// <param name="packet">The inbound ping carrying the client's send
    /// timestamp, echoed verbatim so the client can compute its own
    /// round-trip time.</param>
    /// <returns>A completed <see cref="ValueTask"/> — the echo is synchronous
    /// and the send completes on the poll thread before returning.</returns>
    /// <remarks>
    /// The timestamp is echoed verbatim rather than restamped: it is the
    /// client's own clock reading, meaningful only to the client for RTT, so
    /// Sentinel treats it as an opaque token to bounce back untouched.
    /// </remarks>
    internal static ValueTask OnPing(
        UdpConnection connection, PingPacket packet)
    {
        connection.Send(new PongPacket(packet.SenderTimestampMs));
        return ValueTask.CompletedTask;
    }
}

/*
 *------------------------------------------------------------
 * (KeepAliveHandler.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */