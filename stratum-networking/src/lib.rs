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
//! game's code (making, deleting, checking and listing characters), the
//! Launcher, which can see both, hands it in at start().

use stratum_tools::account::Account;

pub mod tcp;
mod tls;
mod protocol;
mod sessions;
mod client_version;
pub mod udp;

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
    /// Everything the character list shows about each character on the
    /// account, in the order they were made.  A character whose file is
    /// missing or damaged is still in it, marked as not playable, so the
    /// player can see it and delete it.
    pub list: fn(&Account) -> Vec<CharacterSummary>,
}

/// One character, the way the character list shows it.  The game fills it
/// in from the player file (through the Launcher), and protocol.rs turns it
/// into bytes.
// TODO(zones): the position becomes a zone and a position inside it, once
// there are zones.  That changes CharacterList's shape.
pub struct CharacterSummary {
    /// Lowercase.  What the client sends back to pick, delete or play it.
    pub shortname: String,
    /// What the player sees.  "Aldric", or "Aldric the Unwashed".
    pub longname: String,
    /// False when the player file is missing or can't be trusted.  The
    /// client tells the player to notify an admin.
    pub playable: bool,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub fn start(calls: CharacterCalls) -> Result<(), String> {
    // The client list first, so a bad one stops us before anything listens.
    client_version::check()?;
    tcp::start(calls)?;
    // If UDP won't start, TCP comes back down, so nothing is left running.
    if let Err(error) = udp::start() {
        tcp::stop();
        return Err(error);
    }
    Ok(())
}

/// Stops everything that listens, and waits until it has.  Safe to call
/// when nothing was started.
pub fn stop() {
    udp::stop();
    tcp::stop();
}