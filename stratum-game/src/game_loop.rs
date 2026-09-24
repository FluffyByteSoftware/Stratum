//! File:     stratum-game/src/game_loop.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! The game loop.  One thread, `game-tick`, owns the world and runs the
//! tick every 50 ms.  It starts when the admin starts the server (S) in the
//! Launcher) and stops when they stop it, so the world only runs while
//! players can get in.
//!
//! The clock is stratum-cycle's.  The loop asks it for the next tick, runs
//! that tick's work, and asks again.  Five ticks make a round, and each
//! piece of work checks the tick's sub-tick (0 to 4) to see whether this is
//! its turn, so no single tick carries everything.
//!
//! Nothing outside this thread touches the world.  The network will hand
//! the loop its messages through a queue, emptied once a tick, the same way
//! the login worker gets its jobs.  That comes later, from the Networking
//! project.
//!
//! Nothing real runs on the tick yet.  For now the loop keeps time, runs the
//! chat room (chat_room.rs, a test), and says at the end how long its ticks
//! took.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bevy_ecs::prelude::{Entity, World};
use stratum_cycle::{Clock, Tick, TICK_MS};
use stratum_tools::scribe::{self, Channel};

use crate::chat_room;

/// The loop's thread, while it runs.  `None` when the world is stopped.
static THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

/// Set to tell the loop to finish the tick it's on and stop.
static STOP: AtomicBool = AtomicBool::new(false);

/// Starts the game loop on its own thread.  If it's already running, this
/// does nothing.  An `Err` says why the thread couldn't be started.
pub fn start() -> Result<(), String> {
    let mut guard = THREAD.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_some() {
        return Ok(());
    }

    STOP.store(false, Ordering::SeqCst);
    match thread::Builder::new().name("game-tick".to_string()).spawn(run) {
        Ok(handle) => {
            *guard = Some(handle);
            Ok(())
        }
        Err(error) => Err(format!("Couldn't start the game loop: {}", error)),
    }
}

/// Stops the game loop and waits for it.  The tick it's on finishes first,
/// so this takes up to a tick (50 ms).  If it isn't running, this does
/// nothing.
pub fn stop() {
    let handle = {
        let mut guard = THREAD.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.take()
    };

    if let Some(handle) = handle {
        STOP.store(true, Ordering::SeqCst);
        // join() is an Err only if the thread had already crashed.  Rust
        // prints a panic to the terminal, but not to our log, so say it here.
        if handle.join().is_err() {
            scribe::error(Channel::World, "The game loop had crashed before it was stopped.  \
                                           The world wasn't ticking.");
        }
    }
}

/// The loop itself, on the `game-tick` thread.  The world lives here and
/// nowhere else.
fn run() {
    let mut world = World::new();
    let mut clock = Clock::start();
    let mut stats = TickStats::default();
    let chatters = chat_room::open(&mut world);

    scribe::info(Channel::World, &format!("The world is ticking, every {} ms.", TICK_MS));

    while !STOP.load(Ordering::SeqCst) {
        let tick = clock.wait();

        if tick.clock_reset {
            scribe::warn(Channel::World, &format!("The tick fell {} ms behind, which is more than a round, \
                                                   so the clock started fresh at tick {}.  Nothing was \
                                                   skipped, but the world is that far behind the wall \
                                                   clock now.",
                                                  tick.late.as_millis(), tick.number));
        }

        let began = Instant::now();
        run_tick(&mut world, &tick, &chatters);
        stats.record(began.elapsed(), tick.clock_reset);
    }

    scribe::info(Channel::World, &stats.summary());
}

/// One tick's work.  Whatever runs every tick goes at the top, and whatever
/// runs once a round goes under its sub-tick.
// TODO(tick-work): nothing real runs on the tick yet.  Movement and combat
// every tick, and the rest spread across the sub-tick slots.
fn run_tick(world: &mut World, tick: &Tick, chatters: &[Entity]) {
    chat_room::speak(world, chatters, tick.number);
}

/// How the ticks went, for the line in the log when the loop stops.  The
/// time is the work alone, not the sleeping.  tick-sim's rule is an
/// average under about 20 ms of the 50.
#[derive(Debug, Default)]
struct TickStats {
    ticks: u64,
    work: Duration,
    slowest: Duration,
    resets: u64,
}

impl TickStats {
    fn record(&mut self, work: Duration, clock_reset: bool) {
        self.ticks += 1;
        self.work += work;
        if work > self.slowest {
            self.slowest = work;
        }
        if clock_reset {
            self.resets += 1;
        }
    }

    /// The average work per tick, in milliseconds.
    fn average_ms(&self) -> f64 {
        if self.ticks == 0 {
            return 0.0;
        }
        in_ms(self.work) / self.ticks as f64
    }

    fn summary(&self) -> String {
        let resets = match self.resets {
            0 => "The clock never had to start fresh.".to_string(),
            count => format!("The clock started fresh {} time(s).", count),
        };
        format!("The world stopped after {} tick(s).  A tick's work took {:.2} ms on average, \
                 and {:.2} ms at the slowest.  {}",
                self.ticks, self.average_ms(), in_ms(self.slowest), resets)
    }
}

/// A time in milliseconds, with the fraction kept.
// Rust note: this goes through whole nanoseconds, so 9 ms comes out as
// exactly 9.0.  Going through seconds first can leave a stray digit at the
// end, and a test comparing it would fail.
fn in_ms(time: Duration) -> f64 {
    time.as_nanos() as f64 / 1_000_000.0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// The thread isn't tested here: it logs, and tests stay out of the global
// state.  The Launcher is its test.  What's tested is the counting.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_ticks_is_no_time() {
        let stats = TickStats::default();
        assert_eq!(stats.average_ms(), 0.0);
        assert_eq!(stats.summary(), "The world stopped after 0 tick(s).  A tick's work took 0.00 ms on \
                                     average, and 0.00 ms at the slowest.  The clock never had to start \
                                     fresh.");
    }

    #[test]
    fn the_ticks_are_counted_and_the_slowest_kept() {
        let mut stats = TickStats::default();
        stats.record(Duration::from_millis(2), false);
        stats.record(Duration::from_millis(6), true);
        stats.record(Duration::from_millis(1), false);

        assert_eq!(stats.ticks, 3);
        assert_eq!(stats.average_ms(), 3.0);
        assert_eq!(stats.slowest, Duration::from_millis(6));
        assert_eq!(stats.summary(), "The world stopped after 3 tick(s).  A tick's work took 3.00 ms on \
                                     average, and 6.00 ms at the slowest.  The clock started fresh 1 \
                                     time(s).");
    }
}