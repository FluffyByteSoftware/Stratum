/*
 * (ITickable.cs)
 *------------------------------------------------------------
 * Created - 5/10/2026 5:30:00 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace SystemTools.Clock;

/// <summary>
/// A subsystem driven by the <see cref="Heartbeat"/>. Implementers are called once per master tick in
/// the order they were registered, and may self-gate to lower cadences by checking
/// <see cref="TickContext.TickNumber"/> against a divisor of the master rate.
/// </summary>
/// <remarks>Subsystems must complete their work within their <see cref="Tick"/> call. The Heartbeat
/// finishes every tick atomically before considering catch-up or stop signals; long-running work
/// inside a tick blocks the loop and contributes to drift.</remarks>
public interface ITickable
{
    /// <summary>
    /// A diagnostic name for this subsystem. Used in startup banners and per-system logging. Does not
    /// need to be unique but should be descriptive (e.g. "Combat", "AI", "ZoneSim:forest_01").
    /// </summary>
    string Name { get; }

    /// <summary>
    /// Called once per master tick by the Heartbeat. Implementations should be deterministic and
    /// reproducible: use <see cref="TickContext.DeltaSeconds"/> for time-based math rather than
    /// measuring wall-clock time.
    /// </summary>
    /// <param name="context">The current tick context, passed by reference to avoid copying the struct.</param>
    void Tick(in TickContext context);
}

/*
 *------------------------------------------------------------
 * (ITickable.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */