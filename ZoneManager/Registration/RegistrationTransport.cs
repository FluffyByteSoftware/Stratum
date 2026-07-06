/*
 * (RegistrationTransport.cs)
 *------------------------------------------------------------
 * Created - 7/6/2026 5:45:02 PM UTC
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace ZoneManager.Registration;

/// <summary>
/// Transport constants for the Zone↔ZoneManager registration control plane.
/// Server-only, like <see cref="RegistrationControlIds"/>: this is the single
/// source of truth for the registration port, owned by ZoneManager and read
/// by Zone through its one-way reference. Deliberately not a
/// <c>Shared.NetworkConfig</c> field — Zone→ZoneManager already exists, so
/// keeping it here avoids a shared-tier fork (and its Probe re-run) while
/// still giving both sides one value to agree on.
/// </summary>
public static class RegistrationTransport
{
    /// <summary>
    /// Loopback UDP port ZoneManager binds and Zone dials to register.
    /// Graduates to a <c>Shared.NetworkConfig</c> field only if a third,
    /// non-server consumer ever needs it — that would be the shared-tier fork.
    /// </summary>
    public const int Port = 9050;
}

/*
 *------------------------------------------------------------
 * (RegistrationTransport.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */