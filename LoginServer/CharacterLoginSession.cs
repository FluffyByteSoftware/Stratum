/*
 * (CharacterLoginSession.cs)
 *------------------------------------------------------------
 * Created - 6/28/2026 6:04:02 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;

namespace LoginServer;

/// <summary>
/// A connection in the character-login phase: the TCP connection it belongs to, the
/// account it authenticated as, and when it entered the phase.
/// </summary>
/// <param name="ConnectionId">The <c>TcpConnection.Id</c> this entry is keyed
/// by.</param>
/// <param name="AccountId">The account that authenticated on the connection.</param>
/// <param name="RegisteredAtUtc">The UTC instant the entry was registered.</param>
/// <remarks>
/// <see cref="RegisteredAtUtc"/> is recorded for forensics and parity with the UDP
/// side's session record; no timeout logic currently keys off it. A pending entry is
/// cleared when its connection drops or its create resolves, so an abandoned
/// connection is bounded by the connection's own lifetime rather than by a timer
/// here.
/// </remarks>
public readonly record struct CharacterLoginSession(
    long ConnectionId,
    string AccountId,
    DateTime RegisteredAtUtc);

/*
 *------------------------------------------------------------
 * (CharacterLoginSession.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */