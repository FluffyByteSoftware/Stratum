/*
 * (ClientSessionRegistry.cs)
 *------------------------------------------------------------
 * Created - 6/13/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib;
using System;
using System.Collections.Generic;
using System.Threading;
using SystemTools.Logger;

namespace Sentinel;

/// <summary>
/// An authenticated UDP session: the peer it belongs to, the account it
/// authenticated as, and when it was established.
/// </summary>
/// <param name="PeerId">The LiteNetLib peer identifier this session is
/// keyed by.</param>
/// <param name="AccountId">The account that authenticated this session.</param>
/// <param name="ConnectedAtUtc">The UTC instant the session was
/// registered.</param>
public readonly record struct ClientSession(
    int PeerId,
    string AccountId,
    DateTime ConnectedAtUtc);

/// <summary>
/// Tracks authenticated UDP sessions for a single Sentinel instance,
/// keyed by LiteNetLib peer id, and enforces one live session per
/// account.
/// </summary>
/// <remarks>
/// In normal operation every method is called from <c>UdpHost</c>'s
/// single polling thread, so contention is not expected. The registry
/// nonetheless guards all state with an internal lock so its correctness
/// does not depend on that calling convention: a caller on any thread
/// sees a consistent view, and the account-liveness check and the
/// registration it gates happen atomically as one operation. Sessions
/// are held in memory only and do not survive a Sentinel restart.
/// </remarks>
public sealed class ClientSessionRegistry
{
    private readonly Lock _gate = new();
    private readonly Dictionary<int, ClientSession> _sessions = [];
    private readonly HashSet<string> _liveAccounts = new(StringComparer.Ordinal);

    /// <summary>
    /// Attempts to register a newly authenticated session, rejecting the
    /// attempt if the account already holds a live session.
    /// </summary>
    /// <param name="peer">The established peer to bind the session to.</param>
    /// <param name="accountId">The account the peer authenticated as.</param>
    /// <returns><see langword="true"/> if the session was registered;
    /// <see langword="false"/> if the account already has a live session
    /// or the peer id is already tracked.</returns>
    /// <exception cref="ArgumentNullException"><paramref name="peer"/> is
    /// <see langword="null"/>.</exception>
    /// <exception cref="ArgumentException"><paramref name="accountId"/> is
    /// <see langword="null"/> or empty.</exception>
    public bool TryRegister(NetPeer peer, string accountId)
    {
        ArgumentNullException.ThrowIfNull(peer);
        ArgumentException.ThrowIfNullOrEmpty(accountId);

        lock (_gate)
        {
            if (_liveAccounts.Contains(accountId))
            {
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                    $"Rejected UDP session for account already live: " +
                    $"{accountId} (peer {peer.Id})."));
                return false;
            }

            if (_sessions.ContainsKey(peer.Id))
            {
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                    $"Rejected UDP session for peer already tracked: " +
                    $"{peer.Id}."));
                return false;
            }

            _sessions[peer.Id] =
                new ClientSession(peer.Id, accountId, DateTime.UtcNow);
            _liveAccounts.Add(accountId);

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Registered UDP session: account {accountId} " +
                $"(peer {peer.Id}). Live sessions: {_sessions.Count}."));

            return true;
        }
    }

    /// <summary>
    /// Removes the session bound to the specified peer, if one exists.
    /// </summary>
    /// <param name="peerId">The peer id whose session should be removed.</param>
    /// <returns><see langword="true"/> if a session was removed;
    /// <see langword="false"/> if no session was tracked for the peer.</returns>
    public bool Remove(int peerId)
    {
        lock (_gate)
        {
            if (!_sessions.Remove(peerId, out ClientSession session))
                return false;

            _liveAccounts.Remove(session.AccountId);

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Removed UDP session: account {session.AccountId} " +
                $"(peer {peerId}). Live sessions: {_sessions.Count}."));

            return true;
        }
    }

    /// <summary>
    /// Attempts to retrieve the session bound to the specified peer.
    /// </summary>
    /// <param name="peerId">The peer id to look up.</param>
    /// <param name="session">When this method returns, contains the
    /// session if one was tracked; otherwise the default value.</param>
    /// <returns><see langword="true"/> if a session was found; otherwise
    /// <see langword="false"/>.</returns>
    public bool TryGet(int peerId, out ClientSession session)
    {
        lock (_gate)
            return _sessions.TryGetValue(peerId, out session);
    }
}

/*
 *------------------------------------------------------------
 * (ClientSessionRegistry.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */