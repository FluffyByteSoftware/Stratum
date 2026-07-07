/*
 * (DiskManager.cs)
 *------------------------------------------------------------
 * Created - 5/10/2026 4:30:00 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Reflection;
using System.Text;

namespace SystemTools.Storage;

/// <summary>
/// The single point of contact between the game engine and the filesystem. Buffers writes in memory and
/// flushes to disk on a periodic timer or when the cache exceeds its size budget. Reads check the cache
/// first for unflushed writes, falling back to disk on a miss. Also receives formatted log lines via
/// <see cref="Log"/> and accumulates them in per-<see cref="LogFile"/> buffers for daily-rolling log files.
/// </summary>
/// <remarks>
/// <para>Accessed globally via <see cref="Instance"/> after <see cref="Initialize"/> has been called.
/// All disk writes are atomic (write to temp file, fsync, rename). On shutdown, callers should invoke
/// <see cref="ShutdownAsync"/> from a <c>finally</c> block to drain the cache and log buffers before
/// process exit.</para>
/// <para><b>Failure surfacing.</b> Because writes are write-back cached, a disk failure happens after
/// the write call has already returned success. Three mechanisms make that observable: (1) each flush
/// cycle returns a <see cref="FlushReport"/> (via <see cref="FlushAsync"/>) naming every path that
/// failed, for callers that force a flush and need write-through certainty; (2) failed entries are
/// re-queued into the cache (unless a newer write to the same path has superseded them) and retry on
/// the normal flush cadence, so transient failures self-heal; (3) <see cref="HasPersistFailures"/>
/// goes true — permanently, for the life of the process — on the first failure, with
/// <see cref="GetPersistFailures"/> exposing the currently-unresolved paths. On top of that, the
/// first time a given entry fails it is emergency-dumped to <c>recovery/</c> (paired
/// <c>.bin</c>/<c>.txt</c>, see <see cref="DumpRecovery"/>) so the data survives even a process
/// crash while retries are still failing.</para>
/// <para><b>DiskManager never logs through Scribe.</b> Scribe's output path runs through this class
/// (<see cref="Log"/>), so reporting a DiskManager failure via Scribe would route the report back
/// into the failing component — a recursion trap on exactly the path that is broken.
/// <see cref="Console.Error"/> is the deliberate last-resort channel here; structured observability
/// comes from the report, the sticky flag, and the recovery dumps, all consumed by callers that
/// <i>can</i> log safely.</para>
/// </remarks>
public sealed class DiskManager
{
    private const long MaxCacheBytes = 50L * 1024 * 1024;
    private const int FlushIntervalMs = 2000;
    private const string DateToken = "{date}";
    private const string ProcToken = "{proc}";
    private const string RecoveryDirName = "recovery";

    private static volatile DiskManager? _instance;
    private static volatile bool _initialized;
    private static readonly Lock _initLock = new();

    /// <summary>
    /// The process-wide DiskManager instance. Throws if accessed before <see cref="Initialize"/>.
    /// </summary>
    public static DiskManager Instance =>
        _instance ?? throw new InvalidOperationException(
            "DiskManager not initialized.");

    /// <summary>
    /// True if <see cref="Initialize"/> has been called successfully and the manager is ready for use.
    /// Scribe checks this before forwarding log lines to avoid races during startup.
    /// </summary>
    public static bool IsRunning => _initialized && _instance is not null;

    private readonly string _rootPath;

    private readonly Dictionary<string, DiskEntry> _cache = [];
    private readonly Lock _cacheLock = new();
    private long _cacheBytes;

    private readonly Dictionary<string, string> _persistFailures = [];
    private readonly Lock _failureLock = new();
    private volatile bool _hasPersistFailures;

    private readonly LogSink[] _logSinks;
    private readonly Lock _logLock = new();

    private readonly CancellationTokenSource _cts = new();
    private readonly Task _flushTask;
    private readonly SemaphoreSlim _flushSignal = new(0, 1);

    /// <summary>
    /// The absolute root path against which all relative paths passed to read, write, and delete
    /// methods are resolved. Set at <see cref="Initialize"/> and immutable thereafter.
    /// </summary>
    public string RootPath => _rootPath;

    /// <summary>
    /// True if any flush this process has ever failed to persist an entry or log buffer. Sticky for
    /// the life of the process: it never resets, even after the failed entries later reach disk.
    /// </summary>
    /// <remarks>
    /// Sticky on purpose. A health check that went quietly green after a failure would hide that an
    /// emergency dump may exist under <c>recovery/</c> and that data spent time at risk. This flag
    /// answers "has anything ever gone wrong" (go look at <c>recovery/</c> and the console);
    /// <see cref="GetPersistFailures"/> answers the separate question "is anything <i>still</i>
    /// failing right now."
    /// </remarks>
    public bool HasPersistFailures => _hasPersistFailures;

    /// <summary>
    /// Initializes the process-wide DiskManager. Parameterless — the root directory is sourced from
    /// <see cref="Constellations.DataRoot"/>, the single home of the data-root literal, and created
    /// if missing. Subsequent calls are silently ignored. Log destinations are fixed per
    /// <see cref="LogFile"/>; each rolls over automatically at UTC midnight.
    /// </summary>
    public static void Initialize()
    {
        var rootPath = Constellations.DataRoot;

        if (_initialized) return;

        lock (_initLock)
        {
            if (_initialized) return;

            _instance = new DiskManager(rootPath);
            _initialized = true;
        }
    }

    private DiskManager(string rootPath)
    {
        _rootPath = Path.GetFullPath(rootPath);
        Directory.CreateDirectory(_rootPath);

        var today = DateTime.UtcNow.Date;
        var procTag = ResolveProcessTag();
        _logSinks = new LogSink[LogTemplates.Length];
        for (int i = 0; i < LogTemplates.Length; i++)
        {
            var template = LogTemplates[i].Replace(ProcToken, procTag);
            var sink = new LogSink(template);
            sink.Roll(today, _rootPath);
            EnsureDirectoryFor(sink.CurrentPath);
            _logSinks[i] = sink;
        }

        _flushTask = Task.Run(FlushLoop);
    }

    private static readonly string[] LogTemplates =
    [
        "logs/server_{proc}_{date}.log",      // LogFile.Server
        "logs/admin_{proc}_{date}.log",       // LogFile.Admin
        "logs/simulation_{proc}_{date}.log"   // LogFile.Simulation
    ];

    /// <summary>
    /// Writes a UTF-8 encoded text file to the cache. The actual disk write occurs on the next flush
    /// (timer-driven or size-triggered). Subsequent reads of the same path return the cached contents.
    /// </summary>
    /// <param name="relativePath">The path relative to the configured root.</param>
    /// <param name="contents">The text contents to persist.</param>
    public void WriteTextFile(string relativePath, string contents)
    {
        var bytes = new UTF8Encoding(false).GetBytes(contents);
        WriteBinFile(relativePath, bytes);
    }

    /// <summary>
    /// Writes a binary file to the cache. The actual disk write occurs on the next flush (timer-driven
    /// or size-triggered). Subsequent reads of the same path return the cached bytes. A caller that
    /// must know the bytes reached disk forces a flush via <see cref="FlushAsync"/> and inspects the
    /// returned <see cref="FlushReport"/>.
    /// </summary>
    /// <param name="relativePath">The path relative to the configured root.</param>
    /// <param name="data">The byte payload to persist.</param>
    public void WriteBinFile(string relativePath, byte[] data)
    {
        bool shouldSignalFlush;
        lock (_cacheLock)
        {
            if (_cache.TryGetValue(relativePath, out var existing))
                _cacheBytes -= existing.Data.LongLength;

            _cache[relativePath] = new DiskEntry(relativePath, data);
            _cacheBytes += data.LongLength;
            shouldSignalFlush = _cacheBytes >= MaxCacheBytes;
        }

        if (shouldSignalFlush)
            SignalFlush();
    }

    /// <summary>
    /// Reads a UTF-8 text file. Returns cached contents if a pending write exists; otherwise reads from disk.
    /// </summary>
    /// <param name="relativePath">The path relative to the configured root.</param>
    /// <exception cref="FileNotFoundException">Thrown if the file is neither in the cache nor on disk.</exception>
    public string ReadTextFile(string relativePath)
    {
        var bytes = ReadBinFile(relativePath);
        return Encoding.UTF8.GetString(bytes);
    }

    /// <summary>
    /// Reads a binary file. Returns cached bytes if a pending write exists; otherwise reads from disk.
    /// </summary>
    /// <param name="relativePath">The path relative to the configured root.</param>
    /// <exception cref="FileNotFoundException">Thrown if the file is neither in the cache nor on disk.</exception>
    public byte[] ReadBinFile(string relativePath)
    {
        lock (_cacheLock)
        {
            if (_cache.TryGetValue(relativePath, out var entry))
                return entry.Data;
        }

        var fullPath = Path.Combine(_rootPath, relativePath);

        return File.ReadAllBytes(fullPath);
    }

    /// <summary>
    /// Returns <see langword="true"/> if the path resolves to either a pending cache entry or an
    /// existing file on disk. Reflects the same view of the world as <see cref="ReadBinFile"/>.
    /// </summary>
    /// <param name="relativePath">The path relative to the configured root.</param>
    public bool FileExists(string relativePath)
    {
        lock (_cacheLock)
        {
            if (_cache.ContainsKey(relativePath))
                return true;
        }

        var fullPath = Path.Combine(_rootPath, relativePath);
        return File.Exists(fullPath);
    }

    /// <summary>
    /// Removes a file from both the cache (if a write is pending) and from disk (if it exists).
    /// Returns <see langword="true"/> if either was removed. Subsequent reads of the same path will
    /// throw <see cref="FileNotFoundException"/> unless the file is rewritten.
    /// </summary>
    /// <param name="relativePath">The path relative to the configured root.</param>
    public bool DeleteFile(string relativePath)
    {
        bool removedFromCache;
        lock (_cacheLock)
        {
            removedFromCache = _cache.Remove(
                relativePath,
                out var existing);
            if (removedFromCache)
                _cacheBytes -= existing.Data.LongLength;
        }

        var fullPath = Path.Combine(_rootPath, relativePath);
        var removedFromDisk = false;
        if (File.Exists(fullPath))
        {
            File.Delete(fullPath);
            removedFromDisk = true;
        }

        return removedFromCache || removedFromDisk;
    }

    /// <summary>
    /// Appends a formatted log line to the in-memory buffer for the given <paramref name="target"/>.
    /// Called by <c>Scribe</c> (for <see cref="LogFile.Server"/>) and by audit/simulation callers. The
    /// buffer is flushed to the active dated log file on the same flush cadence as the main cache.
    /// </summary>
    /// <param name="target">The destination log file.</param>
    /// <param name="line">The formatted log line to append, without a trailing newline.</param>
    public void Log(LogFile target, string line)
    {
        lock (_logLock)
        {
            _logSinks[(int)target].Buffer.AppendLine(line);
        }
    }

    /// <summary>
    /// Forces an immediate flush of the cache and log buffers to disk, bypassing the timer cadence,
    /// and returns a <see cref="FlushReport"/> naming every path that failed to persist this cycle.
    /// Safe to call concurrently with the background flush loop; entries are snapshotted and cleared
    /// under lock, so simultaneous flushes do not double-write.
    /// </summary>
    /// <returns>The outcome of this flush cycle. <see cref="FlushReport.AllSucceeded"/> means every
    /// pending entry and log buffer reached disk.</returns>
    /// <remarks>
    /// This is the write-through escape hatch from the write-back cache: write, flush, check the
    /// report. Widening the return from <c>Task</c> to <c>Task&lt;FlushReport&gt;</c> is
    /// non-breaking — existing callers that awaited the bare <c>Task</c> compile untouched.
    /// </remarks>
    public Task<FlushReport> FlushAsync()
    {
        return Task.Run(FlushOnce);
    }

    /// <summary>
    /// Returns a snapshot of every path whose most recent flush attempt failed and which has not
    /// since been written clean. Empty when nothing is currently failing — unlike
    /// <see cref="HasPersistFailures"/>, this view heals as retries succeed.
    /// </summary>
    public IReadOnlyList<FlushFailure> GetPersistFailures()
    {
        lock (_failureLock)
        {
            if (_persistFailures.Count == 0)
                return [];

            var snapshot = new FlushFailure[_persistFailures.Count];
            int i = 0;
            foreach (var kvp in _persistFailures)
                snapshot[i++] = new FlushFailure(kvp.Key, kvp.Value);
            return snapshot;
        }
    }

    /// <summary>
    /// Flushes all pending writes and the log buffers to disk, then stops the flush loop. Idempotent on
    /// subsequent calls. Should be invoked from a <c>finally</c> block around the server lifetime, after
    /// <c>Scribe.ShutdownAsync</c> so any in-flight log messages land in the buffers first.
    /// </summary>
    public async Task ShutdownAsync()
    {
        if (!_initialized) return;
        _initialized = false;

        _cts.Cancel();
        SignalFlush();

        try { await _flushTask.ConfigureAwait(false); }
        catch (OperationCanceledException) { }

        FlushOnce();
        _cts.Dispose();
        _flushSignal.Dispose();

        // Drop the static handle now that this instance's primitives 
        // are disposed.
        lock (_initLock)
        {
            _instance = null;
        }
    }

    private void SignalFlush()
    {
        try { _flushSignal.Release(); }
        catch (SemaphoreFullException) { }
    }

    private async Task FlushLoop()
    {
        var token = _cts.Token;
        while (!token.IsCancellationRequested)
        {
            try
            {
                using var timeoutCts =
                    CancellationTokenSource.CreateLinkedTokenSource(token);
                timeoutCts.CancelAfter(FlushIntervalMs);
                try
                {
                    await _flushSignal
                        .WaitAsync(timeoutCts.Token)
                        .ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                    when (!token.IsCancellationRequested)
                {
                    // Timer-driven flush; not a real cancellation.
                }
            }
            catch (OperationCanceledException) { break; }

            if (token.IsCancellationRequested) break;
            FlushOnce();
        }
    }

    private FlushReport FlushOnce()
    {
        var cleanPaths = new List<string>();
        var failedEntries = new List<(DiskEntry Entry, string Message)>();
        var logFailures = new List<FlushFailure>();

        FlushCache(cleanPaths, failedEntries);
        FlushLogBuffers(cleanPaths, logFailures);

        return ReconcileFailureState(cleanPaths, failedEntries, logFailures);
    }

    private void FlushCache(
        List<string> cleanPaths,
        List<(DiskEntry Entry, string Message)> failedEntries)
    {
        DiskEntry[] toWrite;
        lock (_cacheLock)
        {
            if (_cache.Count == 0) return;
            toWrite = new DiskEntry[_cache.Count];
            _cache.Values.CopyTo(toWrite, 0);
            _cache.Clear();
            _cacheBytes = 0;
        }

        List<DiskEntry>? toRequeue = null;

        for (int i = 0; i < toWrite.Length; i++)
        {
            try
            {
                WriteAtomic(toWrite[i]);
                cleanPaths.Add(toWrite[i].Path);
            }
            catch (Exception ex)
            {
                failedEntries.Add((toWrite[i], ex.Message));
                (toRequeue ??= []).Add(toWrite[i]);

                Console.Error.WriteLine(
                    $"[DiskManager] Flush failed for "
                        + $"'{toWrite[i].Path}': {ex.Message}");
            }
        }

        if (toRequeue is null) return;

        // Re-queue failed entries so they retry on the next flush cycle.
        // TryAdd is the no-clobber guard: if a newer write to the same path
        // landed while this flush was running, the newer data owns the key
        // and the stale failed bytes are dropped. No flush is signalled
        // here — the retry rides the normal timer rather than spinning a
        // hot loop against a disk that is currently refusing writes.
        lock (_cacheLock)
        {
            for (int i = 0; i < toRequeue.Count; i++)
            {
                if (_cache.TryAdd(toRequeue[i].Path, toRequeue[i]))
                    _cacheBytes += toRequeue[i].Data.LongLength;
            }
        }
    }

    private void FlushLogBuffers(
        List<string> cleanPaths,
        List<FlushFailure> logFailures)
    {
        var today = DateTime.UtcNow.Date;
        for (int i = 0; i < _logSinks.Length; i++)
        {
            string toWrite;
            string path;
            lock (_logLock)
            {
                var sink = _logSinks[i];
                if (sink.Buffer.Length == 0) continue;

                if (today != sink.CurrentDateUtc)
                {
                    sink.Roll(today, _rootPath);
                    EnsureDirectoryFor(sink.CurrentPath);
                }

                toWrite = sink.Buffer.ToString();
                sink.Buffer.Clear();
                path = sink.CurrentPath;
            }

            var relativePath = Path.GetRelativePath(_rootPath, path);

            try
            {
                File.AppendAllText(
                    path,
                    toWrite,
                    new UTF8Encoding(false));
                cleanPaths.Add(relativePath);
            }
            catch (Exception ex)
            {
                // Re-insert at the front so the lines retry next cycle in
                // their original order, ahead of anything buffered since.
                lock (_logLock)
                {
                    _logSinks[i].Buffer.Insert(0, toWrite);
                }

                logFailures.Add(new FlushFailure(relativePath, ex.Message));

                Console.Error.WriteLine(
                    $"[DiskManager] Log append failed for "
                        + $"'{path}': {ex.Message}");
            }
        }
    }

    private FlushReport ReconcileFailureState(
        List<string> cleanPaths,
        List<(DiskEntry Entry, string Message)> failedEntries,
        List<FlushFailure> logFailures)
    {
        List<(DiskEntry Entry, string Message)>? toDump = null;

        lock (_failureLock)
        {
            // Paths that wrote clean this cycle are no longer failing.
            for (int i = 0; i < cleanPaths.Count; i++)
                _persistFailures.Remove(cleanPaths[i]);

            // A cache entry is dumped only on its first failure: presence in
            // _persistFailures means it was already dumped and is on the
            // retry path, so a 2-second flush cadence cannot spam a new
            // recovery pair per cycle. An entry that heals and later fails
            // again is a new incident and dumps again.
            for (int i = 0; i < failedEntries.Count; i++)
            {
                var (entry, message) = failedEntries[i];
                if (!_persistFailures.ContainsKey(entry.Path))
                    (toDump ??= []).Add((entry, message));
                _persistFailures[entry.Path] = message;
            }

            // Log failures are recorded but never dumped — their text is
            // re-queued into the sink buffer, not lost, and log lines are
            // not recovery-grade payloads.
            for (int i = 0; i < logFailures.Count; i++)
            {
                _persistFailures[logFailures[i].RelativePath] =
                    logFailures[i].Message;
            }

            if (failedEntries.Count > 0 || logFailures.Count > 0)
                _hasPersistFailures = true;
        }

        if (toDump is not null)
            DumpRecovery(toDump);

        if (failedEntries.Count == 0 && logFailures.Count == 0)
            return FlushReport.Clean;

        var failures =
            new FlushFailure[failedEntries.Count + logFailures.Count];
        int n = 0;
        for (int i = 0; i < failedEntries.Count; i++)
        {
            failures[n++] = new FlushFailure(
                failedEntries[i].Entry.Path,
                failedEntries[i].Message);
        }
        for (int i = 0; i < logFailures.Count; i++)
            failures[n++] = logFailures[i];

        return new FlushReport(failures);
    }

    /// <summary>
    /// Best-effort emergency dump of newly-failed cache entries to the
    /// <c>recovery/</c> directory under the data root, as a paired
    /// <c>{utcstamp}_flush.bin</c> / <c>{utcstamp}_flush.txt</c> sharing one
    /// timestamp so the pair is self-evidently linked.
    /// </summary>
    /// <remarks>
    /// The <c>.bin</c> is the recoverable payload: for each entry,
    /// <c>[pathByteCount: Int32 LE][path: UTF-8][dataByteCount: Int32 LE][data]</c>,
    /// repeated. The <c>.txt</c> is the human-facing sidecar — intended path,
    /// byte count, and exception message per entry — readable without a tool.
    /// Writes here are deliberately primitive (direct create, no temp-rename
    /// atomics): the atomic write path is the thing under suspicion when this
    /// runs, so the emergency path shares as little machinery with it as
    /// possible. If the failure is disk-wide (full disk) this dump fails too;
    /// that is accepted — the entries remain re-queued in the cache, and
    /// <see cref="Console.Error"/> is the true last resort.
    /// </remarks>
    private void DumpRecovery(
        List<(DiskEntry Entry, string Message)> newlyFailed)
    {
        try
        {
            var stamp = DateTime.UtcNow.ToString("yyyyMMdd_HHmmssfff");
            var dir = Path.Combine(_rootPath, RecoveryDirName);
            Directory.CreateDirectory(dir);

            var binPath = Path.Combine(dir, stamp + "_flush.bin");
            var txtPath = Path.Combine(dir, stamp + "_flush.txt");

            using (var fs = new FileStream(
                binPath,
                FileMode.CreateNew,
                FileAccess.Write,
                FileShare.None))
            using (var bw = new BinaryWriter(fs))
            {
                for (int i = 0; i < newlyFailed.Count; i++)
                {
                    var pathBytes = Encoding.UTF8.GetBytes(
                        newlyFailed[i].Entry.Path);
                    bw.Write(pathBytes.Length);
                    bw.Write(pathBytes);
                    bw.Write(newlyFailed[i].Entry.Data.Length);
                    bw.Write(newlyFailed[i].Entry.Data);
                }
                fs.Flush(true);
            }

            var sb = new StringBuilder();
            sb.AppendLine(
                $"DiskManager emergency dump {stamp} (UTC)");
            sb.AppendLine($"Entries: {newlyFailed.Count}");
            sb.AppendLine(
                "Payload bytes are in the paired .bin: "
                + "[pathLen:i32][path:utf8][dataLen:i32][data] repeated.");
            sb.AppendLine();
            for (int i = 0; i < newlyFailed.Count; i++)
            {
                sb.AppendLine($"Path:  {newlyFailed[i].Entry.Path}");
                sb.AppendLine(
                    $"Bytes: {newlyFailed[i].Entry.Data.Length}");
                sb.AppendLine($"Error: {newlyFailed[i].Message}");
                sb.AppendLine();
            }
            File.WriteAllText(txtPath, sb.ToString(), new UTF8Encoding(false));

            Console.Error.WriteLine(
                $"[DiskManager] Emergency dump written to '{binPath}' "
                    + "(paired .txt alongside).");
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine(
                $"[DiskManager] Emergency dump FAILED: {ex.Message}. "
                    + "Failed entries remain re-queued in the cache.");
        }
    }

    private static void EnsureDirectoryFor(string fullPath)
    {
        var dir = Path.GetDirectoryName(fullPath);
        if (!string.IsNullOrEmpty(dir))
            Directory.CreateDirectory(dir);
    }

    private void WriteAtomic(in DiskEntry entry)
    {
        var fullPath = Path.Combine(_rootPath, entry.Path);
        EnsureDirectoryFor(fullPath);

        var tmpPath = fullPath + ".tmp";
        using (var fs = new FileStream(
            tmpPath,
            FileMode.Create,
            FileAccess.Write,
            FileShare.None))
        {
            fs.Write(entry.Data, 0, entry.Data.Length);
            fs.Flush(true);
        }

        if (File.Exists(fullPath))
            File.Replace(tmpPath, fullPath, null);
        else
            File.Move(tmpPath, fullPath);
    }

    private static string ResolveProcessTag()
    {
        var name = Assembly.GetEntryAssembly()?.GetName().Name;
        return string.IsNullOrEmpty(name)
            ? Environment.ProcessId.ToString()
            : name.ToLowerInvariant();
    }

    private sealed class LogSink(string template)
    {

        public StringBuilder Buffer { get; } = new();
        public DateTime CurrentDateUtc { get; private set; }
        public string CurrentPath { get; private set; } = "";

        public void Roll(DateTime utcDate, string rootPath)
        {
            CurrentDateUtc = utcDate;
            var dateStr = utcDate.ToString("yyyy-MM-dd");
            var relative = template.Replace(DateToken, dateStr);
            CurrentPath = Path.Combine(rootPath, relative);
        }
    }
}

/*
 *------------------------------------------------------------
 * (DiskManager.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */