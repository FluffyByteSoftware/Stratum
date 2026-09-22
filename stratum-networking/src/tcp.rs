//! File:     stratum-networking/src/tcp.rs
//! Project:  Stratum Networking
//! Author:   Jacob Chacko
//!
//! The TCP side.  It listens, gives each connection a thread of its own,
//! runs the TLS handshake on it, and then the login.  A player who gets in
//! keeps the connection for as long as they are on.
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
//!
//! The login goes in the order docs/PROTOCOL.md gives: we say Hello, the
//! client says the secret word, we say we are waiting, the client sends a
//! username and password, and we answer.  Anything else, in any other
//! order, is a failure.  Every failure gets the same answer, closes the
//! connection, and makes that address wait FAILURE_HOLD before its next
//! try.
//!
//! The login happens here, on the connection's own thread.  It touches the
//! account files and nothing in the game, so the rule about never touching
//! game state still holds.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rustls::{ServerConfig, ServerConnection, StreamOwned};

use stratum_tools::scribe::{self, Channel};
use stratum_tools::{account, constellations, security};

use crate::protocol::{self, Packet, PacketType};
use crate::tls;

/// The most connections open at once.  50 players, plus room for a rush of
/// reconnects after a restart.  A guess.
const MAX_CONNECTIONS: usize = 100;

/// How long a new connection gets to finish TLS and log in.  Without it
/// somebody could open 100 connections and sit there.  A guess.
const LOGIN_DEADLINE: Duration = Duration::from_secs(10);

/// How long a connection thread waits on a read before it looks around.
/// Measured against 100 ms in Session 9, and kept.
const READ_WAIT: Duration = Duration::from_millis(50);

/// How long a write can take before we give up on the client.  A client
/// that stops reading would otherwise hold its thread forever once the
/// socket's buffer fills up.  A guess.
const WRITE_WAIT: Duration = Duration::from_secs(5);

/// How long stop() waits for the connection threads to notice.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// How long an address waits after a failed login before it gets to try
/// again, counted from the failure.
const FAILURE_HOLD: Duration = Duration::from_secs(2);

/// How much we ask TLS for in one read.  Bigger than any packet we take,
/// so one read can hold a whole one.
const READ_CHUNK: usize = 8192;

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

/// When each address last failed a login.  Only the ones inside
/// FAILURE_HOLD matter, and the rest get cleared out as new failures come
/// in.
// Rust note: a HashMap can't be built before the program starts the way a
// Mutex around a `None` can.  LazyLock builds it the first time anybody
// touches it.
static RECENT_FAILURES: LazyLock<Mutex<HashMap<IpAddr, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The TLS connection once the handshake is done.  It reads and writes
/// like a socket, and does the encrypting and decrypting on the way.
type TlsStream = StreamOwned<ServerConnection, TcpStream>;

/// Where a connection has got to in the login.
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    /// We said Hello and are waiting for the secret word.
    SecretWord,
    /// We said we are waiting, for the username and password.
    Credentials,
    /// In.  Only the packets a logged-in player can send are taken.
    LoggedIn,
}

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

/// One connection's thread, from the first byte to the last.  Waits out
/// the hold if this address failed recently, runs the TLS handshake, then
/// hands over to converse() for the login and whatever comes after it.
fn connection(mut socket: TcpStream, peer: SocketAddr, tls: Arc<ServerConfig>, stopping: Arc<AtomicBool>) {
    // The hold comes before TLS, so an address that keeps getting it wrong
    // costs us a sleeping thread and no crypto.
    if !wait_out_hold(peer, &stopping) {
        return;
    }

    // The deadline starts after the hold, so the hold doesn't eat into it.
    let started = Instant::now();

    let timeouts = socket.set_read_timeout(Some(READ_WAIT))
        .and_then(|_| socket.set_write_timeout(Some(WRITE_WAIT)));
    if let Err(error) = timeouts {
        scribe::warn(Channel::NetTcp,
                     &format!("Couldn't set timeouts for {}: {}.  Closing it.", peer, error));
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

    let mut stream = StreamOwned::new(session, socket);
    converse(&mut stream, peer, started, &stopping);
    goodbye(&mut stream);
}

/// Everything after TLS: the login, then the logged-in player's packets,
/// until one side hangs up or the server stops.
fn converse(stream: &mut TlsStream, peer: SocketAddr, started: Instant, stopping: &AtomicBool) {
    if send(stream, &protocol::hello()).is_err() {
        return;
    }

    let mut stage = Stage::SecretWord;
    // Bytes that have arrived and aren't a whole packet yet.
    let mut incoming: Vec<u8> = Vec::new();
    let mut chunk = [0u8; READ_CHUNK];

    loop {
        if stopping.load(Ordering::SeqCst) {
            return;
        }
        if stage != Stage::LoggedIn && started.elapsed() >= LOGIN_DEADLINE {
            scribe::info(Channel::NetTcp,
                         &format!("{} didn't log in within {} seconds.  Closing it.",
                                  peer, LOGIN_DEADLINE.as_secs()));
            refuse(stream, peer);
            return;
        }

        match stream.read(&mut chunk) {
            Ok(0) => {
                scribe::info(Channel::NetTcp, &format!("{} hung up.", peer));
                return;
            }
            Ok(count) => incoming.extend_from_slice(&chunk[..count]),
            // TODO(game-loop): once there is a game loop, this is where
            // anything it queued for this player gets sent.
            Err(error) if timed_out(&error) => continue,
            Err(error) => {
                scribe::info(Channel::NetTcp, &format!("Lost {}: {}.", peer, error));
                return;
            }
        }

        // One read can hold part of a packet, or several.  Take out every
        // whole one, and leave the part for the next read to finish.
        loop {
            let packet = match protocol::take_packet(&mut incoming) {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(problem) => {
                    scribe::info(Channel::NetTcp,
                                 &format!("{} sent {}.  Closing it.", peer, problem));
                    if stage != Stage::LoggedIn {
                        refuse(stream, peer);
                    }
                    return;
                }
            };
            match handle(stream, peer, stage, &packet) {
                Some(next) => stage = next,
                None => return,
            }
        }
    }
}

/// Deals with one whole packet.  Hands back the stage to carry on in, or
/// `None` when the connection should close.
fn handle(stream: &mut TlsStream, peer: SocketAddr, stage: Stage, packet: &Packet) -> Option<Stage> {
    match (stage, PacketType::from_byte(packet.kind)) {
        (Stage::SecretWord, Some(PacketType::SecretWord)) => {
            match protocol::read_one_string(&packet.payload) {
                Ok(word) if word == protocol::SECRET_WORD => {
                    if send(stream, &protocol::awaiting_authentication()).is_err() {
                        return None;
                    }
                    Some(Stage::Credentials)
                }
                // What they sent instead isn't logged.  It could be anything.
                _ => {
                    scribe::info(Channel::NetTcp, &format!("{} didn't know the secret word.", peer));
                    refuse(stream, peer);
                    None
                }
            }
        }

        (Stage::Credentials, Some(PacketType::AuthenticationRequest)) => {
            // The clock starts the moment the attempt is in, and every path
            // below runs out the same floor before anything goes back.
            let arrived = Instant::now();
            let request = protocol::read_authentication_request(&packet.payload);
            let logged_in = match &request {
                Ok((username, password)) => log_in(username, password),
                Err(_) => false,
            };
            security::pad_login_time(arrived);

            match (&request, logged_in) {
                (Ok((username, _)), true) => {
                    scribe::info(Channel::Security,
                                 &format!("{} logged in as {}.", peer, name_for_log(username)));
                }
                (Ok((username, _)), false) => {
                    scribe::info(Channel::Security,
                                 &format!("Login from {} as {} failed.", peer, name_for_log(username)));
                }
                (Err(problem), _) => {
                    scribe::info(Channel::Security,
                                 &format!("{} sent a login we couldn't read: {}.", peer, problem));
                }
            }

            if !logged_in {
                refuse(stream, peer);
                return None;
            }
            if send(stream, &protocol::authentication_result(true, protocol::SUCCESS_MESSAGE)).is_err() {
                return None;
            }
            Some(Stage::LoggedIn)
        }

        (Stage::LoggedIn, Some(PacketType::SimpleTcpMesg)) => {
            match protocol::read_one_string(&packet.payload) {
                Ok(text) => {
                    // Rust note: `{:?}` prints the string in quotes, with any
                    // newline written as \n, so one message stays one log
                    // line whatever is in it.
                    scribe::info(Channel::NetTcp,
                                 &format!("{} sent {:?}.  Sending it back.", peer, text));
                    if send(stream, &protocol::simple_message(&text)).is_err() {
                        return None;
                    }
                    Some(Stage::LoggedIn)
                }
                Err(problem) => {
                    scribe::info(Channel::NetTcp,
                                 &format!("{} sent a SimpleTcpMesg with {}.  Closing it.", peer, problem));
                    None
                }
            }
        }

        // Somebody who is in can send a packet from a later group before we
        // know what to do with it.  That is our shortcoming, not theirs.
        (Stage::LoggedIn, _) => {
            scribe::info(Channel::NetTcp,
                         &format!("{} sent packet type 0x{:02X}, which we don't handle yet.  Ignored it.",
                                  peer, packet.kind));
            Some(Stage::LoggedIn)
        }

        // Anything else during the login is out of turn.
        (_, _) => {
            scribe::info(Channel::NetTcp,
                         &format!("{} sent packet type 0x{:02X} out of turn.", peer, packet.kind));
            refuse(stream, peer);
            None
        }
    }
}

/// Tries a username and password against the account files.  True if they
/// are right.  It doesn't pad the time -- the caller does that, on every
/// path, this one included.
fn log_in(username: &str, password: &str) -> bool {
    let mut account = match account::load_account(username) {
        Ok(Some(account)) => account,
        // No such account, a name that isn't allowed, or a damaged file.
        // All three are a plain failure, and none of them costs a hash.
        Ok(None) | Err(_) => return false,
    };

    if !security::verify_password(password, &account.password_hash_string) {
        return false;
    }

    // The password was right.  A login time we couldn't save isn't worth
    // turning the player away over.
    if let Err(error) = account::record_login(&mut account) {
        scribe::warn(Channel::Security,
                     &format!("Couldn't save the login time for {}: {}.  Letting them in anyway.",
                              account.username, error));
    }
    true
}

/// A username the way it goes in the log.  Only a name that follows the
/// username rules gets written down.  Anything else could be somebody's
/// password typed into the wrong box -- a password needs a symbol, and a
/// username can't have one -- or junk meant to mess up the log.
fn name_for_log(username: &str) -> String {
    match account::check_username(username) {
        Ok(name) => name,
        Err(_) => "a name that isn't allowed".to_string(),
    }
}

/// Sends the one failure answer and puts the address on hold.  Every way
/// out of a failed login comes through here.
fn refuse(stream: &mut TlsStream, peer: SocketAddr) {
    record_failure(peer.ip());
    let _ = send(stream, &protocol::authentication_result(false, protocol::FAILURE_MESSAGE));
}

/// Sends one packet's bytes and pushes them out onto the wire.
fn send(stream: &mut TlsStream, bytes: &[u8]) -> io::Result<()> {
    stream.write_all(bytes)?;
    stream.flush()
}

/// Says goodbye properly (TLS's `close_notify`), so the client knows we
/// closed on purpose and nothing was cut off.  The socket itself closes
/// when the stream goes away, straight after.
fn goodbye(stream: &mut TlsStream) {
    stream.conn.send_close_notify();
    while stream.conn.wants_write() {
        if stream.conn.write_tls(&mut stream.sock).is_err() {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// The hold
// ---------------------------------------------------------------------------

/// Notes that an address just failed a login.
fn record_failure(address: IpAddr) {
    let mut failures = RECENT_FAILURES.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Anything older than the hold is no use any more.  Clearing it out here
    // keeps the list from growing for as long as the server runs.
    failures.retain(|_, failed_at| failed_at.elapsed() < FAILURE_HOLD);
    failures.insert(address, Instant::now());
}

/// How much longer an address has to wait, or `None` if it doesn't.
fn hold_remaining(address: IpAddr) -> Option<Duration> {
    let failures = RECENT_FAILURES.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let failed_at = failures.get(&address)?;
    // Rust note: `checked_sub` is `None` when the hold is already over,
    // instead of going below zero.
    FAILURE_HOLD.checked_sub(failed_at.elapsed())
}

/// Sleeps until the address's hold is over, READ_WAIT at a time so a stop
/// isn't kept waiting.  False if the server stopped first.
fn wait_out_hold(peer: SocketAddr, stopping: &AtomicBool) -> bool {
    let Some(wait) = hold_remaining(peer.ip()) else {
        return true;
    };
    scribe::info(Channel::NetTcp,
                 &format!("{} failed a login less than {} seconds ago.  Holding it for {} ms.",
                          peer, FAILURE_HOLD.as_secs(), wait.as_millis()));

    let until = Instant::now() + wait;
    loop {
        if stopping.load(Ordering::SeqCst) {
            return false;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        thread::sleep(left.min(READ_WAIT));
    }
}

// ---------------------------------------------------------------------------
// Small pieces
// ---------------------------------------------------------------------------

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