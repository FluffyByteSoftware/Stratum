//! File:     stratum-networking/src/tcp.rs
//! Project:  Stratum Networking
//! Author:   Jacob Chacko
//!
//! The TCP side.  For now it listens, logs each connection, and hangs up.
//! That is enough for Probe to reach us.  TLS, the protocol, and a thread
//! per connection come in the next sessions.
//!
//! The listener is one thread that sits inside accept() and wakes up once
//! per connection.  Stopping it is the one awkward part: accept() blocks
//! until somebody connects, so stop() sets a flag and then connects to the
//! listener itself, which is the "somebody".

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use stratum_tools::constellations;
use stratum_tools::scribe::{self, Channel};

/// The running TCP side.  Held in TCP below while the server is started.
struct TcpSide {
    /// Set by stop().  The listener thread checks it after every accept().
    stopping: Arc<AtomicBool>,
    /// What we are listening on, so stop() knows where to knock.
    address: SocketAddr,
    listener: JoinHandle<()>,
}

// Rust note: the same shape Constellations uses for its global.  `None`
// means the TCP side isn't running.
static TCP: Mutex<Option<TcpSide>> = Mutex::new(None);

/// Binds the address and port from the config file and starts the listener
/// thread.  An `Err` says what went wrong, in words, and nothing is running.
pub fn start() -> Result<(), String> {
    let mut guard = TCP.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_some() {
        return Err("The TCP side is already running.".to_string());
    }

    let settings = constellations::get();
    let address = SocketAddr::new(settings.tcp_host_address, settings.tcp_port);
    let listener = TcpListener::bind(address)
        .map_err(|error| format!("Couldn't listen on {}: {}", address, error))?;

    let stopping = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stopping);
    let handle = thread::Builder::new()
        .name("tcp-listener".to_string())
        .spawn(move || listen(listener, flag))
        .map_err(|error| format!("Couldn't start the TCP listener thread: {}", error))?;

    scribe::info(Channel::NetTcp, &format!("Listening on {}.", address));
    *guard = Some(TcpSide { stopping, address, listener: handle });
    Ok(())
}

/// Stops the listener and waits for its thread to finish.  Does nothing if
/// it isn't running.
pub fn stop() {
    // Take the side out of the global first, so the lock isn't held while
    // we wait on the thread.
    let side = {
        let mut guard = TCP.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.take()
    };
    let Some(side) = side else {
        return;
    };

    side.stopping.store(true, Ordering::SeqCst);

    // Wake accept() by connecting to ourselves.  The listener sees the flag
    // and returns instead of logging us as a connection.
    let knock = wake_address(side.address);
    if let Err(error) = TcpStream::connect_timeout(&knock, Duration::from_secs(1)) {
        scribe::warn(Channel::NetTcp,
                     &format!("Couldn't wake the listener on {}: {}.", knock, error));
    }

    let _ = side.listener.join();
    scribe::info(Channel::NetTcp, "Stopped listening.");
}

/// The listener thread.  Each connection gets one log line and the door.
fn listen(listener: TcpListener, stopping: Arc<AtomicBool>) {
    loop {
        match listener.accept() {
            Ok((stream, peer)) => {
                if stopping.load(Ordering::SeqCst) {
                    return;
                }
                scribe::info(Channel::NetTcp, &format!("Connection from {}.", peer));
                // TODO(protocol): hand the connection its own thread instead
                // of closing it.
                drop(stream);
            }
            Err(error) => {
                if stopping.load(Ordering::SeqCst) {
                    return;
                }
                scribe::warn(Channel::NetTcp, &format!("Accept failed: {}.", error));
                // A failure that keeps happening (out of file handles, say)
                // would otherwise fill the log as fast as the disk allows.
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Where stop() connects to wake the listener.  Usually the listening
/// address itself.  But "listen on everything" (0.0.0.0, or ::) isn't an
/// address anybody can connect to, so that becomes localhost.
fn wake_address(address: SocketAddr) -> SocketAddr {
    if !address.ip().is_unspecified() {
        return address;
    }
    let localhost = match address.ip() {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
    };
    SocketAddr::new(localhost, address.port())
}