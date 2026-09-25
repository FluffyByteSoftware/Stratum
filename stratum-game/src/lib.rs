//! File:     stratum-game/src/lib.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! The game.  Everything that lives in the world starts here.  So far that
//! is the actors (actor.rs), the player file each character is saved in
//! (player_file.rs), the character names (names.rs), making and
//! deleting a character (character.rs), and the game loop that owns the
//! world and runs the tick (game_loop.rs), and the world itself: chunks of
//! blocks (chunk.rs), what the blocks are (blocks.rs), how a region of
//! them is saved (region_file.rs), and the whole of it (world.rs).
//!
//! main() calls `names::start()` at launch, after `account::start()`, so
//! the game knows which character names are taken.

pub mod actor;
pub mod character;
pub mod names;
pub mod player_file;
pub mod game_loop;
pub mod chunk;
pub mod blocks;
pub mod region_file;
pub mod world;


// For testing purposes 
mod chat_room;
