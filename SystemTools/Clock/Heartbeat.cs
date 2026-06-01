/*
 * (Heartbeat.cs)
 *------------------------------------------------------------
 * Created - 5/10/2026 5:30:00 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Diagnostics;

namespace SystemTools.Clock;

/// <summary>
/// The process-wide fixed-timestep tick driver. Runs a dedicated thread that calls every registered
/// <see cref="ITickable"/> in registration order at a fixed rate (default 30 Hz). Subsystems gate
/// themselves to lower cadences via <see cref="TickContext.TickNumber"/>.
/// </summary>
/// <remarks>One Heartbeat per process. Typical lifecycle: <see cref="Initialize"/> at startup,
/// <see cref="Register"/> each subsystem in order of importance, <see cref="Start"/> to begin
/// ticking, <see cref="StopAsync"/> from a <c>finally</c> block on shutdown.
/// 
/// The loop uses <see cref="Stopwatch"/> for timing (wall-clock NTP jumps cannot affect it) and a
/// sleep-then-spin pattern to hit tick boundaries with sub-millisecond accuracy. Ticks are atomic:
/// once a tick begins, every registered subsystem runs to completion before the loop checks for
/// stop signals or catch-up. If a tick exceeds the master interval the loop accepts wall-clock
/// drift; if drift exceeds <see cref="MaxCatchUpTicks"/> the loop snaps to current time rather
/// than running ticks back-to-back to catch up.</remarks>
public sealed class Heartbeat
{
    private const int MaxCatchUpTicks = 5;
    private const double SleepThresholdSeconds = 0.002;

    private static Heartbeat? _instance;
    private static bool _initialized;

    /// <summary>
    /// The process-wide Heartbeat instance. Throws if accessed before <see cref="Initialize"/>.
    /// </summary>
    public static Heartbeat Instance =>
        _instance ?? throw new InvalidOperationException("Heartbeat not initialized.");

    /// <summary>
    /// True if <see cref="Initialize"/> has been called successfully. Does not imply
    /// <see cref="Start"/> has been called.
    /// </summary>
    public static bool IsRunning => _initialized && _instance is not null;

    private readonly int _tickRateHz;
    private readonly double _tickIntervalSeconds;
    private readonly List<ITickable> _systems = [];

    private Thread? _thread;
    private volatile bool _stopRequested;
    private volatile bool _started;
    private TaskCompletionSource<bool>? _stopCompletion;

    private long _currentTick;
    private double _elapsedSeconds;
    private double _measuredHz;

    /// <summary>
    /// The configured tick rate in Hz. Set at <see cref="Initialize"/>; immutable thereafter.
    /// </summary>
    public int TickRateHz => _tickRateHz;

    /// <summary>
    /// The number of ticks completed since <see cref="Start"/>. Reads are not synchronized; values
    /// observed from other threads may lag the actual count by one tick.
    /// </summary>
    public long CurrentTick => _currentTick;

    /// <summary>
    /// The actual measured tick rate based on wall-clock time over the last 30 ticks. Diverges from
    /// <see cref="TickRateHz"/> if the loop falls behind due to long ticks.
    /// </summary>
    public double MeasuredHz => _measuredHz;

    /// <summary>
    /// Initializes the process-wide Heartbeat. Subsequent calls are silently ignored.
    /// </summary>
    /// <param name="tickRateHz">The master tick rate in Hz. Defaults to 30. Subsystem cadences are
    /// expressed as divisors of this rate.</param>
    public static void Initialize(int tickRateHz = 30)
    {
        if (_initialized) return;
        if (tickRateHz <= 0)
            throw new ArgumentOutOfRangeException(nameof(tickRateHz), "Tick rate must be positive.");

        _instance = new Heartbeat(tickRateHz);
        _initialized = true;
    }

    private Heartbeat(int tickRateHz)
    {
        _tickRateHz = tickRateHz;
        _tickIntervalSeconds = 1.0 / tickRateHz;
        _measuredHz = tickRateHz;
    }

    /// <summary>
    /// Registers a subsystem to be ticked. Must be called before <see cref="Start"/>. Registration
    /// order determines tick order; register systems in order of importance (perception before AI,
    /// AI before combat, combat before networking, etc.).
    /// </summary>
    /// <param name="system">The subsystem to register.</param>
    /// <exception cref="InvalidOperationException">Thrown if called after <see cref="Start"/>.</exception>
    public void Register(ITickable system)
    {
        if (_started)
            throw new InvalidOperationException("Cannot register systems after Heartbeat has started.");
        _systems.Add(system);
    }

    /// <summary>
    /// Starts the tick loop on a dedicated thread. The thread runs until <see cref="StopAsync"/> is
    /// called. May only be called once per Heartbeat instance.
    /// </summary>
    /// <exception cref="InvalidOperationException">Thrown if called twice.</exception>
    public void Start()
    {
        if (_started)
            throw new InvalidOperationException("Heartbeat has already been started.");
        _started = true;

        _stopCompletion = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        _thread = new Thread(Loop)
        {
            Name = "Stratum.Heartbeat",
            IsBackground = false,
            Priority = ThreadPriority.AboveNormal
        };
        _thread.Start();
    }

    /// <summary>
    /// Signals the loop to stop and returns a task that completes after the current tick finishes
    /// and the loop thread exits. Heartbeat is one-shot; after this returns the instance is dead.
    /// </summary>
    public Task StopAsync()
    {
        if (!_started || _stopCompletion is null)
            return Task.CompletedTask;

        _stopRequested = true;
        return _stopCompletion.Task;
    }

    private void Loop()
    {
        var sw = Stopwatch.StartNew();
        double nextTickAt = 0;
        long measureStartTick = 0;
        double measureStartElapsed = 0;

        try
        {
            while (!_stopRequested)
            {
                double now = sw.Elapsed.TotalSeconds;
                double remaining = nextTickAt - now;

                if (remaining > SleepThresholdSeconds)
                {
                    Thread.Sleep(1);
                    continue;
                }
                if (remaining > 0)
                {
                    Thread.SpinWait(100);
                    continue;
                }

                _elapsedSeconds = now;
                RunTick();
                _currentTick++;
                nextTickAt += _tickIntervalSeconds;

                if (_currentTick - measureStartTick >= _tickRateHz)
                {
                    double window = now - measureStartElapsed;
                    if (window > 0)
                        _measuredHz = (_currentTick - measureStartTick) / window;
                    measureStartTick = _currentTick;
                    measureStartElapsed = now;
                }

                double drift = now - nextTickAt;
                if (drift > _tickIntervalSeconds * MaxCatchUpTicks)
                    nextTickAt = now + _tickIntervalSeconds;
            }
        }
        finally
        {
            _stopCompletion?.TrySetResult(true);
        }
    }

    private void RunTick()
    {
        var ctx = new TickContext(_currentTick, _tickIntervalSeconds, _elapsedSeconds);
        for (int i = 0; i < _systems.Count; i++)
        {
            try
            {
                _systems[i].Tick(in ctx);
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine(
                    $"[Heartbeat] System '{_systems[i].Name}' threw during tick {_currentTick}: {ex}");
            }
        }
    }
}

/*
 *------------------------------------------------------------
 * (Heartbeat.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */