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
//! messages to the game loop through a queue (GameMessage, below), and the
//! loop empties it once a tick.  That is the one rule that keeps a slow
//! client from slowing the simulation.
//!
//! And this crate never depends on stratum-game.  Where it needs the
//! game's code (making, deleting, checking and listing characters), the
//! Launcher, which can see both, hands it in at start().  The queue goes
//! the other way: the Launcher makes it, hands this crate the sending end,
//! and hands the game loop the receiving end.

use std::net::SocketAddr;
use std::sync::mpsc::Sender;

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

/// A note from the network to the game loop.  The UDP thread puts one on
/// the queue when something happens to a player, and the loop takes them
/// all at the top of its next tick.  The loop only ever sees a player;
/// which account, connection and address belong to them stays in here.
///
/// Two kinds so far.  Movement, chat and combat come later, on the same
/// queue.
// Rust note: an enum where each variant carries its own fields.  In C#
// this would be an abstract record with a subclass per kind, and the loop
// does a `match` on it, one arm per kind.
#[derive(Debug, Clone, PartialEq)]
pub enum GameMessage {
    /// A player's UDP Connect was let in.  The loop reads their player
    /// file, puts them into the world, and fills in where their packets
    /// come from.  Everything the loop needs to do that is here, so it
    /// touches one file and nothing else.
    Entered {
        /// The account's username, lowercase.
        account: String,
        /// The account's UUID, for the Player component.
        account_uid: String,
        /// The character's short name, lowercase.
        character: String,
        /// The character's UUID, which the player file has to agree with.
        uuid: String,
        /// Where the player's UDP packets come from.
        address: SocketAddr,
    },
    /// A player has gone: quiet too long, logged out, or logged out from
    /// somewhere else.  The loop reads them back, saves them and takes
    /// them out.  The address says which stay this is for: a player who
    /// took their own session over can be Entered again before the old
    /// stay's Left arrives, and the loop leaves the new one alone.
    Left {
        account: String,
        address: SocketAddr,
    },
}

/// Starts both sides.  `to_game` is the sending end of the queue the
/// Launcher made; the UDP thread keeps it.
pub fn start(calls: CharacterCalls, to_game: Sender<GameMessage>) -> Result<(), String> {
    // The client list first, so a bad one stops us before anything listens.
    client_version::check()?;
    tcp::start(calls)?;
    // If UDP won't start, TCP comes back down, so nothing is left running.
    if let Err(error) = udp::start(to_game) {
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