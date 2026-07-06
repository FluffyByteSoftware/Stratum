/*
 * (ZoneRegistry.cs)
 *------------------------------------------------------------
 * Created - 7/6/2026 7:05:21 PM UTC
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Networking.Udp;

namespace ZoneManager.Registration;

/// <summary>
/// The authoritative in-memory registry of live zones, keyed by zone id.
/// ZoneManager owns one instance; entries are added as verified zones
/// complete the registration round trip. Server-only, like the rest of
/// <see cref="RegistrationControlIds"/>'s namespace.
/// </summary>
/// <remarks>
/// Writes currently arrive on the single <see cref="UdpHost"/> poll thread,
/// but the registry is guarded by a lock because later readers (the death
/// clock, cross-zone routing) run on other threads. Registration is
/// first-wins: a zone id already present is rejected, not overwritten —
/// see <see cref="TryRegister"/>. Eviction (dropping an entry when a peer
/// disconnects) is a separate later brick; until it exists, a crashed
/// zone's entry persists until ZoneManager restarts, so a relaunched zone
/// of the same id cannot re-register.
/// </remarks>
internal sealed class ZoneRegistry
{
    private readonly Lock _sync = new();
    private readonly Dictionary<uint, ZoneRegistration> _zones = [];

    /// <summary>
    /// Attempts to register a verified zone. First-wins: if the zone id is
    /// already live, the existing entry stands and this returns
    /// <see langword="false"/> — the caller must reject the duplicate
    /// dialer. On success the entry is stored and this returns
    /// <see langword="true"/>.
    /// </summary>
    /// <param name="zoneId">The authenticated zone id (the registry key).</param>
    /// <param name="connection">The zone's transport connection.</param>
    /// <returns><see langword="true"/> if newly registered;
    /// <see langword="false"/> if the id was already present.</returns>
    public bool TryRegister(uint zoneId, UdpConnection connection)
    {
        var registration = new ZoneRegistration(zoneId, connection);

        lock (_sync)
        {
            // TryAdd is the first-wins gate: no overwrite on a live id.
            return _zones.TryAdd(zoneId, registration);
        }
    }
}

/// <summary>
/// A single live-zone registry entry: the authenticated zone id and the
/// transport connection ZoneManager uses to reach that zone. The id is
/// carried in the value as well as the key so an enumerated entry is
/// self-describing.
/// </summary>
/// <param name="ZoneId">The authenticated zone id.</param>
/// <param name="Connection">The zone's transport connection.</param>
internal sealed record ZoneRegistration(uint ZoneId, UdpConnection Connection);

/*
 *------------------------------------------------------------
 * (ZoneRegistry.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */