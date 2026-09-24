//! File:     stratum-launcher/src/main.rs
//! Project:  Stratum Core
//! Author:   Jacob Chacko
//!
//! Entry point.  Core is the driver: it starts up, brings the other pieces
//! online in order, and gets the game ready for play.  Right now the pieces
//! are Scribe, DiskMan, Constellations, Security's hashing worker, the
//! account, the game's character names, and the Launcher, in that order.
//! Fingerprinter has nothing to start.

// Rust note: `mod launcher;` tells the compiler that src/launcher.rs is part
// of this program.  The tools aren't named here any more -- they are their
// own crate now, and the `use` lines below reach into it.
mod launcher;

use stratum_tools::{account, constellations, diskman, scribe, security};
use stratum_tools::scribe::{Channel, ScribeConfig};

/// The main entry point for the server.  This is where the program starts
/// running.  It is the first function called, and the last one to return.
fn main() {
    // Scribe first, so that everything after it has somewhere to complain.
    scribe::start();

    // Ctrl-C kills the server on the spot, without saving the config or
    // letting DiskMan finish its writes.  Q is the way out, so Ctrl-C does
    // nothing from here on.
    ignore_ctrl_c();

    // Then DiskMan's writer thread, so anything that saves from here on has
    // somewhere to put it.
    diskman::start();

    // Then the settings.  Scribe had to come up on its built-in defaults,
    // because it has to be there before Constellations is.  Now that the
    // config file is loaded, Scribe gets the real ones.
    constellations::load();
    initialize_scribe();

    // The folders inside the content folder: logs, accounts, saved/ssl (which
    // only our user can open), saved/players and saved/orphaned/players.  Any
    // that are missing get made now, and any .tmp a crash left in them goes.
    constellations::make_folders();

    // Then Security's hashing worker, before anything can hash a password.
    // It only needs Scribe, for its complaints.
    security::start();

    let settings = constellations::get();
    scribe::info(Channel::Core, &format!("TCP is configured for {}:{}",
                                         settings.tcp_host_address, settings.tcp_port));


    // Which usernames are already taken.  If the account folder can't be
    // read we can't tell, and nothing after this is safe.
    if !account::start() {
        scribe::error(Channel::Core, "Can't go on without the account folder.  \
        Shutting down.");
        constellations::save();
        diskman::stop();
        std::process::exit(1);
    }

    // Then which character names are, which the game learns from the
    // accounts.  So it has to come after them.
    stratum_game::names::start();

    // The admin's menu.  It runs until they pick Q, and then we shut down.
    launcher::run();

    scribe::info(Channel::Core, "Shutting down...");

    // The hashing worker finishes anything still in line, then stops.  Q only
    // works with the server stopped, so nothing new can join the line.
    security::stop();

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
        log_dir: constellations::log_folder(),
        // The config file talks in megabytes and Scribe counts in bytes.
        // `saturating_mul` stops at the biggest number there is instead of
        // wrapping around, in case somebody types a silly size.
        max_file_bytes: settings.max_log_size_mb.saturating_mul(1024 * 1024),
    });
}


/// Tells the operating system to ignore Ctrl-C (SIGINT) for this program,
/// so the only way out is Q, which shuts down properly.  Ctrl-\ still kills
/// it, for when the menu itself is stuck.
// Rust note: the standard library has no way to ignore a signal, so this
// calls C's signal() from the C library Rust already links against.
// `unsafe` is because the compiler can't check a call into C.  The numbers
// are Linux's: SIGINT is 2, and handing it 1 (SIG_IGN) means ignore it.
// signal() hands back SIG_ERR, every bit set, if it refused.
fn ignore_ctrl_c() {
    const SIGINT: i32 = 2;
    const SIG_IGN: usize = 1;
    const SIG_ERR: usize = usize::MAX;

    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }

    let previous = unsafe { signal(SIGINT, SIG_IGN) };
    if previous == SIG_ERR {
        scribe::warn(Channel::Core, "Couldn't switch off Ctrl-C.  \
                                     It will still kill the server without saving.");
    }
}