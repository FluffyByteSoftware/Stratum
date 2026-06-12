/*
 * (LockoutTracker.cs)
 *------------------------------------------------------------
 * Created - 5/30/2026 1:18:27 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using System.Collections.Concurrent;

namespace LoginServer;

/// <summary>
/// Tracks failed authentication attempts and enforces temporary account 
/// lockouts.
/// </summary>
/// <remarks>Accounts are locked for 1 minute after 3 consecutive failures.
/// Lockouts expire automatically. Account
/// identifiers are case-insensitive. Thread-safe.</remarks>
public sealed class LockoutTracker
{
    private const int MaxFailures = 3;
    private static readonly long LockoutMs = 
        (long)TimeSpan.FromMinutes(1).TotalMilliseconds;

    private sealed class Entry
    {
        public int FailureCount;
        public long LockedUntilUnixMs;
    }

    private readonly ConcurrentDictionary<string, Entry> _entries
        = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Determines whether the specified account is currently locked.
    /// </summary>
    /// <param name="accountId">The account identifier to check.</param>
    /// <returns>true if the account is locked and the lock period has not 
    /// expired; otherwise, false.</returns>
    public bool IsLocked(string accountId)
    {
        if (string.IsNullOrEmpty(accountId)) return false;
        if (!_entries.TryGetValue(accountId, out var entry)) return false;

        lock (entry)
        {
            if (entry.LockedUntilUnixMs == 0) return false;

            if (NowMs() >= entry.LockedUntilUnixMs)
            {
                entry.FailureCount = 0;
                entry.LockedUntilUnixMs = 0;
                return false;
            }

            return true;
        }
    }

    /// <summary>
    /// Records a failure attempt for the specified account and locks it 
    /// if the failure threshold is reached.
    /// </summary>
    /// <param name="accountId">The account identifier.</param>
    /// <returns><c>true</c> if this failure triggers a lockout; 
    /// otherwise, <c>false</c>.</returns>
    public bool RecordFailure(string accountId)
    {
        if (string.IsNullOrEmpty(accountId)) return false;

        var entry = _entries.GetOrAdd(accountId, static _ => new Entry());

        lock (entry)
        {
            long now = NowMs();

            // An expired lock resets the slate before this failure is counted.
            if (entry.LockedUntilUnixMs != 0 && now >= entry.LockedUntilUnixMs)
            {
                entry.FailureCount = 0;
                entry.LockedUntilUnixMs = 0;
            }

            bool wasLocked = entry.LockedUntilUnixMs != 0;
            entry.FailureCount++;

            if (!wasLocked && entry.FailureCount >= MaxFailures)
            {
                entry.LockedUntilUnixMs = now + LockoutMs;
                return true;
            }

            return false;
        }
    }

    /// <summary>
    /// Removes the entry associated with the specified account identifier.
    /// </summary>
    /// <param name="accountId">The account identifier of the entry to 
    /// remove.</param>
    public void Clear(string accountId)
    {
        if (string.IsNullOrEmpty(accountId)) return;
        _entries.TryRemove(accountId, out _);
    }

    private static long NowMs() => DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
}

/*
 *------------------------------------------------------------
 * (LockoutTracker.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */