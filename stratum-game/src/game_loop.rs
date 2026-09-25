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
//! Nothing outside this thread touches the world.  The network hands the
//! loop its messages through a queue (stratum-networking's GameMessage),
//! and the loop empties it at the top of every tick, before any other
//! work.  Two kinds so far: a player has come in over UDP, so they go into
//! the world from their player file, and a player has gone, so they are
//! read back, saved and taken out.  The loop only knows a player by their
//! account; which connection and address they have is the network's.
//!
//! When the loop stops, networking is already down and nobody is coming or
//! going, so whoever is still in gets saved on the way out.
//!
//! The voxel world (world.rs) is loaded from its folder at S), before the
//! thread starts, and a world that won't load stops the server from
//! starting: there is nothing for a player to stand in.  It goes on the
//! `World` as a resource, and comes down with it at stop.
//!
//! Nothing else runs on the tick yet.  For now the loop keeps time, runs
//! the chat room (chat_room.rs, a test), and says at the end how long its
//! ticks took.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bevy_ecs::prelude::{Entity, World};
use stratum_cycle::{Clock, Tick, TICK_MS};
use stratum_networking::GameMessage;
use stratum_tools::account;
use stratum_tools::scribe::{self, Channel};

use crate::actor::Player;
use crate::chat_room;
use crate::chunk::{AIR, BlockId, CHUNK_SIDE};
use crate::player_file;
use crate::world::{self, VoxelWorld};

/// The loop's thread, while it runs.  `None` when the world is stopped.
static THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

/// Set to tell the loop to finish the tick it's on and stop.
static STOP: AtomicBool = AtomicBool::new(false);

/// One player's stay in the world, by account.
struct Stay {
    entity: Entity,
    /// The character's short name, for the log.
    character: String,
    /// Where their UDP packets come from.  A Left has to name the same
    /// address, or it's for a stay that has already been replaced.
    address: SocketAddr,
}

/// Who is in the world, by account.  An account is only on once, so one
/// stay each.
type Players = HashMap<String, Stay>;

/// Starts the game loop on its own thread.  `from_net` is the receiving
/// end of the queue the Launcher made; the thread keeps it.  If the loop
/// is already running, this does nothing and the receiver is dropped.  An
/// `Err` says why the world couldn't be loaded, or the thread couldn't be
/// started.
///
/// The world is loaded here, on the caller's thread, so that a world that
/// won't load is an `Err` the Launcher can show, and not a crash in a
/// thread nobody is watching.
pub fn start(from_net: Receiver<GameMessage>) -> Result<(), String> {
    let mut guard = THREAD.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_some() {
        return Ok(());
    }

    let voxel_world = world::load()?;

    STOP.store(false, Ordering::SeqCst);
    match thread::Builder::new().name("game-tick".to_string()).spawn(move || run(from_net, voxel_world)) {
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
fn run(from_net: Receiver<GameMessage>, voxel_world: VoxelWorld) {
    let mut world = World::new();
    let mut players: Players = HashMap::new();
    let mut clock = Clock::start();
    let mut stats = TickStats::default();
    let chatters = chat_room::open(&mut world);

    // One look at the middle of the map before it goes in, so the log
    // shows the loaded blocks are where the generator put them.
    scribe::info(Channel::World, &whats_under_the_middle(&voxel_world));
    world.insert_resource(voxel_world);

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
        run_tick(&mut world, &tick, &chatters, &mut players, &from_net);
        stats.record(began.elapsed(), tick.clock_reset);
    }

    // Networking stopped before us, so no Left is coming for anybody still
    // in.  They get saved here, the same way a Left would.
    for (account, stay) in players.drain() {
        leave(&mut world, &account, &stay);
    }

    scribe::info(Channel::World, &stats.summary());
}

/// One tick's work.  The queue first, so a player who arrived during the
/// last tick is in the world before anything runs.  Then whatever runs
/// every tick, and whatever runs once a round under its sub-tick.
// TODO(tick-work): nothing real runs on the tick yet.  Movement and combat
// every tick, and the rest spread across the sub-tick slots.
fn run_tick(world: &mut World,
            tick: &Tick,
            chatters: &[Entity],
            players: &mut Players,
            from_net: &Receiver<GameMessage>) {
    take_messages(world, players, from_net);
    chat_room::speak(world, chatters, tick.number);
}

/// What the middle of the map is standing on, for the log: the top solid
/// block in that column and its name.
fn whats_under_the_middle(voxel_world: &VoxelWorld) -> String {
    let header = &voxel_world.header;
    let side = CHUNK_SIDE as i32;
    let x = (header.first_chunk[0] + header.last_chunk[0] + 1) * side / 2;
    let z = (header.first_chunk[2] + header.last_chunk[2] + 1) * side / 2;
    let sky = (header.last_chunk[1] + 1) * side;

    match top_solid_block(voxel_world, x, z, sky) {
        Some((y, block)) => {
            let name = voxel_world.blocks.name_of(block).unwrap_or("something unnamed");
            format!("The middle of the map, ({}, {}), stands on {} at y = {}.", x, z, name, y)
        }
        None => format!("The middle of the map, ({}, {}), has nothing under it at all.", x, z),
    }
}

/// The highest block that isn't air in a column, looking down from
/// `from`, and its height.  `None` if the column is air all the way down
/// to the bottom of the world.
fn top_solid_block(voxel_world: &VoxelWorld, x: i32, z: i32, from: i32) -> Option<(i32, BlockId)> {
    let bottom = voxel_world.header.first_chunk[1] * CHUNK_SIDE as i32;
    for y in (bottom..from).rev() {
        let block = voxel_world.block_at(x, y, z);
        if block != AIR {
            return Some((y, block));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The queue from the network
// ---------------------------------------------------------------------------

/// Empties the queue.  Everything the network put on it since the last
/// tick gets done now, in the order it arrived.  There is no cap: a
/// message is one small file at most, and logins arrive one at a time
/// anyway, since the password check is one at a time.
fn take_messages(world: &mut World, players: &mut Players, from_net: &Receiver<GameMessage>) {
    loop {
        match from_net.try_recv() {
            Ok(GameMessage::Entered { account, account_uid, character, uuid, address }) => {
                enter(world, players, account, account_uid, character, uuid, address);
            }
            Ok(GameMessage::Left { account, address }) => {
                // A Left for an address we don't have is for a stay that
                // was already replaced (see enter()), and is nothing.
                let ours = players.get(&account)
                    .map(|stay| stay.address == address)
                    .unwrap_or(false);
                if ours {
                    if let Some(stay) = players.remove(&account) {
                        leave(world, &account, &stay);
                    }
                }
            }
            // Rust note: Empty means nothing more this tick.  Disconnected
            // means the network has dropped its end, which it does when it
            // stops, and after that there is never anything to take.
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return,
        }
    }
}

/// Puts a player into the world from their player file, and notes where
/// their packets come from.  A file that won't load gets an Error in the
/// log and no entity: the connection thread read the same file moments
/// ago, so this is something going wrong between the two reads.
fn enter(world: &mut World,
         players: &mut Players,
         account: String,
         account_uid: String,
         character: String,
         uuid: String,
         address: SocketAddr) {
    // The same account already in.  A player who takes their own session
    // over can get here before the sweep sends the old stay's Left.  The
    // old stay ends first, saved like any other.
    if let Some(old) = players.remove(&account) {
        scribe::info(Channel::World,
                     &format!("{} is coming in again, so their earlier stay as {} ends first.",
                              account,
                              account::display_name(&old.character)));
        leave(world, &account, &old);
    }

    let shown = account::display_name(&character);
    let file = match player_file::load(&account, &character, &uuid) {
        Ok(Some(file)) => file,
        Ok(None) => {
            scribe::error(Channel::World,
                          &format!("{} has no player file, so {} never got into the world.",
                                   shown,
                                   account));
            return;
        }
        // load() has already logged what is wrong with it.
        Err(_) => {
            scribe::error(Channel::World,
                          &format!("{}'s player file is damaged, so {} never got into the world.",
                                   shown,
                                   account));
            return;
        }
    };

    let entity = player_file::spawn(world, &file, &account, &account_uid);
    // The file doesn't hold these; the network does.  last_ip stays None,
    // since nothing saves it yet.
    if let Some(mut player) = world.get_mut::<Player>(entity) {
        player.current_ip = Some(address.ip());
        player.udp_port = Some(address.port());
    }

    scribe::info(Channel::World,
                 &format!("{} came into the world as {}, at ({:.1}, {:.1}, {:.1}).",
                          account,
                          shown,
                          file.position.x,
                          file.position.y,
                          file.position.z));
    players.insert(account, Stay { entity, character, address });
}

/// Takes a player out of the world: read back, saved, despawned.  The
/// save goes through `write_later()`, one file, so the tick never waits on
/// the disk.  A player that can't be read back is despawned anyway, with
/// an Error in the log, since there is nothing to save.
fn leave(world: &mut World, account: &str, stay: &Stay) {
    let shown = account::display_name(&stay.character);
    match player_file::read_back(world, stay.entity) {
        Some(file) => {
            if let Err(problem) = player_file::save(&file, account) {
                scribe::error(Channel::World,
                              &format!("{} on {} couldn't be saved on the way out: {}",
                                       shown,
                                       account,
                                       problem));
            }
        }
        None => {
            scribe::error(Channel::World,
                          &format!("{} on {} wasn't a whole player any more, so nothing was saved.",
                                   shown,
                                   account));
        }
    }
    world.despawn(stay.entity);
    scribe::info(Channel::World, &format!("{} ({}) left the world.", account, shown));
}

// ---------------------------------------------------------------------------
// How the ticks went
// ---------------------------------------------------------------------------

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
// state.  Neither are enter() and leave(), which read and write player
// files through DiskMan, nor start(), which loads the world.  The Launcher
// is their test.  What's tested is the counting, and the look at the
// middle of the map, on a world generated in memory.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::GRASS;
    use crate::world::GROUND_LEVEL;

    #[test]
    fn the_middle_of_the_map_stands_on_grass() {
        let voxel_world = VoxelWorld::generate(1, 1);
        let (y, block) = top_solid_block(&voxel_world, 128, 128, 128).unwrap();
        assert_eq!(block, GRASS);
        assert!((GROUND_LEVEL - 1..=GROUND_LEVEL + 1).contains(&y));

        let line = whats_under_the_middle(&voxel_world);
        assert!(line.starts_with("The middle of the map, (128, 128), stands on grass at y = "), "{}", line);
    }

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