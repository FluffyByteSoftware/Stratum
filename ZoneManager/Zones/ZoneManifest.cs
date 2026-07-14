/*
 * (ZoneManifest.cs)
 *------------------------------------------------------------
 * Created - 7/14/2026 3:06:57 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace ZoneManager.Zones;

/// <summary>
/// The parsed shape of a zone's <c>manifest.json</c> — the file that
/// declares a child directory of the zones root to be a zone. One
/// manifest per zone directory; hand-authored plaintext JSON.
/// </summary>
/// <remarks>
/// The schema (<c>zoneShort</c>, <c>name</c>, <c>description</c>,
/// camelCase keys) is owned by the Game Library project; this type is
/// the reader's mirror of it, not the authority. Fields the schema has
/// filed for later (<c>seed</c>, a spawn point) are deliberately absent
/// until their readers exist. Properties default to
/// <see cref="string.Empty"/> rather than null so a manifest with a
/// missing key fails the store's emptiness validation loudly instead of
/// surfacing as a null reference downstream.
/// </remarks>
public sealed class ZoneManifest
{
    /// <summary>
    /// The zone's short identifier. Must be non-empty and must match
    /// the containing directory name lowercased — the directory and the
    /// manifest naming the same zone is what makes the layout legible
    /// on disk.
    /// </summary>
    public string ZoneShort { get; set; } = string.Empty;

    /// <summary>
    /// The zone's human-readable display name.
    /// </summary>
    public string Name { get; set; } = string.Empty;

    /// <summary>
    /// A short operator-facing description of the zone's purpose.
    /// </summary>
    public string Description { get; set; } = string.Empty;
}

/*
 *------------------------------------------------------------
 * (ZoneManifest.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */