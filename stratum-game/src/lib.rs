//! File:     stratum-game/src/lib.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! The game.  Everything that lives in the world starts here.  So far that
//! is the actors (actor.rs), the player file each character is saved in
//! (player_file.rs), the character names (names.rs), and making and
//! deleting a character (character.rs).  The world itself comes later.
//!
//! main() calls `names::start()` at launch, after `account::start()`, so
//! the game knows which character names are taken.

pub mod actor;
pub mod character;
pub mod names;
pub mod player_file;