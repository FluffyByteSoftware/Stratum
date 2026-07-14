/*
 * (ZoneStore.cs)
 *------------------------------------------------------------
 * Created - 7/14/2026 3:06:57 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Collections.Concurrent;
using SystemTools;
using SystemTools.Logger;
using SystemTools.Storage;

namespace ZoneManager.Zones;

/// <summary>
/// Discovers every zone declared under the zones root at boot and
/// answers lookups by zone short-name thereafter. Discovery only:
/// a zone in this store is <em>known</em>, not <em>running</em> —
/// liveness (boot, freeze, thaw) is a separate concern this store
/// deliberately knows nothing about.
/// </summary>
/// <remarks>
/// The disk is the game library: each child directory of
/// <see cref="Constellations.ZonesPath"/> is one zone, declared by the
/// <c>manifest.json</c> it contains. Read-only by design — manifests
/// are authored by hand (and eventually by worldgen), so the scan
/// skips-and-warns on anything malformed and never deletes or repairs.
/// Thin policy over <see cref="JsonContentScanner"/>: this class
/// describes what a valid manifest looks like; the loop mechanics live
/// in the shared utility.
/// </remarks>
public sealed class ZoneStore
{
    private const string ManifestFileName = "manifest.json";
    private const string ZonesRelativeDir = "zones";

    private static ZoneStore? _instance;
    private static bool _initialized;

    /// <summary>
    /// The singleton instance of the <see cref="ZoneStore"/>.
    /// </summary>
    /// <exception cref="InvalidOperationException">Accessed before
    /// <see cref="Initialize"/> has run.</exception>
    public static ZoneStore Instance =>
        _instance
        ?? throw new InvalidOperationException(
            "ZoneStore not initialized.");

    /// <summary>
    /// Gets a value indicating whether the store is initialized.
    /// </summary>
    public static bool IsRunning => _initialized && _instance is not null;

    private readonly ConcurrentDictionary<string, ZoneManifest>
        _zones = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Initializes the singleton and performs the boot scan of the
    /// zones directory. Idempotent: a second call is a no-op.
    /// </summary>
    public static void Initialize()
    {
        if (_initialized) return;
        _instance = new ZoneStore();
        _initialized = true;
        _instance.BootScan();
    }

    private ZoneStore() { }

    /// <summary>
    /// Attempts to retrieve the manifest for a known zone.
    /// </summary>
    /// <param name="zoneShort">The zone's short identifier;
    /// case-insensitive.</param>
    /// <param name="manifest">The manifest, if the zone is known.</param>
    /// <returns><c>true</c> if the zone was discovered at boot.</returns>
    public bool TryGet(string zoneShort, out ZoneManifest manifest)
        => _zones.TryGetValue(zoneShort, out manifest!);

    /// <summary>
    /// Gets every known zone's manifest, in no guaranteed order.
    /// </summary>
    public IReadOnlyCollection<ZoneManifest> ListAll()
        => [.. _zones.Values];

    /// <summary>
    /// The number of zones discovered. Zero after a boot scan is a fact
    /// worth logging, not an error.
    /// </summary>
    public int Count => _zones.Count;

    /// <summary>
    /// Clears all in-memory manifests and marks the store stopped.
    /// </summary>
    /// <returns>A completed task, matching the store lifecycle
    /// convention.</returns>
    public Task ShutdownAsync()
    {
        if (!_initialized) return Task.CompletedTask;
        _initialized = false;
        _zones.Clear();
        return Task.CompletedTask;
    }

    /// <summary>
    /// Walks the zones root and loads every well-formed manifest.
    /// </summary>
    /// <remarks>
    /// Enumeration is one <see cref="JsonContentScanner.Load{T}"/> call
    /// per child directory rather than one call for all of them,
    /// because validation needs the containing directory's name and the
    /// scanner's callback only sees the file name — which is
    /// <c>manifest.json</c> for every zone. A child directory with no
    /// manifest at all is warned here rather than handed to the
    /// scanner, so the warning says what is actually wrong instead of
    /// surfacing as a read failure.
    /// </remarks>
    private void BootScan()
    {
        if (!Directory.Exists(Constellations.ZonesPath))
        {
            Directory.CreateDirectory(Constellations.ZonesPath);
            Scribe.Pump(new ScribeMessage(
                ScribeSeverity.Warn,
                "Zones directory did not exist; created empty at "
                    + $"'{Constellations.ZonesPath}'. No zones "
                    + "discovered."));
            return;
        }

        var zoneDirs = Directory.EnumerateDirectories(
            Constellations.ZonesPath);

        foreach (var zoneDir in zoneDirs)
        {
            var dirName = Path.GetFileName(zoneDir);
            var manifestFullPath =
                Path.Combine(zoneDir, ManifestFileName);

            if (!File.Exists(manifestFullPath))
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Zone directory '{dirName}' has no "
                        + $"'{ManifestFileName}'; skipping."));
                continue;
            }

            var relativePath =
                $"{ZonesRelativeDir}/{dirName}/{ManifestFileName}";

            JsonContentScanner.Load<ZoneManifest>(
                [relativePath],
                (manifest, _) => Accept(manifest, dirName));
        }

        Scribe.Pump(new ScribeMessage(
            ScribeSeverity.Info,
            $"Zone scan complete — {Count} zone(s) discovered."));
    }

    /// <summary>
    /// Validates one parsed manifest against its containing directory
    /// and stores it if well-formed.
    /// </summary>
    /// <param name="manifest">The parsed manifest.</param>
    /// <param name="dirName">The name of the directory the manifest
    /// was found in.</param>
    /// <returns><c>null</c> on acceptance, or the skip reason.</returns>
    private string? Accept(ZoneManifest manifest, string dirName)
    {
        if (string.IsNullOrWhiteSpace(manifest.ZoneShort))
            return "manifest has an empty or missing 'zoneShort'.";

        var expectedDirName =
            manifest.ZoneShort.ToLowerInvariant();
        if (!string.Equals(
            dirName,
            expectedDirName,
            StringComparison.Ordinal))
        {
            return $"manifest declares zoneShort "
                + $"'{manifest.ZoneShort}' but lives in directory "
                + $"'{dirName}' (expected '{expectedDirName}').";
        }

        if (!_zones.TryAdd(manifest.ZoneShort, manifest))
            return $"duplicate zoneShort '{manifest.ZoneShort}'.";

        Scribe.Pump(new ScribeMessage(
            ScribeSeverity.Info,
            $"Loaded zone '{manifest.ZoneShort}' "
                + $"('{manifest.Name}')."));
        return null;
    }
}

/*
 *------------------------------------------------------------
 * (ZoneStore.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */