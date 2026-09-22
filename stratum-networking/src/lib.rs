//! File:     stratum-networking/src/lib.rs
//! Project:  Stratum Networking
//! Author:   Jacob Chacko
//!
//! The part of Stratum that talks to the network.  Two sides: TCP for
//! logins, and for chat and anything else that has to arrive, and UDP for
//! game traffic.  The Launcher's S) starts and stops both from here.
//!
//! The network never touches game state directly.  Its threads hand
//! messages to the game loop through queues and take the answers back the
//! same way.  That is the one rule that keeps a slow client from slowing
//! the simulation.

pub mod tcp;

/// Starts everything that listens.  An `Err` says, in words, what couldn't
/// be started, and nothing is left running.
pub fn start() -> Result<(), String> {
    tcp::start()
    // TODO(udp): the UDP side starts here too, once it exists.
}

/// Stops everything that listens, and waits until it has.  Safe to call
/// when nothing was started.
pub fn stop() {
    tcp::stop();
}