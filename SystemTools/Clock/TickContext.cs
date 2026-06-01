/*
 * (TickContext.cs)
 *------------------------------------------------------------
 * Created - 5/10/2026 5:30:00 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace SystemTools.Clock;

/// <summary>
/// Per-tick state passed to every <see cref="ITickable"/> on each invocation. Immutable for the
/// duration of the tick.
/// </summary>
/// <remarks>Constructed by the Heartbeat at the start of each tick and passed by <c>in</c> to avoid
/// copying. <see cref="DeltaSeconds"/> is always the fixed logical timestep regardless of how long
/// the previous tick took in wall-clock time; this is the determinism guarantee of the fixed-timestep
/// loop.</remarks>
public readonly struct TickContext(long tickNumber, double deltaSeconds, double elapsedSeconds)
{
    /// <summary>
    /// Monotonic tick counter, starting at 0 on the first tick after <see cref="Heartbeat.Start"/>.
    /// Increments by 1 each tick. Subsystems gate themselves to lower cadences by checking
    /// <c>TickNumber % divisor == 0</c>. At 30 Hz a <see cref="long"/> will not overflow within any
    /// realistic process lifetime.
    /// </summary>
    public long TickNumber { get; } = tickNumber;

    /// <summary>
    /// The fixed logical timestep in seconds. Always equal to <c>1.0 / TickRateHz</c>. Use this for
    /// time-based math (<c>position += velocity * ctx.DeltaSeconds</c>) so the simulation remains
    /// deterministic regardless of wall-clock drift.
    /// </summary>
    public double DeltaSeconds { get; } = deltaSeconds;

    /// <summary>
    /// Wall-clock seconds elapsed since the Heartbeat started. For diagnostics and logging only;
    /// not used in simulation math.
    /// </summary>
    public double ElapsedSeconds { get; } = elapsedSeconds;
}

/*
 *------------------------------------------------------------
 * (TickContext.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */