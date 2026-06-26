/*
 * (DispatchOutcome.cs)
 *------------------------------------------------------------
 * Created - 5/29/2026 12:28:56 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Networking.Dispatch;

/// <summary>
/// Specifies the outcome of a packet dispatch operation.
/// </summary>
public enum DispatchOutcome : byte
{
    /// <summary>
    /// Never produced by the dispatcher; a default result leads here
    /// </summary>
    None = 0,
    /// <summary>
    /// Packet was successfully dispatched to a handler, and the handler 
    /// executed without throwing an exception.
    /// </summary>
    Success,
    /// <summary>
    /// Packet was unsuccessfully dispatched.
    /// </summary>
    UnknownType,
    /// <summary>
    /// Packet was determined to be invalid.
    /// </summary>
    InvalidPacket,
    /// <summary>
    /// Packet was successfully dispatched to a handler, and the handler
    /// threw an exception.
    /// </summary>
    HandlerException
}



/*
 *------------------------------------------------------------
 * (DispatchOutcome.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */