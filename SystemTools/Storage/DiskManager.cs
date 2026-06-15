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
/// <remarks>Accessed globally via <see cref="Instance"/> after <see cref="Initialize"/> has been called.
/// All disk writes are atomic (write to temp file, fsync, rename). On shutdown, callers should invoke
/// <see cref="ShutdownAsync"/> from a <c>finally</c> block to drain the cache and log buffers before
/// process exit.</remarks>
public sealed class DiskManager
{
    private const long MaxCacheBytes = 50L * 1024 * 1024;
    private const int FlushIntervalMs = 2000;
    private const string DateToken = "{date}";
    private const string ProcToken = "{proc}";

    private static DiskManager? _instance;
    private static bool _initialized;

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
    /// Initializes the process-wide DiskManager. Subsequent calls are silently ignored. Log
    /// destinations are fixed per <see cref="LogFile"/>; each rolls over automatically at UTC midnight.
    /// </summary>
    /// <param name="rootPath">The root directory all relative paths are resolved against. Created if missing.</param>
    public static void Initialize()
    {
        var rootPath = Constellations.DataRoot;

        if (_initialized) return;
        _instance = new DiskManager(rootPath);
        _initialized = true;
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
    /// or size-triggered). Subsequent reads of the same path return the cached bytes.
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
    /// Forces an immediate flush of the cache and log buffers to disk, bypassing the timer cadence.
    /// Safe to call concurrently with the background flush loop; entries are snapshotted and cleared
    /// under lock, so simultaneous flushes do not double-write.
    /// </summary>
    public Task FlushAsync()
    {
        return Task.Run(FlushOnce);
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

    private void FlushOnce()
    {
        FlushCache();
        FlushLogBuffers();
    }

    private void FlushCache()
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

        for (int i = 0; i < toWrite.Length; i++)
        {
            try
            {
                WriteAtomic(toWrite[i]);
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine(
                    $"[DiskManager] Flush failed for "
                        + $"'{toWrite[i].Path}': {ex.Message}");
            }
        }
    }

    private void FlushLogBuffers()
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

            try
            {
                File.AppendAllText(
                    path,
                    toWrite,
                    new UTF8Encoding(false));
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine(
                    $"[DiskManager] Log append failed for "
                        + $"'{path}': {ex.Message}");
            }
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