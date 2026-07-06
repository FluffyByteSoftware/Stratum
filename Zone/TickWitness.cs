/*
 * (TickWitness.cs)
 *------------------------------------------------------------
 * Created - 7/6/2026 10:38:56 PM UTC
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using SystemTools.Clock;

namespace Zone;

/// <summary>
/// A deliberately empty subsystem registered on the <see cref="Heartbeat"/>
/// for this brick's verify gate: it does no work, only counts how many times
/// it was ticked. Removed (or repurposed) when the first real system lands.
/// </summary>
/// <remarks>
/// The count exists because <see cref="Heartbeat.CurrentTick"/> only proves
/// the loop spun; this counter proves dispatch reached a registered system.
/// Two different claims — the gate reads both and expects them equal. Reads
/// from the shutdown path may lag the tick thread by one; exactness is not
/// required, agreement within a tick is.
/// </remarks>
internal sealed class TickWitness : ITickable
{
    private long _tickCount;

    /// <summary>
    /// Gets the current tick count.
    /// </summary>
    public long TickCount => _tickCount;

    /// <inheritdoc/>
    public string Name => "TickWitness";

    /// <summary>
    /// Increments the internal tick counter.
    /// </summary>
    /// <param name="context">The tick context containing timing and state information.</param>
    public void Tick(in TickContext context) => _tickCount++;
}

/*
 *------------------------------------------------------------
 * (TickWitness.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */