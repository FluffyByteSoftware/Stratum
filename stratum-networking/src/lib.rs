//! File:     stratum-networking/src/lib.rs
//! Project:  Stratum Networking
//! Author:   Jacob Chacko
//!
//! The part of Stratum that talks to the network.  Two sides: TCP for
//! logins, character select, chat and anything else that has to arrive,
//! and UDP for game traffic.  The Launcher's S) starts and stops both from
//! here.
//!
//! The network never touches game state directly.  Its threads hand
//! messages to the game loop through queues and take the answers back the
//! same way.  That is the one rule that keeps a slow client from slowing
//! the simulation.
//!
//! And this crate never depends on stratum-game.  Where it needs the
//! game's code (making, deleting and checking a character), the Launcher,
//! which can see both, hands it in at start().

use stratum_tools::account::Account;

pub mod tcp;
mod tls;
mod protocol;
mod sessions;

/// The game's character functions, handed in by the Launcher so this crate
/// can call them without knowing where they live.  Each `Err` is a message
/// for the player.
// Rust note: a field whose type is written like a function signature holds
// a function, and `(calls.create)(&mut account, name)` calls it.  The
// brackets are needed, or Rust goes looking for a method called create.
#[derive(Clone, Copy)]
pub struct CharacterCalls {
    /// Makes a character on the account, and hands back its UUID.
    pub create: fn(&mut Account, &str) -> Result<String, String>,
    /// Deletes a character from the account, file and all.
    pub delete: fn(&mut Account, &str) -> Result<(), String>,
    /// Says whether a character on the account can be played: it is there,
    /// and its file can be trusted.
    pub check: fn(&Account, &str) -> Result<(), String>,
}

/// Starts everything that listens.  An `Err` says, in words, what couldn't
/// be started, and nothing is left running.
pub fn start(calls: CharacterCalls) -> Result<(), String> {
    tcp::start(calls)
    // TODO(udp): the UDP side starts here too, once it exists.
}

/// Stops everything that listens, and waits until it has.  Safe to call
/// when nothing was started.
pub fn stop() {
    tcp::stop();
}