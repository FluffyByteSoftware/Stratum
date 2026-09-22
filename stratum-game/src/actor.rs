//! File:     stratum-game/src/actor.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! Actors.  An actor is anything living in the world that can move: it has
//! a name, a place, a way it's facing, and health.  There are two kinds so
//! far.  A Player is driven by a person, and an Agent (an NPC) is driven by
//! the computer.
//!
//! My old mental model was Discworld's, where "living" was a base class and
//! players and NPCs both inherited from it.  This time the actors are held
//! in an entity component system (bevy_ecs).  An entity is just an id, and
//! the components here are the pieces of data that get attached to it.  So
//! there is no Actor class to inherit from.  An actor is an entity with
//! these components on it:
//!
//! ```text
//! Player:  Actor, ActorName, Position, Rotation, Health, Player
//! Agent:   Actor, ActorName, Position, Rotation, Health, Agent
//! ```
//!
//! "Is this a player?" becomes "does this entity have a Player on it?".
//!
//! Nothing in here knows about files.  What gets saved, and how, is the
//! player file's business.

use std::net::IpAddr;
use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};

/// Every actor has one of these, and nothing else does.  It holds nothing.
/// It is there so "every actor in the world" is one question to ask.
// Rust note: a struct with no fields and no braces is a "unit struct".  It
// takes up no memory at all, so a marker like this is free.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Actor;

/// An actor's name, two ways.  The shortname is the lowercase one the game
/// goes by, and it matches the player file (aldric.plyr is "aldric").  The
/// longname is what other players see, and it can carry titles and capitals
/// ("Aldric the Unwashed").
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ActorName {
    pub shortname: String,
    pub longname: String,
}

/// Where the actor is in the world.  The voxel it's standing in is the
/// whole-number part of each of these.
///
/// Just three numbers for now.  Nothing does any vector maths with them yet,
/// and when movement does, a maths crate gets measured then.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Which way the actor is facing, as a quaternion: four numbers that
/// describe a rotation without the problems angles have.  Godot uses the
/// same thing, so it can go to the client as it is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rotation {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

// A new actor faces straight ahead, which in a quaternion is w = 1 and the
// rest 0.  All zeroes wouldn't be a rotation at all, which is why this one
// isn't derived like Position's.
impl Default for Rotation {
    fn default() -> Rotation {
        Rotation { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }
    }
}

/// An actor's health.  It never goes above the most it can be, never goes
/// below 0, and the most it can be is never below 1.
///
/// The numbers can't be set straight.  Everything goes through the methods
/// below, so the rules can't be skipped by accident.  Anything that lowers
/// the health gets a HealthChange back, and a Died means the caller has to
/// deal with the death.
// Rust note: the fields have no `pub` in front of them, so nothing outside
// this file can touch them.  Same idea as private fields in C#.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Health {
    current: i32,
    max: i32,
}

/// What happened to an actor's health.  Died only comes back the one time
/// the health reaches 0.  Hitting a corpse again is just Updated.
// Rust note: `#[must_use]` means the compiler warns if a caller throws this
// away without looking at it.  That's on purpose.  Forgetting to check for a
// death is exactly the bug we don't want.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HealthChange {
    Updated,
    Died,
}

impl Health {
    /// Makes a health, kept inside the rules.  A player file that says 150
    /// out of 100 comes back as 100 out of 100.
    pub fn new(current: i32, max: i32) -> Health {
        let max = max.max(1);
        Health { current: current.clamp(0, max), max }
    }

    pub fn current(&self) -> i32 {
        self.current
    }

    pub fn max(&self) -> i32 {
        self.max
    }

    /// Sets the health outright, kept between 0 and the max.
    // TODO(death): whatever gets a Died back calls die(), which takes the
    // player's input away and respawns them.  Neither exists yet.
    pub fn set(&mut self, value: i32) -> HealthChange {
        let was_alive = self.current > 0;
        self.current = value.clamp(0, self.max);

        if was_alive && self.current == 0 {
            HealthChange::Died
        } else {
            HealthChange::Updated
        }
    }

    /// Moves the health up or down.  Damage is a negative number.
    pub fn adjust(&mut self, change: i32) -> HealthChange {
        // `saturating_add` stops at the biggest (or smallest) number there
        // is, instead of wrapping around, in case something hits for a
        // silly amount.
        self.set(self.current.saturating_add(change))
    }

    /// Sets the most the health can be.  Never below 1.  If the health is
    /// over the new max, it comes down to it.  Raising the max doesn't heal
    /// anybody.
    ///
    /// This can't kill, since the max is never below 1, so there's no
    /// HealthChange to hand back.
    pub fn set_max(&mut self, value: i32) {
        self.max = value.max(1);
        if self.current > self.max {
            self.current = self.max;
        }
    }

    /// Moves the most the health can be, up or down.
    pub fn adjust_max(&mut self, change: i32) {
        self.set_max(self.max.saturating_add(change));
    }
}

/// On an actor a person is driving.  The uuid is the character's own, the
/// same one the player file holds and the account points at.
///
/// Only the uuid gets saved.  The rest is filled in when the player logs in,
/// and the network parts stay empty (None) until they come in over the
/// network.  A character loaded from the Launcher never gets them.
// Rust note: `Option<IpAddr>` is either `Some(address)` or `None`.  It's how
// Rust says "might not be there", where C# would use null.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Player {
    /// The character's own UUID.
    pub uuid: String,
    /// The account's UUID (`account_uid` in the account file).
    pub account_uid: String,
    /// The account's username, lowercase.  The player file lives in a
    /// folder named after it, and the UUID alone can't find that.
    pub account: String,
    pub last_ip: Option<IpAddr>,
    pub current_ip: Option<IpAddr>,
    pub udp_port: Option<u16>,
}

/// On an actor the computer is driving.  Empty for now.  What an agent
/// thinks with comes later.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Agent;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_health_stays_inside_the_rules() {
        assert_eq!(Health::new(150, 100), Health::new(100, 100));
        assert_eq!(Health::new(-5, 100).current(), 0);
        assert_eq!(Health::new(10, 0).max(), 1);
        assert_eq!(Health::new(10, 0).current(), 1);
    }

    #[test]
    fn health_stays_between_zero_and_the_max() {
        let mut health = Health::new(50, 100);

        assert_eq!(health.adjust(500), HealthChange::Updated);
        assert_eq!(health.current(), 100);

        assert_eq!(health.set(-20), HealthChange::Died);
        assert_eq!(health.current(), 0);
    }

    #[test]
    fn a_death_only_comes_back_once() {
        let mut health = Health::new(10, 100);

        assert_eq!(health.adjust(-4), HealthChange::Updated);
        assert_eq!(health.adjust(-6), HealthChange::Died);
        assert_eq!(health.adjust(-6), HealthChange::Updated);
        assert_eq!(health.current(), 0);

        // Healed back up, it can die again.
        assert_eq!(health.set(30), HealthChange::Updated);
        assert_eq!(health.adjust(i32::MIN), HealthChange::Died);
    }

    #[test]
    fn the_max_never_drops_below_one_and_pulls_the_health_down() {
        let mut health = Health::new(80, 100);

        health.set_max(60);
        assert_eq!(health, Health::new(60, 60));

        health.set_max(200);
        assert_eq!(health, Health::new(60, 200));

        health.adjust_max(-1000);
        assert_eq!(health, Health::new(1, 1));
    }
}