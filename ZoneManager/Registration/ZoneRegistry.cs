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
/// see <see cref="TryRegister"/>. Eviction is keyed on the disconnecting
/// peer's id, not the zone id — see <see cref="TryUnregister"/> — so a
/// crashed or relaunched zone frees its slot and can re-register.
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

    /// <summary>
    /// Attempts to evict a live zone when its peer disconnects, keyed by the
    /// disconnecting peer's transport id. Scans for the entry whose
    /// <see cref="UdpConnection.PeerId"/> matches <paramref name="peerId"/>
    /// and removes it. Returns <see langword="true"/> with the evicted zone
    /// id on a match; <see langword="false"/> otherwise.
    /// </summary>
    /// <param name="peerId">The disconnecting peer's transport id
    /// (<c>NetPeer.Id</c>).</param>
    /// <param name="zoneId">On success, the evicted zone id; zero otherwise.</param>
    /// <returns><see langword="true"/> if an entry was evicted;
    /// <see langword="false"/> if no live entry owned that peer id.</returns>
    /// <remarks>
    /// Keyed on peer id, not zone id, deliberately. A rejected duplicate
    /// dialer disconnects carrying the same zone id as the live incumbent,
    /// so evicting by zone id would drop the wrong entry. The duplicate was
    /// never stored (first-wins <see cref="TryRegister"/> refused it), so no
    /// entry owns its peer id and the scan finds nothing — the reject case
    /// needs no special guard. Peer ids are unique per live peer, so at most
    /// one entry matches. The scan is O(n) over at most a couple dozen zones
    /// — trivial — and it keeps eviction independent of whether the peer's
    /// Tag survives into the disconnect callback. The early return exits
    /// before the enumerator advances again, so removing mid-iteration is
    /// safe.
    /// </remarks>
    public bool TryUnregister(int peerId, out uint zoneId)
    {
        lock (_sync)
        {
            foreach (var (id, registration) in _zones)
            {
                if (registration.Connection.PeerId == peerId)
                {
                    _zones.Remove(id);
                    zoneId = id;
                    return true;
                }
            }
        }

        zoneId = 0;
        return false;
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