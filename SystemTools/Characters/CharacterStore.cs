/*
 * (CharacterStore.cs)
 *------------------------------------------------------------
 * Created - 6/15/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */
using System.Collections.Concurrent;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using SystemTools.Logger;
using SystemTools.Storage;

namespace SystemTools.Characters;

/// <summary>
/// Manages character records with thread-safe in-memory storage and JSON
/// file-based persistence, one file per character at
/// <c>characters/{characterName}.json</c>.
/// </summary>
/// <remarks>
/// Singleton; initialize via <see cref="Initialize"/> before touching
/// <see cref="Instance"/>. The character name is the lowercase canonical key:
/// it is the dictionary key, the file name, and the back-reference target from
/// the owning account. Keying the file by name is a deliberate choice — it makes
/// the characters directory human-legible (the file listing is the roster) and
/// enforces global name uniqueness for free, since the filesystem cannot hold two
/// files of the same name. Names must be 3-14 lowercase letters. Display
/// capitalization and titles live in <see cref="CharacterRecord.FullName"/>, not
/// here. All operations are thread-safe via concurrent collections.
/// </remarks>
public sealed partial class CharacterStore
{
    private const string CharactersRelativeDir = "characters";
    private const string FilenameSuffix = ".json";
    private const string SearchPattern = "*.json";
    private const int MinNameLength = 3;
    private const int MaxNameLength = 14;

    /// <summary>
    /// Matches a valid character name: lowercase letters only. Length is checked
    /// separately so this pattern stays a pure charset assertion.
    /// </summary>
    [GeneratedRegex("^[a-z]+$")]
    private static partial Regex NameCharsetPattern();

    private static CharacterStore? _instance;
    private static bool _initialized;

    /// <summary>
    /// The singleton instance of the <see cref="CharacterStore"/>.
    /// </summary>
    /// <exception cref="InvalidOperationException">Accessed before
    /// <see cref="Initialize"/> has run.</exception>
    public static CharacterStore Instance =>
        _instance
        ?? throw new InvalidOperationException(
            "CharacterStore not initialized.");

    /// <summary>
    /// Gets a value indicating whether the store is initialized and running.
    /// </summary>
    public static bool IsRunning => _initialized && _instance is not null;

    private readonly ConcurrentDictionary<string, CharacterRecord> _records
        = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Initializes the singleton and performs an initial boot scan of the
    /// characters directory. Idempotent: a second call is a no-op.
    /// </summary>
    public static void Initialize()
    {
        if (_initialized) return;
        _instance = new CharacterStore();
        _initialized = true;
        _instance.BootScan();
    }

    private CharacterStore() { }

    /// <summary>
    /// Attempts to retrieve a character record by its name.
    /// </summary>
    /// <param name="characterName">The lowercase character name to look up.</param>
    /// <param name="record">When this method returns, contains the record if
    /// found; otherwise <c>null</c>.</param>
    /// <returns><c>true</c> if a record was found; otherwise <c>false</c>.</returns>
    public bool TryGet(
        string characterName,
        out CharacterRecord record)
    {
        if (string.IsNullOrEmpty(characterName))
        {
            record = null!;
            return false;
        }

        if (_records.TryGetValue(characterName, out var found))
        {
            record = found;
            return true;
        }

        record = null!;
        return false;
    }

    /// <summary>
    /// Gets all character names in case-insensitive sorted order.
    /// </summary>
    /// <returns>A read-only list of character names.</returns>
    public IReadOnlyList<string> ListNames()
    {
        var names = new List<string>(_records.Count);
        foreach (var kv in _records)
            names.Add(kv.Value.CharacterName);
        names.Sort(StringComparer.OrdinalIgnoreCase);
        return names;
    }

    /// <summary>
    /// Adds a new character record and persists it to disk.
    /// </summary>
    /// <param name="record">The character record to add.</param>
    /// <remarks>
    /// Persist failure rolls the in-memory add back, so the dictionary never
    /// holds a character that is not also on disk.
    /// </remarks>
    /// <exception cref="InvalidOperationException">A character with the same
    /// name already exists.</exception>
    /// <exception cref="ArgumentException">The record's name fails validation.</exception>
    public void Add(CharacterRecord record)
    {
        ValidateRecord(record);

        if (!_records.TryAdd(record.CharacterName, record))
            throw new InvalidOperationException(
                $"Character '{record.CharacterName}' already exists.");

        try
        {
            PersistRecord(record);
        }
        catch
        {
            _records.TryRemove(record.CharacterName, out _);
            throw;
        }
    }

    /// <summary>
    /// Updates an existing character record and persists the change.
    /// </summary>
    /// <param name="record">The character record to update.</param>
    /// <remarks>
    /// Persist failure restores the prior in-memory value, keeping memory and
    /// disk consistent.
    /// </remarks>
    /// <exception cref="InvalidOperationException">The character does not exist
    /// or was modified concurrently.</exception>
    /// <exception cref="ArgumentException">The record's name fails validation.</exception>
    public void Update(CharacterRecord record)
    {
        ValidateRecord(record);

        if (!_records.TryGetValue(record.CharacterName, out var existing))
            throw new InvalidOperationException(
                $"Character '{record.CharacterName}' does not exist.");

        if (!_records.TryUpdate(record.CharacterName, record, existing))
            throw new InvalidOperationException(
                $"Character '{record.CharacterName}' was modified "
                    + "concurrently.");

        try
        {
            PersistRecord(record);
        }
        catch
        {
            _records.TryUpdate(record.CharacterName, existing, record);
            throw;
        }
    }

    /// <summary>
    /// Removes a character record and deletes its file from disk.
    /// </summary>
    /// <param name="characterName">The name of the character to remove.</param>
    /// <remarks>
    /// Disk-delete failure re-adds the record to memory so the two views stay in
    /// agreement.
    /// </remarks>
    /// <exception cref="InvalidOperationException">The character does not exist.</exception>
    /// <exception cref="ArgumentException">The name fails validation.</exception>
    public void Remove(string characterName)
    {
        ValidateName(characterName);

        if (!_records.TryRemove(characterName, out var removed))
            throw new InvalidOperationException(
                $"Character '{characterName}' does not exist.");

        try
        {
            DiskManager.Instance.DeleteFile(
                BuildRelativePath(characterName));
        }
        catch
        {
            _records.TryAdd(characterName, removed);
            throw;
        }
    }

    /// <summary>
    /// Shuts the store down and clears all in-memory state.
    /// </summary>
    /// <returns>A completed task; shutdown is synchronous but exposed as a task
    /// to match the store lifecycle convention.</returns>
    public Task ShutdownAsync()
    {
        if (!_initialized) return Task.CompletedTask;
        _initialized = false;
        _records.Clear();
        return Task.CompletedTask;
    }

    /// <summary>
    /// Scans the characters directory at startup and loads every well-formed
    /// record into memory.
    /// </summary>
    /// <remarks>
    /// A file that cannot be read, fails to parse, carries an invalid name, or
    /// whose stored name disagrees with its file name is skipped with a warning
    /// and left untouched on disk. A character file is the least-replaceable thing
    /// on disk, so a single bad file is treated as a problem for the operator to
    /// resolve, never something the boot scan deletes or rewrites.
    /// </remarks>
    private void BootScan()
    {
        var disk = DiskManager.Instance;
        var charactersRoot = Path.Combine(
            disk.RootPath,
            CharactersRelativeDir);

        if (!Directory.Exists(charactersRoot))
        {
            Directory.CreateDirectory(charactersRoot);
            return;
        }

        var files = Directory.EnumerateFiles(
            charactersRoot,
            SearchPattern,
            SearchOption.TopDirectoryOnly);

        foreach (var fullPath in files)
        {
            var fileName = Path.GetFileName(fullPath);
            var name = ExtractNameFromFilename(fileName);
            if (name is null)
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Skipping unrecognized character file: "
                        + $"'{fileName}'."));
                continue;
            }

            var relativePath = $"{CharactersRelativeDir}/{fileName}";
            CharacterRecord? record;
            try
            {
                var json = disk.ReadTextFile(relativePath);
                record = JsonSerializer.Deserialize<CharacterRecord>(
                    json,
                    JsonConfigurator.ContentIndented);
            }
            catch (Exception ex)
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Failed to load character file '{fileName}'.",
                    ex));
                continue;
            }

            if (record is null
                || string.IsNullOrEmpty(record.CharacterName)
                || !IsValidName(record.CharacterName)
                || !string.Equals(
                    record.CharacterName,
                    name,
                    StringComparison.OrdinalIgnoreCase))
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Character file '{fileName}' has invalid or "
                        + "mismatched record; skipping."));
                continue;
            }

            if (!_records.TryAdd(record.CharacterName, record))
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Duplicate character name '{record.CharacterName}' "
                        + $"in '{fileName}'; skipping."));
            }
        }
    }

    /// <summary>
    /// Serializes a record to JSON and writes it through
    /// <see cref="DiskManager"/> at its name-derived path.
    /// </summary>
    private static void PersistRecord(CharacterRecord record)
    {
        var json = JsonSerializer.Serialize(record, JsonConfigurator.ContentIndented);
        var relativePath = BuildRelativePath(record.CharacterName);
        DiskManager.Instance.WriteTextFile(relativePath, json);
    }

    /// <summary>
    /// Builds the directory-relative path for a character's file.
    /// </summary>
    private static string BuildRelativePath(string characterName)
        => $"{CharactersRelativeDir}/{BuildFileName(characterName)}";

    /// <summary>
    /// Builds the file name for a character. The name is forced lowercase so the
    /// file name always matches the canonical key, even though valid names are
    /// already lowercase by validation.
    /// </summary>
    private static string BuildFileName(string characterName)
        => characterName.ToLowerInvariant() + FilenameSuffix;

    /// <summary>
    /// Recovers the character name from a file name, or returns <c>null</c> if the
    /// file name is not a valid <c>{name}.json</c> with a valid name.
    /// </summary>
    private static string? ExtractNameFromFilename(string fileName)
    {
        if (!fileName.EndsWith(
            FilenameSuffix,
            StringComparison.OrdinalIgnoreCase))
            return null;

        var name = fileName[..^FilenameSuffix.Length];
        return IsValidName(name) ? name : null;
    }

    /// <summary>
    /// Validates a record and its name prior to a write.
    /// </summary>
    /// <exception cref="ArgumentNullException">The record is <c>null</c>.</exception>
    /// <exception cref="ArgumentException">The name fails validation.</exception>
    private static void ValidateRecord(CharacterRecord record)
    {
        ArgumentNullException.ThrowIfNull(record);
        ValidateName(record.CharacterName);
    }

    /// <summary>
    /// Throws if a character name is empty, out of the 3-14 length range, or
    /// contains anything other than lowercase letters.
    /// </summary>
    /// <exception cref="ArgumentException">The name fails any rule.</exception>
    private static void ValidateName(string characterName)
    {
        if (string.IsNullOrEmpty(characterName))
            throw new ArgumentException(
                "Character name must not be empty.",
                nameof(characterName));

        if (characterName.Length < MinNameLength
            || characterName.Length > MaxNameLength)
            throw new ArgumentException(
                $"Character name length must be {MinNameLength}"
                    + $"-{MaxNameLength}.",
                nameof(characterName));

        if (!NameCharsetPattern().IsMatch(characterName))
            throw new ArgumentException(
                "Character name must contain only lowercase letters.",
                nameof(characterName));
    }

    /// <summary>
    /// Non-throwing name check used by the boot scan and file-name parsing.
    /// </summary>
    /// <returns><c>true</c> if the name satisfies every rule.</returns>
    private static bool IsValidName(string characterName)
        => !string.IsNullOrEmpty(characterName)
            && characterName.Length >= MinNameLength
            && characterName.Length <= MaxNameLength
            && NameCharsetPattern().IsMatch(characterName);
}

/*
 *------------------------------------------------------------
 * (CharacterStore.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */