//! File:     src/main.rs
//! Project:  Stratum Core
//! Author:   Jacob Chacko
//!
//! Entry point.  Core is the driver: it starts up, brings the other pieces
//! online in order, and gets the game ready for play.  Right now the pieces
//! are Scribe, Constellations, Security, Account, and DiskMan.

// Rust note: `mod scribe;` tells the compiler that src/scribe.rs is part of
// this program.  A file that isn't named like this doesn't get compiled at
// all, no matter what folder it is sitting in.
mod scribe;
mod constellations;
mod diskman;
mod security;
mod account;

use scribe::{Channel, ScribeConfig};

/// The main entry point for the server.  This is where the program starts
/// running.  It is the first function called, and the last one to return.
fn main() {
    // Scribe first, so that everything after it has somewhere to complain.
    scribe::start();

    // Then DiskMan's writer thread, so anything that saves from here on has
    // somewhere to put it.
    diskman::start();
    
    // Then the settings.  Scribe had to come up on its built-in defaults,
    // because it has to be there before Constellations is.  Now that the
    // config file is loaded, Scribe gets the real ones.
    constellations::load();
    initialize_scribe();

    let settings = constellations::get();
    scribe::info(Channel::Core, &format!("TCP will listen on {}:{}",
                                         settings.tcp_host_address, settings.tcp_port));


    // Which usernames and character names are already taken.  If the account
    // folder can't be read we can't tell, and nothing after this is safe.
    if !account::start() {
        scribe::error(Channel::Core, "Can't go on without the account folder.  Shutting down.");
        constellations::save();
        diskman::stop();
        std::process::exit(1);
    }

    scribe::info(Channel::Core, "Shutting down...");
    
    // Last thing on the way out: whatever settings are in memory go back to
    // the config file.
    constellations::save();

    // And the very last thing: DiskMan writes whatever it is still holding
    // and waits until it is on the disk.
    diskman::stop();
}

/// Hands Scribe its settings out of Constellations.  This lives here and not
/// in scribe.rs so that Scribe never has to know Constellations exists.
fn initialize_scribe() {
    let settings = constellations::get();

    scribe::initialize(ScribeConfig {
        show_debug: settings.show_debug,
        color_debug: settings.color_debug,
        color_info: settings.color_info,
        color_warn: settings.color_warn,
        color_error: settings.color_error,
        log_dir: settings.log_folder,
        // The config file talks in megabytes and Scribe counts in bytes.
        // `saturating_mul` stops at the biggest number there is instead of
        // wrapping around, in case somebody types a silly size.
        max_file_bytes: settings.max_log_size_mb.saturating_mul(1024 * 1024),
    });
}