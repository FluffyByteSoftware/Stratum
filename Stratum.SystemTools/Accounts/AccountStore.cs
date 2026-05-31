/*
 * (AccountStore.cs)
 *------------------------------------------------------------
 * Created - 5/21/2026 3:29:12 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */
using System.Collections.Concurrent;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using Stratum.SystemTools.Logger;
using Stratum.SystemTools.Storage;

namespace Stratum.SystemTools.Accounts;

/// <summary>
/// Manages account records with thread-safe in-memory storage and JSON file-based persistence.
/// </summary>
/// <remarks>This class uses a singleton pattern and must be initialized via 
/// <see cref="Initialize"/> before accessing the <see cref="Instance"/>. Account IDs must 
/// be 3-24 characters long and contain only lowercase letters.
/// All operations are thread-safe using concurrent collections.</remarks>
public sealed partial class AccountStore
{
    private const string AccountsRelativeDir = "accounts";
    private const string FilenameSuffix = "_account.json";
    private const string SearchPattern = "*_account.json";
    private const int MinIdLength = 3;
    private const int MaxIdLength = 24;

    [GeneratedRegex("^[a-z]+$")]
    private static partial Regex IdCharsetPattern();

    private static AccountStore? _instance;
    private static bool _initialized;

    /// <summary>
    /// The singleton instance of the AccountStore.
    /// </summary>
    public static AccountStore Instance =>
        _instance
        ?? throw new InvalidOperationException(
            "AccountStore not initialized.");

    /// <summary>
    /// Gets a value indicating whether the instance is initialized and running.
    /// </summary>
    public static bool IsRunning => _initialized && _instance is not null;

    private readonly ConcurrentDictionary<string, AccountRecord> _records
        = new(StringComparer.OrdinalIgnoreCase);

    private readonly JsonSerializerOptions _jsonOptions = new()
    {
        WriteIndented = true,
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.Never
    };

    /// <summary>
    /// Initializes the AccountStore singleton instance and performs an initial boot scan.
    /// </summary>
    public static void Initialize()
    {
        if (_initialized) return;
        _instance = new AccountStore();
        _initialized = true;
        _instance.BootScan();
    }

    private AccountStore() { }

    /// <summary>
    /// Attempts to retrieve an account record by its identifier.
    /// </summary>
    /// <param name="accountId">The account identifier to search for.</param>
    /// <param name="record">When this method returns, contains the account record if found; 
    /// otherwise, <c>null</c>.</param>
    /// <returns><c>true</c> if the account record was found; otherwise, <c>false</c>.</returns>
    public bool TryGet(
        string accountId,
        out AccountRecord record)
    {
        if (string.IsNullOrEmpty(accountId))
        {
            record = null!;
            return false;
        }

        if (_records.TryGetValue(accountId, out var found))
        {
            record = found;
            return true;
        }

        record = null!;
        return false;
    }

    /// <summary>
    /// Gets all record IDs in sorted order.
    /// </summary>
    /// <returns>A read-only list of record IDs sorted case-insensitively.</returns>
    public IReadOnlyList<string> ListIds()
    {
        var ids = new List<string>(_records.Count);
        foreach (var kv in _records)
            ids.Add(kv.Value.Id);
        ids.Sort(StringComparer.OrdinalIgnoreCase);
        return ids;
    }

    /// <summary>
    /// Adds an account record to the collection.
    /// </summary>
    /// <param name="record">The account record to add.</param>
    /// <exception cref="InvalidOperationException">Thrown when an account with the 
    /// specified identifier already exists.</exception>
    public void Add(AccountRecord record)
    {
        ValidateRecord(record);

        if (!_records.TryAdd(record.Id, record))
            throw new InvalidOperationException(
                $"Account '{record.Id}' already exists.");

        try
        {
            PersistRecord(record);
        }
        catch
        {
            _records.TryRemove(record.Id, out _);
            throw;
        }
    }

    /// <summary>
    /// Updates an existing account record.
    /// </summary>
    /// <param name="record">The account record to update.</param>
    /// <exception cref="InvalidOperationException">The account does not 
    /// exist or was modified concurrently.</exception>
    public void Update(AccountRecord record)
    {
        ValidateRecord(record);

        if (!_records.TryGetValue(record.Id, out var existing))
            throw new InvalidOperationException(
                $"Account '{record.Id}' does not exist.");

        if (!_records.TryUpdate(record.Id, record, existing))
            throw new InvalidOperationException(
                $"Account '{record.Id}' was modified concurrently.");

        try
        {
            PersistRecord(record);
        }
        catch
        {
            _records.TryUpdate(record.Id, existing, record);
            throw;
        }
    }

    /// <summary>
    /// Removes the account record and deletes its associated file from disk.
    /// </summary>
    /// <param name="accountId">The unique identifier of the account to remove.</param>
    /// <exception cref="InvalidOperationException">Thrown when the account with 
    /// the specified identifier does not exist.</exception>
    public void Remove(string accountId)
    {
        ValidateId(accountId);

        if (!_records.TryRemove(accountId, out var removed))
            throw new InvalidOperationException(
                $"Account '{accountId}' does not exist.");

        try
        {
            DiskManager.Instance.DeleteFile(BuildRelativePath(accountId));
        }
        catch
        {
            _records.TryAdd(accountId, removed);
            throw;
        }
    }

    /// <summary>
    /// Shuts down the service and clears all internal state.
    /// </summary>
    /// <returns>A task that represents the asynchronous shutdown operation.</returns>
    public Task ShutdownAsync()
    {
        if (!_initialized) return Task.CompletedTask;
        _initialized = false;
        _records.Clear();
        return Task.CompletedTask;
    }

    private void BootScan()
    {
        var disk = DiskManager.Instance;
        var accountsRoot = Path.Combine(
            disk.RootPath,
            AccountsRelativeDir);

        if (!Directory.Exists(accountsRoot))
        {
            Directory.CreateDirectory(accountsRoot);
            return;
        }

        var files = Directory.EnumerateFiles(
            accountsRoot,
            SearchPattern,
            SearchOption.TopDirectoryOnly);

        foreach (var fullPath in files)
        {
            var fileName = Path.GetFileName(fullPath);
            var id = ExtractIdFromFilename(fileName);
            if (id is null)
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Skipping unrecognized account file: '{fileName}'."));
                continue;
            }

            var relativePath = $"{AccountsRelativeDir}/{fileName}";
            AccountRecord? record;
            try
            {
                var json = disk.ReadTextFile(relativePath);
                record = JsonSerializer.Deserialize<AccountRecord>(
                    json,
                    _jsonOptions);
            }
            catch (Exception ex)
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Failed to load account file '{fileName}'.",
                    ex));
                continue;
            }

            if (record is null
                || string.IsNullOrEmpty(record.Id)
                || !IsValidId(record.Id)
                || !string.Equals(
                    record.Id,
                    id,
                    StringComparison.OrdinalIgnoreCase))
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Account file '{fileName}' has invalid or "
                        + "mismatched record; skipping."));
                continue;
            }

            if (!_records.TryAdd(record.Id, record))
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Duplicate account id '{record.Id}' in "
                        + $"'{fileName}'; skipping."));
            }
        }
    }

    private void PersistRecord(AccountRecord record)
    {
        var json = JsonSerializer.Serialize(record, _jsonOptions);
        var relativePath = BuildRelativePath(record.Id);
        DiskManager.Instance.WriteTextFile(relativePath, json);
    }

    private static string BuildRelativePath(string accountId)
        => $"{AccountsRelativeDir}/{BuildFileName(accountId)}";

    private static string BuildFileName(string accountId)
        => accountId.ToLowerInvariant() + FilenameSuffix;

    private static string? ExtractIdFromFilename(string fileName)
    {
        if (!fileName.EndsWith(
            FilenameSuffix,
            StringComparison.OrdinalIgnoreCase))
            return null;

        var id = fileName[..^FilenameSuffix.Length];
        return IsValidId(id) ? id : null;
    }

    private static void ValidateRecord(AccountRecord record)
    {
        ArgumentNullException.ThrowIfNull(record);
        ValidateId(record.Id);
    }

    private static void ValidateId(string accountId)
    {
        if (string.IsNullOrEmpty(accountId))
            throw new ArgumentException(
                "Account id must not be empty.",
                nameof(accountId));

        if (accountId.Length < MinIdLength
            || accountId.Length > MaxIdLength)
            throw new ArgumentException(
                $"Account id length must be {MinIdLength}"
                    + $"-{MaxIdLength}.",
                nameof(accountId));

        if (!IdCharsetPattern().IsMatch(accountId))
            throw new ArgumentException(
                "Account id must contain only lowercase letters.",
                nameof(accountId));
    }

    private static bool IsValidId(string accountId)
        => !string.IsNullOrEmpty(accountId)
            && accountId.Length >= MinIdLength
            && accountId.Length <= MaxIdLength
            && IdCharsetPattern().IsMatch(accountId);
}

/*
 *------------------------------------------------------------
 * (AccountStore.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */