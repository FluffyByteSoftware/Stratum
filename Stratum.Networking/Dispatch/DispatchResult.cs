/*
 * (DispatchResult.cs)
 *------------------------------------------------------------
 * Created - 5/29/2026 12:56:00 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Stratum.Networking.Dispatch;

public readonly struct DispatchResult
{
    /// <summary>
    /// The outcome of the packet dispatch.
    /// </summary>
    public DispatchOutcome Outcome { get; }
    /// <summary>
    /// The TypeId of the packet/dispatch outcome.
    /// Used for logging/metrics primarily.
    /// </summary>
    public uint TypeId { get; }
    /// <summary>
    /// The exception (if available) that occurred at the dispatcher.
    /// </summary>
    public Exception? Exception { get; }

    private DispatchResult(DispatchOutcome outcome, 
        uint typeId, 
        Exception? exception = null) 
    {
        Outcome = outcome;
        TypeId = typeId;
        Exception = exception;
    }

    /// <summary>
    /// Was this a successfully dispatched packet outcome?
    /// </summary>
    public bool IsSuccess => Outcome == DispatchOutcome.Success;

    /// <summary>
    /// Creates a successful dispatch result with the specified type identifier.
    /// </summary>
    /// <param name="typeId">The type identifier.</param>
    /// <returns>A dispatch result indicating success.</returns>
    public static DispatchResult Success(uint typeId) 
        => new(DispatchOutcome.Success, typeId, null);

    /// <summary>
    /// Creates a dispatch result indicating an unknown type was encountered.
    /// </summary>
    /// <param name="typeId">The identifier of the unknown type.</param>
    /// <returns>A dispatch result with an unknown type outcome.</returns>
    public static DispatchResult UnknownType(uint typeId)
        => new(DispatchOutcome.UnknownType, typeId, null);

    /// <summary>
    /// Creates a dispatch result indicating an invalid packet.
    /// </summary>
    /// <param name="typeId">The packet type identifier.</param>
    /// <param name="exception">The exception that occurred during packet processing.</param>
    /// <returns>A new DispatchResult with the InvalidPacket outcome.</returns>
    public static DispatchResult InvalidPacket(uint typeId, Exception exception)
        => new(DispatchOutcome.InvalidPacket, typeId, exception);

    /// <summary>
    /// Creates a dispatch result indicating a handler exception occurred.
    /// </summary>
    /// <param name="typeId">The type identifier.</param>
    /// <param name="exception">The exception that occurred during handler execution.</param>
    /// <returns>A <see cref="DispatchResult"/> with a handler exception outcome.</returns>
    public static DispatchResult HandlerException(uint typeId, Exception exception)
        => new(DispatchOutcome.HandlerException, typeId, exception);
}



/*
 *------------------------------------------------------------
 * (DispatchResult.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */