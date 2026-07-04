/*
 * (FlushReport.cs)
 *------------------------------------------------------------
 * Created - 7/4/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace SystemTools.Storage;

/// <summary>
/// A single failed disk write from one flush cycle: the root-relative path
/// the data was destined for, and the exception message that stopped it.
/// </summary>
/// <param name="relativePath">The path, relative to the DiskManager root,
/// that the entry failed to persist to.</param>
/// <param name="message">The exception message from the failed write
/// attempt.</param>
public readonly struct FlushFailure(string relativePath, string message)
{
    /// <summary>
    /// The root-relative path the entry failed to persist to. Matches the
    /// path originally passed to the write call, byte for byte.
    /// </summary>
    public string RelativePath { get; } = relativePath;

    /// <summary>
    /// The exception message from the failed write attempt. Message only —
    /// the full exception is not retained, because the report may outlive
    /// the flush cycle and a held exception would pin its stack and any
    /// captured state with it.
    /// </summary>
    public string Message { get; } = message;
}

/// <summary>
/// The outcome of one flush cycle, returned by
/// <see cref="DiskManager.FlushAsync"/>. An empty failure list means every
/// pending entry and log buffer reached disk.
/// </summary>
/// <remarks>
/// This type exists because <see cref="DiskManager.WriteBinFile"/> is
/// write-back cached: the write call returns before any disk I/O is
/// attempted, so a caller cannot learn about a persist failure through the
/// write call itself. The report is the synchronous half of the answer — a
/// caller that needs write-through semantics (e.g. a key provider that must
/// not report success over an unpersisted keypair) forces a flush and
/// inspects the result. The asynchronous half — failures discovered by the
/// background timer flush, long after any call site has moved on — is
/// covered by <see cref="DiskManager.HasPersistFailures"/> and the sticky
/// failure record behind it. Construction is <see langword="internal"/>:
/// only DiskManager mints reports, so a non-clean report always describes a
/// real flush attempt.
/// </remarks>
public sealed class FlushReport
{
    /// <summary>
    /// The shared all-clear report. Returned for flush cycles with nothing
    /// pending as well as fully successful ones — the distinction carries no
    /// information a caller can act on, and sharing one instance keeps the
    /// common path allocation-free.
    /// </summary>
    public static FlushReport Clean { get; } = new([]);

    private readonly FlushFailure[] _failures;

    /// <summary>
    /// Every write that failed during this flush cycle. Cache entries and
    /// log-buffer appends both appear here, keyed by their root-relative
    /// paths.
    /// </summary>
    public IReadOnlyList<FlushFailure> Failures => _failures;

    /// <summary>
    /// True if every pending entry and log buffer reached disk this cycle.
    /// </summary>
    public bool AllSucceeded => _failures.Length == 0;

    internal FlushReport(FlushFailure[] failures)
    {
        _failures = failures;
    }

    /// <summary>
    /// Returns <see langword="true"/> if the given root-relative path failed
    /// to persist this cycle. Ordinal, case-sensitive comparison — the same
    /// keying the write cache uses, so the path that went into
    /// <see cref="DiskManager.WriteBinFile"/> is the path to ask about.
    /// </summary>
    /// <param name="relativePath">The root-relative path to check.</param>
    public bool Failed(string relativePath)
    {
        for (int i = 0; i < _failures.Length; i++)
        {
            if (string.Equals(
                _failures[i].RelativePath,
                relativePath,
                StringComparison.Ordinal))
            {
                return true;
            }
        }

        return false;
    }
}

/*
 *------------------------------------------------------------
 * (FlushReport.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */