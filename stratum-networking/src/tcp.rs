//! File:     stratum-networking/src/tcp.rs
//! Project:  Stratum Networking
//! Author:   Jacob Chacko
//!
//! The TCP side.  It listens, gives each connection a thread of its own,
//! and runs the TLS handshake on it.  Then it hangs up, because there is no
//! protocol yet to say what comes next.
//!
//! The listener is one thread that sits inside accept() and wakes up once
//! per connection.  Stopping it is the one awkward part: accept() blocks
//! until somebody connects, so stop() sets a flag and then connects to the
//! listener itself, which is the "somebody".
//!
//! Each connection thread waits on a read for READ_WAIT at a time, so it
//! wakes up often enough to notice the deadline and the stop flag.  TLS
//! doesn't mind being woken up in the middle of a handshake.  We measured
//! 10,000 of those timeouts across 50 connections without one failure.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rustls::{ServerConfig, ServerConnection};

use stratum_tools::constellations;
use stratum_tools::scribe::{self, Channel};

use crate::tls;

/// The most connections open at once.  50 players, plus room for a rush of
/// reconnects after a restart.  A guess.
const MAX_CONNECTIONS: usize = 100;

/// How long a new connection gets to finish TLS (and, once there is one, to
/// log in).  Without it somebody could open 100 connections and sit there.
/// A guess.
const LOGIN_DEADLINE: Duration = Duration::from_secs(10);

/// How long a connection thread waits on a read before it looks around.
/// Measured against 100 ms in Session 9, and kept.
const READ_WAIT: Duration = Duration::from_millis(50);

/// How long stop() waits for the connection threads to notice.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// The running TCP side.  Held in TCP below while the server is started.
struct TcpSide {
    /// Set by stop().  The listener checks it after every accept(), and the
    /// connection threads every time a read gives up.
    stopping: Arc<AtomicBool>,
    /// What we are listening on, so stop() knows where to knock.
    address: SocketAddr,
    listener: JoinHandle<()>,
    /// How many connection threads are running.
    open: Arc<AtomicUsize>,
}

// Rust note: the same shape Constellations uses for its global.  `None`
// means the TCP side isn't running.
static TCP: Mutex<Option<TcpSide>> = Mutex::new(None);

/// Loads the TLS certificate and key, binds the address and port from the
/// config file, and starts the listener thread.  An `Err` says what went
/// wrong, in words, and nothing is running.
pub fn start() -> Result<(), String> {
    let mut guard = TCP.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_some() {
        return Err("The TCP side is already running.".to_string());
    }

    // TLS first, so a missing or broken key stops us before anything is
    // listening.
    let tls = tls::server_config()?;

    let settings = constellations::get();
    let address = SocketAddr::new(settings.tcp_host_address, settings.tcp_port);
    let listener = TcpListener::bind(address)
        .map_err(|error| format!("Couldn't listen on {}: {}", address, error))?;

    let stopping = Arc::new(AtomicBool::new(false));
    let open = Arc::new(AtomicUsize::new(0));
    let flag = Arc::clone(&stopping);
    let count = Arc::clone(&open);
    let handle = thread::Builder::new()
        .name("tcp-listener".to_string())
        .spawn(move || listen(listener, tls, flag, count))
        .map_err(|error| format!("Couldn't start the TCP listener thread: {}", error))?;

    scribe::info(Channel::NetTcp, &format!("Listening on {}.", address));
    *guard = Some(TcpSide { stopping, address, listener: handle, open });
    Ok(())
}

/// Stops the listener and the connection threads, and waits for them to
/// finish.  Does nothing if the TCP side isn't running.
pub fn stop() {
    // Take the side out of the global first, so the lock isn't held while
    // we wait on the threads.
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

    // The connection threads see the same flag within READ_WAIT.  Wait for
    // them, but not forever.
    let started = Instant::now();
    loop {
        let still_open = side.open.load(Ordering::SeqCst);
        if still_open == 0 {
            break;
        }
        if started.elapsed() >= STOP_WAIT {
            scribe::warn(Channel::NetTcp,
                         &format!("{} connection(s) still open after {} seconds.  Stopping anyway.",
                                  still_open, STOP_WAIT.as_secs()));
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    scribe::info(Channel::NetTcp, "Stopped listening.");
}

/// The listener thread.  Each connection gets a log line and a thread of
/// its own, unless MAX_CONNECTIONS are already open.
fn listen(listener: TcpListener, tls: Arc<ServerConfig>, stopping: Arc<AtomicBool>,
          open: Arc<AtomicUsize>) {
    // Set once we have said we are full, so a flood gets one Warn and not
    // one per connection.
    let mut said_full = false;

    loop {
        match listener.accept() {
            Ok((stream, peer)) => {
                if stopping.load(Ordering::SeqCst) {
                    return;
                }
                if open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    if !said_full {
                        said_full = true;
                        scribe::warn(Channel::NetTcp,
                                     &format!("{} connections are open, which is the most we take.  \
                                     Turning new ones away.", MAX_CONNECTIONS));
                    }
                    drop(stream);
                    continue;
                }
                said_full = false;

                scribe::info(Channel::NetTcp, &format!("Connection from {}.", peer));
                let counted = OpenConnection::new(&open);
                let tls = Arc::clone(&tls);
                let flag = Arc::clone(&stopping);
                let spawned = thread::Builder::new()
                    .name(format!("tcp-conn-{}", peer))
                    .spawn(move || {
                        // Rust note: this moves `counted` into the thread,
                        // so it is dropped (and the count goes down) when
                        // the thread ends.
                        let _counted = counted;
                        connection(stream, peer, tls, flag);
                    });
                // If the thread won't start, the closure is thrown away, and
                // `counted` with it, so the count still comes out right.
                if let Err(error) = spawned {
                    scribe::warn(Channel::NetTcp,
                                 &format!("Couldn't start a thread for {}: {}.", peer, error));
                }
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

/// Counts one open connection for as long as it is held.
// Rust note: `Drop` is code that runs when a value goes away.  Here that is
// when the connection thread ends, whether it returns normally, returns
// early, or panics.  So there is no way out of a connection that forgets to
// take it off the count.
struct OpenConnection(Arc<AtomicUsize>);

impl OpenConnection {
    fn new(open: &Arc<AtomicUsize>) -> OpenConnection {
        open.fetch_add(1, Ordering::SeqCst);
        OpenConnection(Arc::clone(open))
    }
}

impl Drop for OpenConnection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One connection's thread.  Runs the TLS handshake, then hangs up.
fn connection(mut socket: TcpStream, peer: SocketAddr, tls: Arc<ServerConfig>, stopping: Arc<AtomicBool>) {
    let started = Instant::now();

    if let Err(error) = socket.set_read_timeout(Some(READ_WAIT)) {
        scribe::warn(Channel::NetTcp,
                     &format!("Couldn't set a read timeout for {}: {}.  Closing it.", peer, error));
        return;
    }
    // Small messages go out straight away, instead of being held back to
    // be sent together.
    let _ = socket.set_nodelay(true);

    let mut session = match ServerConnection::new(tls) {
        Ok(session) => session,
        Err(error) => {
            scribe::warn(Channel::NetTcp,
                         &format!("Couldn't start TLS for {}: {}.  Closing it.", peer, error));
            return;
        }
    };

    // Rust note: complete_io() does whatever reading and writing the
    // handshake needs next.  When a read gives up after READ_WAIT it comes
    // back as an error, and we go round again.
    while session.is_handshaking() {
        if stopping.load(Ordering::SeqCst) {
            return;
        }
        if started.elapsed() >= LOGIN_DEADLINE {
            scribe::info(Channel::NetTcp,
                         &format!("{} didn't finish TLS in {} seconds.  Closing it.",
                                  peer, LOGIN_DEADLINE.as_secs()));
            return;
        }
        match session.complete_io(&mut socket) {
            Ok(_) => {}
            Err(error) if timed_out(&error) => {}
            Err(error) => {
                scribe::info(Channel::NetTcp, &format!("TLS with {} failed: {}.", peer, error));
                return;
            }
        }
    }

    scribe::info(Channel::NetTcp,
                 &format!("TLS with {} is up, in {} ms.", peer, started.elapsed().as_millis()));

    // TODO(protocol): the read-wait loop goes here.  Read for READ_WAIT,
    // hand anything up to the game loop, send anything it queued, round
    // again.  Until there is a protocol, we say goodbye properly and close.
    session.send_close_notify();
    while session.wants_write() {
        if session.write_tls(&mut socket).is_err() {
            break;
        }
    }
}

/// A read that gave up after READ_WAIT.  Linux calls it WouldBlock, and
/// some other systems call it TimedOut.
fn timed_out(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
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