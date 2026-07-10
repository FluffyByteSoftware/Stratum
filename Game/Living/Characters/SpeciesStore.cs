/*
 * (SpeciesStore.cs)
 *------------------------------------------------------------
 * Created - 7/10/2026 2:52:43 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Collections.Concurrent;
using System.Text.Json;
using Shared.Game.Characters;
using SystemTools;
using SystemTools.Logger;
using SystemTools.Storage;

namespace Game.Living.Characters;

/// <summary>
/// Loads every species definition from the species content directory at
/// boot and answers lookups by <see cref="PlayableSpecies"/> thereafter.
/// Read-only by design: species files are authored by hand, so the server
/// never writes, creates, or deletes them — a bad file is an operator
/// problem to fix on disk, not something the boot scan repairs.
/// </summary>
/// <remarks>
/// Mirrors <c>CharacterStore</c>'s boot-scan manners (skip-and-warn, never
/// delete) but not its write paths, because content flows in the opposite
/// direction: characters are server-written records, species are
/// human-written definitions. An empty or missing directory is reported at
/// boot rather than treated as an error — no definitions is a true fact
/// the operator should see, and unlike config there is no sensible default
/// to generate.
/// </remarks>
public sealed class SpeciesStore
{
    private const string SpeciesRelativeDir = "game/species";
    private const string FilenameSuffix = ".json";
    private const string SearchPattern = "*.json";

    private static SpeciesStore? _instance;
    private static bool _initialized;

    /// <summary>
    /// The singleton instance of the <see cref="SpeciesStore"/>.
    /// </summary>
    /// <exception cref="InvalidOperationException">Accessed before
    /// <see cref="Initialize"/> has run.</exception>
    public static SpeciesStore Instance =>
        _instance
        ?? throw new InvalidOperationException(
            "SpeciesStore not initialized.");

    /// <summary>
    /// Gets a value indicating whether the store is initialized.
    /// </summary>
    public static bool IsRunning => _initialized && _instance is not null;

    private readonly ConcurrentDictionary<PlayableSpecies, SpeciesDefinition>
        _definitions = new();

    /// <summary>
    /// Initializes the singleton and performs the boot scan of the species
    /// directory. Idempotent: a second call is a no-op.
    /// </summary>
    public static void Initialize()
    {
        if (_initialized) return;
        _instance = new SpeciesStore();
        _initialized = true;
        _instance.BootScan();
    }

    private SpeciesStore() { }

    /// <summary>
    /// Attempts to retrieve the loaded definition for a species.
    /// </summary>
    /// <param name="species">The species to look up. <c>None</c> never
    /// resolves — it is the uninitialized sentinel, not a species.</param>
    /// <param name="definition">The definition, if one was loaded.</param>
    /// <returns><c>true</c> if a definition was found.</returns>
    public bool TryGet(
        PlayableSpecies species,
        out SpeciesDefinition definition)
        => _definitions.TryGetValue(species, out definition);

    /// <summary>
    /// Gets every loaded definition, in no guaranteed order.
    /// </summary>
    public IReadOnlyCollection<SpeciesDefinition> ListAll()
        => [.. _definitions.Values];

    /// <summary>
    /// The number of definitions currently loaded. Zero after a boot scan
    /// is a fact worth logging, not an error.
    /// </summary>
    public int Count => _definitions.Count;

    /// <summary>
    /// Clears all in-memory definitions and marks the store stopped.
    /// </summary>
    /// <returns>A completed task, matching the store lifecycle
    /// convention.</returns>
    public Task ShutdownAsync()
    {
        if (!_initialized) return Task.CompletedTask;
        _initialized = false;
        _definitions.Clear();
        return Task.CompletedTask;
    }

    /// <summary>
    /// Scans the species directory and loads every well-formed definition.
    /// </summary>
    /// <remarks>
    /// A file that fails to parse, carries <c>None</c> as its id, has no
    /// limbs, or whose file name disagrees with its id is skipped with a
    /// warning and left untouched. The no-limbs check exists because the
    /// serializer silently ignores unknown keys — a typo'd
    /// <c>"limbs"</c> key would otherwise load as an empty body and pass
    /// quietly; validating the result catches what the parse cannot.
    /// </remarks>
    private void BootScan()
    {
        var disk = DiskManager.Instance;
        var speciesRoot = Path.Combine(
            disk.RootPath,
            SpeciesRelativeDir);

        if (!Directory.Exists(speciesRoot))
        {
            Directory.CreateDirectory(speciesRoot);
            Scribe.Pump(new ScribeMessage(
                ScribeSeverity.Warn,
                "Species directory did not exist; created empty at "
                    + $"'{speciesRoot}'. No definitions loaded."));
            return;
        }

        var files = Directory.EnumerateFiles(
            speciesRoot,
            SearchPattern,
            SearchOption.TopDirectoryOnly);

        foreach (var fullPath in files)
        {
            var fileName = Path.GetFileName(fullPath);
            var relativePath = $"{SpeciesRelativeDir}/{fileName}";

            SpeciesDefinition definition;
            try
            {
                var json = disk.ReadTextFile(relativePath);
                definition = JsonSerializer
                    .Deserialize<SpeciesDefinition>(
                        json,
                        JsonConfigurator.ContentIndented);
            }
            catch (Exception ex)
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Failed to load species file '{fileName}'.",
                    ex));
                continue;
            }

            if (definition.Id == PlayableSpecies.None)
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Species file '{fileName}' has id 'None' or a "
                        + "missing id; skipping."));
                continue;
            }

            if (definition.Limbs is null || definition.Limbs.Count == 0)
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Species file '{fileName}' loaded with no limbs "
                        + "(missing or typo'd key?); skipping."));
                continue;
            }

            var expectedFileName =
                definition.Id.ToString().ToLowerInvariant()
                    + FilenameSuffix;
            if (!string.Equals(
                fileName,
                expectedFileName,
                StringComparison.OrdinalIgnoreCase))
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Species file '{fileName}' declares id "
                        + $"'{definition.Id}' but should be named "
                        + $"'{expectedFileName}'; skipping."));
                continue;
            }

            if (!_definitions.TryAdd(definition.Id, definition))
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Duplicate species id '{definition.Id}' in "
                        + $"'{fileName}'; skipping."));
                continue;
            }

            Scribe.Pump(new ScribeMessage(
                ScribeSeverity.Info,
                $"Loaded species '{definition.Name}' "
                    + $"({definition.Id}) from '{fileName}'."));
        }

        Scribe.Pump(new ScribeMessage(
            ScribeSeverity.Info,
            $"Species boot scan complete: {Count} definition(s) "
                + "loaded."));
    }
}

/*
 *------------------------------------------------------------
 * (SpeciesStore.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */