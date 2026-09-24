//! File:     stratum-cycle/src/lib.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! The tick timers.  The server runs on a tick every 50 ms, and this is the
//! clock that says when the next one is due.  It keeps time and counts
//! ticks, and that's all.  The work each tick does belongs to whoever runs
//! the loop (the game, for now), so this crate knows nothing about the game
//! and depends on nothing.
//!
//! Five ticks make a round, 250 ms.  Every tick knows its number and its
//! sub-tick (0 to 4), and each piece of work checks whether this tick is its
//! turn.  That spreads the work out, so no single tick carries everything:
//!
//! ```text
//! tick 0  sub-tick 0   movement, combat, AI
//! tick 1  sub-tick 1   movement, combat, regen
//! ...
//! tick 5  sub-tick 0   a new round
//! ```
//!
//! The clock sleeps to fixed deadlines (start, start + 50 ms, start + 100 ms
//! and so on), never "50 ms after the last tick finished", so being a little
//! late now and then never adds up.  tick-sim measured it: under 0.1 ms late
//! on a quiet machine, under 2 ms with Dota 2 running.
//!
//! When a tick runs slow, the ticks it held up aren't dropped.  They run
//! straight after each other, with no sleep, until the clock is back on
//! schedule.  Dropping them would mean whatever lives on those sub-ticks
//! misses its turn and waits a whole round.  If the clock falls more than a
//! round behind (a stall, the machine swapping, a debugger), it stops trying
//! to catch up and starts fresh from now.  Even then no tick number is
//! skipped, so every sub-tick still comes round in order.  What's lost is
//! time: the game falls behind the wall clock by however long the stall was.

use std::thread;
use std::time::{Duration, Instant};

/// How long a tick is.  20 ticks a second.
pub const TICK_MS: u64 = 50;

/// How many ticks make a round.  A round is 250 ms.
pub const TICKS_PER_ROUND: u64 = 5;

/// How many ticks behind the clock can fall and still catch up.  One round.
/// Past this it starts fresh from now instead.
pub const CATCH_UP_CAP: u64 = 5;

const TICK: Duration = Duration::from_millis(TICK_MS);
const MOST_BEHIND: Duration = Duration::from_millis(TICK_MS * CATCH_UP_CAP);

/// One tick, as the clock hands it out.  The number counts every tick that
/// has run since the clock started, and never skips.
// Rust note: u64 won't run out.  At 20 ticks a second it would take about
// 29 billion years.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tick {
    pub number: u64,
    /// How late the tick started against its deadline.  Zero when it was
    /// on time.  While the clock catches up, this shrinks tick by tick.
    pub late: Duration,
    /// True when the clock had fallen too far behind and started fresh at
    /// this tick.  `late` says how far behind it was when it gave up.  The
    /// loop should log it: a server that keeps doing this is running over
    /// its budget.
    pub clock_reset: bool,
}

impl Tick {
    /// Where this tick falls in its round, 0 to 4.
    pub fn sub_tick(&self) -> u64 {
        self.number % TICKS_PER_ROUND
    }

    /// Which round this tick is in, counting from 0.
    pub fn round(&self) -> u64 {
        self.number / TICKS_PER_ROUND
    }
}

/// The clock.  The loop that owns it calls `wait()` at the top of every
/// tick, and gets the tick back when it's time to run it.
pub struct Clock {
    next_number: u64,
    next_deadline: Instant,
}

impl Clock {
    /// Starts the clock.  The first tick is due straight away.
    pub fn start() -> Clock {
        Clock { next_number: 0, next_deadline: Instant::now() }
    }

    /// Waits until the next tick is due and hands it back.  If the tick is
    /// already late, this doesn't wait at all.
    pub fn wait(&mut self) -> Tick {
        let (tick, sleep) = self.step(Instant::now());
        if let Some(time) = sleep {
            thread::sleep(time);
        }
        tick
    }

    /// The part of `wait()` that decides, without the sleeping, so the tests
    /// can hand it whatever "now" they like.  Hands back the tick, and how
    /// long to sleep before running it.
    fn step(&mut self, now: Instant) -> (Tick, Option<Duration>) {
        let number = self.next_number;
        self.next_number += 1;
        let deadline = self.next_deadline;

        // Early.  Sleep until the deadline.
        if now < deadline {
            self.next_deadline = deadline + TICK;
            let tick = Tick { number, late: Duration::ZERO, clock_reset: false };
            return (tick, Some(deadline - now));
        }

        // Too far behind to catch up.  This tick runs now, and the next one
        // is due a tick after it, as if the clock had started here.
        let late = now - deadline;
        if late > MOST_BEHIND {
            self.next_deadline = now + TICK;
            return (Tick { number, late, clock_reset: true }, None);
        }

        // On time, or late enough to be catching up.  Run it now, and the
        // next deadline stays where it always was.
        self.next_deadline = deadline + TICK;
        (Tick { number, late, clock_reset: false }, None)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Nothing in here sleeps.  Each test takes "now" once, then hands the clock
// times made from it, so the tests are exact and quick.
#[cfg(test)]
mod tests {
    use super::*;

    fn ms(count: u64) -> Duration {
        Duration::from_millis(count)
    }

    #[test]
    fn sub_ticks_and_rounds() {
        let tick = |number| Tick { number, late: Duration::ZERO, clock_reset: false };

        assert_eq!(tick(0).sub_tick(), 0);
        assert_eq!(tick(4).sub_tick(), 4);
        assert_eq!(tick(4).round(), 0);
        assert_eq!(tick(5).sub_tick(), 0);
        assert_eq!(tick(5).round(), 1);
        assert_eq!(tick(12).sub_tick(), 2);
        assert_eq!(tick(12).round(), 2);
    }

    #[test]
    fn an_early_tick_sleeps_to_its_deadline() {
        let start = Instant::now();
        let mut clock = Clock { next_number: 0, next_deadline: start };

        let (tick, sleep) = clock.step(start);
        assert_eq!(tick.number, 0);
        assert_eq!(tick.late, Duration::ZERO);
        assert_eq!(sleep, None);

        // Tick 0 took 20 ms, so tick 1 sleeps the other 30.
        let (tick, sleep) = clock.step(start + ms(20));
        assert_eq!(tick.number, 1);
        assert_eq!(sleep, Some(ms(30)));

        // Tick 1 woke on time at 50 and took 20 ms.  Tick 2 is due at 100.
        let (tick, sleep) = clock.step(start + ms(70));
        assert_eq!(tick.number, 2);
        assert_eq!(sleep, Some(ms(30)));
    }

    #[test]
    fn a_slow_tick_is_caught_up() {
        let start = Instant::now();
        let mut clock = Clock { next_number: 0, next_deadline: start };
        let _ = clock.step(start);

        // Tick 0 took 180 ms.  Ticks 1 to 5 run back to back, each 20 ms,
        // each a little less late, and none of them sleeps.
        let (tick, sleep) = clock.step(start + ms(180));
        assert_eq!((tick.number, tick.late, sleep), (1, ms(130), None));
        let (tick, sleep) = clock.step(start + ms(200));
        assert_eq!((tick.number, tick.late, sleep), (2, ms(100), None));
        let (tick, sleep) = clock.step(start + ms(220));
        assert_eq!((tick.number, tick.late, sleep), (3, ms(70), None));
        let (tick, sleep) = clock.step(start + ms(240));
        assert_eq!((tick.number, tick.late, sleep), (4, ms(40), None));
        let (tick, sleep) = clock.step(start + ms(260));
        assert_eq!((tick.number, tick.late, sleep), (5, ms(10), None));

        // Caught up: tick 6 is due at 300 and sleeps till then.
        let (tick, sleep) = clock.step(start + ms(280));
        assert_eq!((tick.number, tick.late, sleep), (6, Duration::ZERO, Some(ms(20))));
        assert!(!tick.clock_reset);
    }

    #[test]
    fn a_round_behind_still_catches_up() {
        let start = Instant::now();
        let mut clock = Clock { next_number: 0, next_deadline: start };
        let _ = clock.step(start);

        // Tick 1 was due at 50.  At 300 it's exactly a round late, which is
        // still inside the cap.
        let (tick, _) = clock.step(start + ms(300));
        assert_eq!(tick.late, ms(250));
        assert!(!tick.clock_reset);
    }

    #[test]
    fn too_far_behind_starts_fresh() {
        let start = Instant::now();
        let mut clock = Clock { next_number: 0, next_deadline: start };
        let _ = clock.step(start);

        // Tick 0 stalled for 400 ms.  Tick 1 was due at 50, so it's 350
        // late, past the cap.  It runs now, and the clock starts fresh.
        let (tick, sleep) = clock.step(start + ms(400));
        assert_eq!((tick.number, tick.late, sleep), (1, ms(350), None));
        assert!(tick.clock_reset);

        // No number was skipped, and tick 2 is due 50 ms after tick 1 ran.
        let (tick, sleep) = clock.step(start + ms(420));
        assert_eq!((tick.number, tick.late, sleep), (2, Duration::ZERO, Some(ms(30))));
        assert!(!tick.clock_reset);
    }
}