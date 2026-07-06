/*
 * (RegistrationControlIds.cs)
 *------------------------------------------------------------
 * Created - 7/6/2026 3:17:35 PM UTC
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace ZoneManager.Registration;

/// <summary>
/// Packet type IDs for the Zone↔ZoneManager registration control plane.
/// Server-only traffic that never crosses the client boundary, so it lives
/// here in ZoneManager rather than in <c>Shared.MessagePacketIds</c>.
/// </summary>
/// <remarks>
/// These use channel byte <c>0xF0</c>, deliberately clear of Shared's
/// ascending client channels (<c>0x00</c>–<c>0x03</c>). Sentinel's
/// client↔zone bridge forwards only client channels; a control-plane
/// channel it never touches makes "never bridged to a client" structural,
/// not a matter of convention. The type layout matches every other packet:
/// 1 byte channel + 3 bytes ID.
/// </remarks>
public static class RegistrationControlIds
{
    /// <summary>
    /// ZoneManager → Zone. Sent on the accept path once the peer is
    /// established, confirming the zone is registered. Echoes the zone id
    /// the registration was accepted under.
    /// </summary>
    public const uint RegistrationAccepted = 0xF0_00_00_01;
}

/*
 *------------------------------------------------------------
 * (RegistrationControlIds.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */