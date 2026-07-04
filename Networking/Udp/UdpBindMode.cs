/*
 * (UdpBindMode.cs)
 *------------------------------------------------------------
 * Created - 7/1/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Networking.Udp;

/// <summary>
/// Selects which local interfaces a <see cref="UdpHost"/> binds its
/// listening socket to.
/// </summary>
/// <remarks>
/// A two-mode enum rather than a raw bind <c>IPAddress</c> parameter
/// because LiteNetLib's address-binding <c>Start</c> overload takes a
/// paired IPv4/IPv6 address, not a single address — so a lone
/// <c>IPAddress</c> could not be passed through without inventing a v6
/// companion for it. The two modes the project actually needs map
/// cleanly onto that overload: <see cref="AllInterfaces"/> uses the
/// port-only <c>Start</c>, and <see cref="LoopbackOnly"/> passes the
/// loopback address pair. Expressing exactly those two supported modes
/// is more honest than accepting an arbitrary address the host cannot
/// bind. A future "bind one specific NIC" need would extend this enum
/// (or revisit the parameter) at the point it becomes concrete, not
/// speculatively now.
/// <para>
/// <see cref="AllInterfaces"/> is <c>0</c> so it is the default any
/// caller gets by omitting the argument — the pre-existing behavior
/// every current <see cref="UdpHost"/> consumer already relies on,
/// preserved unchanged.
/// </para>
/// </remarks>
public enum UdpBindMode
{
    /// <summary>
    /// Bind the listening socket on all local interfaces (LiteNetLib's
    /// port-only <c>Start</c>). The default; matches the host's original
    /// behavior before a bind mode was selectable.
    /// </summary>
    AllInterfaces = 0,

    /// <summary>
    /// Bind the listening socket to loopback only, so no off-machine
    /// endpoint can reach it. Used for same-machine process-to-process
    /// traffic (e.g. ZoneManager's supervisor bus) where an off-box
    /// sender is never legitimate and network-level spoofing is
    /// therefore out of the threat model.
    /// </summary>
    LoopbackOnly,
}

/*
 *------------------------------------------------------------
 * (UdpBindMode.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */