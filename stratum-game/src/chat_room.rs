//! File:     stratum-game/src/chat_room.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! A test, not a feature.  A room full of Agents with made-up names, taking
//! turns to talk: the first one speaks on tick 0, the next on tick 1, and
//! round again after the last.  Every speech is a line in the log:
//!
//! ```text
//! Aaajsdj says: Poop
//! Qorblex says: Poop
//! ```
//!
//! It's there to put something on the tick we can watch.  If a tick ever got
//! skipped, somebody would miss their turn, and the log would show it.
//!
//! At one line a tick, that's 20 lines a second, so it's for short runs.
//! `CHATTERS` at 0 turns it off.

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;

use bevy_ecs::prelude::{Entity, World};
use stratum_tools::account;
use stratum_tools::scribe::{self, Channel};

use crate::actor::{Actor, ActorName, Agent, Health, Position, Rotation};

/// How many Agents are in the room.  0 means no room at all.
const CHATTERS: usize = 0;

/// What everybody says.
const LINE: &str = "Poop";

/// Bytes it takes to make one name: one for the length, and one for each of
/// up to 12 letters.
const BYTES_PER_NAME: usize = 13;

/// Fills the room: spawns `CHATTERS` Agents, each with a name nobody else
/// in the room has, and hands them back in speaking order.
///
/// The names are random letters from /dev/urandom.  They are never put on
/// the character names list, so they can't block a player from a name.
pub fn open(world: &mut World) -> Vec<Entity> {
    let mut chatters = Vec::new();
    if CHATTERS == 0 {
        return chatters;
    }

    let mut random = match File::open("/dev/urandom") {
        Ok(file) => file,
        Err(error) => {
            scribe::warn(Channel::World, &format!("No chat room: /dev/urandom couldn't be opened: {}", error));
            return chatters;
        }
    };

    // Two the same is very unlikely, but it would spoil the turns, so a
    // repeat gets thrown away and rolled again.  The limit is so a broken
    // /dev/urandom can't keep us here forever.
    let mut taken = HashSet::new();
    let mut tries = 0;
    while chatters.len() < CHATTERS && tries < CHATTERS * 10 {
        tries += 1;
        let mut bytes = [0u8; BYTES_PER_NAME];
        if let Err(error) = random.read_exact(&mut bytes) {
            scribe::warn(Channel::World, &format!("/dev/urandom stopped giving bytes: {}", error));
            break;
        }

        let name = name_from_bytes(&bytes);
        if !taken.insert(name.clone()) {
            continue;
        }

        let entity = world.spawn((
            Actor,
            ActorName {
                longname: account::display_name(&name),
                shortname: name,
            },
            Position::default(),
            Rotation::default(),
            Health::new(100, 100),
            Agent,
        )).id();
        chatters.push(entity);
    }

    scribe::info(Channel::World, &format!("{} Agent(s) are in the chat room.", chatters.len()));
    chatters
}

/// Whoever's turn it is says their line.  Called once a tick.
pub fn speak(world: &World, chatters: &[Entity], tick_number: u64) {
    if chatters.is_empty() {
        return;
    }

    let speaker = chatters[whose_turn(chatters.len(), tick_number)];
    if let Some(name) = world.get::<ActorName>(speaker) {
        scribe::info(Channel::World, &format!("{} says: {}", name.longname, LINE));
    }
}

/// Which chatter speaks on this tick, counting from 0.
fn whose_turn(count: usize, tick_number: u64) -> usize {
    (tick_number % count as u64) as usize
}

/// Makes a name out of random bytes: the first byte picks a length of 4 to
/// 12, and the rest pick the letters.  Lowercase, like every name we store.
// Rust note: `% 26` can favour a few letters very slightly, since 256
// doesn't divide by 26.  For gibberish names that doesn't matter.
fn name_from_bytes(bytes: &[u8; BYTES_PER_NAME]) -> String {
    let length = 4 + (bytes[0] % 9) as usize;
    bytes[1..=length].iter()
        .map(|byte| (b'a' + byte % 26) as char)
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names;

    #[test]
    fn names_follow_the_character_name_rules() {
        // Every length the first byte can pick, with every letter in play.
        for first in 0..=255u8 {
            let mut bytes = [0u8; BYTES_PER_NAME];
            bytes[0] = first;
            for (index, byte) in bytes.iter_mut().enumerate().skip(1) {
                *byte = first.wrapping_mul(7).wrapping_add(index as u8);
            }

            let name = name_from_bytes(&bytes);
            assert_eq!(names::check_character_name(&name), Ok(name.clone()));
        }
    }

    #[test]
    fn the_turns_go_round_in_order() {
        let turns: Vec<usize> = (0..7).map(|tick| whose_turn(3, tick)).collect();
        assert_eq!(turns, vec![0, 1, 2, 0, 1, 2, 0]);
    }
}