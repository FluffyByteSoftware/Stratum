/*
 * (CharacterLoginRegistry.cs)
 *------------------------------------------------------------
 * Created - 6/28/2026 6:10:08 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using System.Collections.Generic;
using System.Threading;
using SystemTools.Logger;

namespace LoginServer;

/// <summary>
/// Tracks connections held open after a <c>NeedsCharacter</c> outcome, binding each
/// authenticated TCP connection to the account it authenticated as so the character-
/// create handler can resolve the owning account from the connection rather than from
/// the (untrusted) create packet.
/// </summary>
/// <remarks>
/// This is the TCP-side analog of Sentinel's <c>ClientSessionRegistry</c>, kept in
/// the <c>LoginServer</c> process because the connection it tracks lives here: the
/// identity is born when an auth on a LoginServer TCP connection resolves to
/// <c>NeedsCharacter</c>, and it is consumed by the create handler running on that
/// same connection. It deliberately does <b>not</b> enforce one entry per account the
/// way the UDP registry enforces one session per account: that rule there is a single-
/// use-token security property, whereas double-create here is already prevented by
/// <c>CharacterCreator</c> returning <c>AccountAlreadyHasCharacter</c>, so a liveness
/// guard would defend a case the service already closes. All state is guarded by an
/// internal lock — unlike the UDP path's single poll thread, LoginServer serves
/// connections concurrently, so the lock is load-bearing — and is held in memory only,
/// not surviving a restart.
/// </remarks>
public sealed class CharacterLoginRegistry
{
    private readonly Lock _gate = new();

    private readonly Dictionary<long, CharacterLoginSession> _pending = [];

    /// <summary>
    /// Registers a connection that authenticated but has no character yet, binding it
    /// to the account it authenticated as.
    /// </summary>
    /// <param name="connectionId">The <c>TcpConnection.Id</c> to key the entry
    /// by.</param>
    /// <param name="accountId">The account the connection authenticated as.</param>
    /// <returns><see langword="true"/> if the entry was registered;
    /// <see langword="false"/> if the connection is already tracked.</returns>
    /// <exception cref="ArgumentException"><paramref name="accountId"/> is
    /// <see langword="null"/> or empty.</exception>
    /// <remarks>
    /// A connection should authenticate once, so an already-tracked connection id is
    /// anomalous rather than expected; the attempt is rejected with a warning rather
    /// than silently overwriting, so the bug surfaces instead of hiding behind
    /// last-write-wins.
    /// </remarks>
    public bool TryRegister(long connectionId, string accountId)
    {
        ArgumentException.ThrowIfNullOrEmpty(accountId);

        lock (_gate)
        {
            if (_pending.ContainsKey(connectionId))
            {
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                    $"Rejected character-login entry for connection already " +
                    $"tracked: {connectionId}."));
                return false;
            }

            _pending[connectionId] =
                new CharacterLoginSession(
                    connectionId, accountId, DateTime.UtcNow);

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Registered character-login: account {accountId} " +
                $"(connection {connectionId}). Pending: {_pending.Count}."));

            return true;
        }
    }

    /// <summary>
    /// Removes the entry bound to the specified connection, if one exists.
    /// </summary>
    /// <param name="connectionId">The connection id whose entry should be
    /// removed.</param>
    /// <returns><see langword="true"/> if an entry was removed;
    /// <see langword="false"/> if no entry was tracked for the connection.</returns>
    /// <remarks>
    /// Called both when a create resolves and when a pending connection drops, so a
    /// no-op removal (no entry tracked) is an ordinary outcome — a connection that
    /// never reached <c>NeedsCharacter</c> has nothing here — and returns
    /// <see langword="false"/> without logging.
    /// </remarks>
    public bool Remove(long connectionId)
    {
        lock (_gate)
        {
            if (!_pending.Remove(connectionId, out CharacterLoginSession entry))
                return false;

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Removed character-login: account {entry.AccountId} " +
                $"(connection {connectionId}). Pending: {_pending.Count}."));

            return true;
        }
    }

    /// <summary>
    /// Attempts to retrieve the entry bound to the specified connection.
    /// </summary>
    /// <param name="connectionId">The connection id to look up.</param>
    /// <param name="session">When this method returns, contains the entry if one was
    /// tracked; otherwise the default value.</param>
    /// <returns><see langword="true"/> if an entry was found; otherwise
    /// <see langword="false"/>.</returns>
    public bool TryGet(long connectionId, out CharacterLoginSession session)
    {
        lock (_gate)
            return _pending.TryGetValue(connectionId, out session);
    }
}

/*
 *------------------------------------------------------------
 * (CharacterLoginRegistry.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */