//! File:     stratum-networking/src/tcp.rs
//! Project:  Stratum Networking
//! Author:   Jacob Chacko
//!
//! The TCP side.  It listens, gives each connection a thread of its own,
//! runs the TLS handshake on it, then the login, then character select.
//! A player who gets in keeps the connection for as long as they are on.
//!
//! The listener is one thread that sits inside accept() and wakes up once
//! per connection.  Stopping it is the one awkward part: accept() blocks
//! until somebody connects, so stop() sets a flag and then connects to the
//! listener itself, which is the "somebody".
//!
//! Each connection thread waits on a read for READ_WAIT at a time, so it
//! wakes up often enough to notice the deadline, the stop flag, and being
//! logged out from somewhere else.  TLS doesn't mind being woken up in the
//! middle of a handshake.  We measured 10,000 of those timeouts across 50
//! connections without one failure.
//!
//! The login goes in the order docs/PROTOCOL.md gives: we say Hello, the
//! client says the secret word, we say we are waiting, the client sends a
//! username and password, and we answer.  Anything else, in any other
//! order, is a failure.  Every failure gets the same answer, closes the
//! connection, and makes that address wait FAILURE_HOLD before its next
//! try.
//!
//! A right password for an account that is already on gets asked what to
//! do instead: log the other session out, or hang up.  sessions.rs keeps
//! the list of who is on.  A right password on a full server gets told so
//! and hung up on, with no hold, because it isn't a failed login.
//!
//! Then character select.  The player gets their list, and can make a
//! character, delete one, or pick one to play.  Picking one gets them a
//! login token and the UDP port to take it to.  Going into the world from
//! there waits for the UDP side and the game loop.
//!
//! The login and character select both happen here, on the connection's
//! own thread.  They touch account and player files and nothing in the
//! running game, so the rule about never touching game state still holds.
//! Making, deleting and checking a character are the game's code, and this
//! crate can't see the game, so the Launcher hands them in at start()
//! (CharacterCalls, in lib.rs).

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rustls::{ServerConfig, ServerConnection, StreamOwned};

use stratum_tools::account::{self, Account};
use stratum_tools::scribe::{self, Channel};
use stratum_tools::{constellations, security};

use crate::CharacterCalls;
use crate::protocol::{self, Choice, KickReason, Packet, PacketType};
use crate::sessions::{self, Claim, Refused};
use crate::tls;

/// The most connections open at once.  Jacob's number: room for
/// sessions::MAX_LOGGED_IN players, and a few more, so a full server can
/// still answer the next player and tell them so.
const MAX_CONNECTIONS: usize = 55;

/// How long a new connection gets to finish TLS and log in.  Without it
/// somebody could open every connection we take and sit there.  A guess.
const LOGIN_DEADLINE: Duration = Duration::from_secs(10);

/// How long a player gets to answer "this account is already logged in".
/// A person is reading a prompt, so it is longer than the login deadline.
/// A guess.
const CHOICE_DEADLINE: Duration = Duration::from_secs(30);

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

/// What a player sees when a character can't be played because something
/// on our side went wrong.  The details are in the log.
const OUR_FAULT: &str = "The server couldn't do that right now.  Tell an admin.";

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

/// What every connection thread needs, worked out once in start() and
/// shared.
struct Setup {
    tls: Arc<ServerConfig>,
    calls: CharacterCalls,
    /// Handed to a player along with their token.  Read once at start(),
    /// the same as the TCP port, so it can't change under a running server.
    udp_port: u16,
}

/// Where a connection has got to.
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    /// We said Hello and are waiting for the secret word.
    SecretWord,
    /// We said we are waiting, for the username and password.
    Credentials,
    /// The password was right and the account is already on.  Waiting for
    /// the player to say what to do about it.
    Choosing,
    /// In, with the character list.  Making, deleting and picking.
    CharacterSelect,
    /// A character is picked and the player has their token.
    InWorld,
}

impl Stage {
    /// True before the password has been checked.  A mistake here is a
    /// failed login, with the one failure answer and the hold.
    fn logging_in(self) -> bool {
        self == Stage::SecretWord || self == Stage::Credentials
    }
}

/// Everything one connection knows about itself.
struct Conversation {
    peer: SocketAddr,
    stage: Stage,
    /// The account, lowercase, once its password has been right.  Empty
    /// until then.
    username: String,
    /// This connection's hold on the account.  `None` until it is logged
    /// in for real, and while it waits in Choosing.
    claim: Option<Claim>,
    /// When the player was asked to choose, for CHOICE_DEADLINE.
    asked_at: Option<Instant>,
}

/// Loads the TLS certificate and key, binds the address and port from the
/// config file, and starts the listener thread.  An `Err` says what went
/// wrong, in words, and nothing is running.
pub fn start(calls: CharacterCalls) -> Result<(), String> {
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

    let setup = Arc::new(Setup { tls, calls, udp_port: settings.udp_port });
    let stopping = Arc::new(AtomicBool::new(false));
    let open = Arc::new(AtomicUsize::new(0));
    let flag = Arc::clone(&stopping);
    let count = Arc::clone(&open);
    let handle = thread::Builder::new()
        .name("tcp-listener".to_string())
        .spawn(move || listen(listener, setup, flag, count))
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
                         &format!("{} connection(s) still open after {} seconds.  \
                         Stopping anyway.",
                                  still_open, STOP_WAIT.as_secs()));
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    scribe::info(Channel::NetTcp, "Stopped listening.");
}

/// The listener thread.  Each connection gets a log line and a thread of
/// its own, unless MAX_CONNECTIONS are already open.
fn listen(listener: TcpListener,
          setup: Arc<Setup>,
          stopping: Arc<AtomicBool>,
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
                                     &format!("{} connections are open, which is the \
                                     most we take.  \
                                     Turning new ones away.", MAX_CONNECTIONS));
                    }
                    drop(stream);
                    continue;
                }
                said_full = false;

                scribe::info(Channel::NetTcp, &format!("Connection from {}.", peer));
                let counted = OpenConnection::new(&open);
                let setup = Arc::clone(&setup);
                let flag = Arc::clone(&stopping);
                let spawned = thread::Builder::new()
                    .name(format!("tcp-conn-{}", peer))
                    .spawn(move || {
                        // Rust note: this moves `counted` into the thread,
                        // so it is dropped (and the count goes down) when
                        // the thread ends.
                        let _counted = counted;
                        connection(stream, peer, setup, flag);
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
fn connection(mut socket: TcpStream, peer: SocketAddr, setup: Arc<Setup>, stopping: Arc<AtomicBool>) {
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

    let mut session = match ServerConnection::new(Arc::clone(&setup.tls)) {
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
    let mut talk = Conversation {
        peer,
        stage: Stage::SecretWord,
        username: String::new(),
        claim: None,
        asked_at: None,
    };
    converse(&mut stream, &mut talk, &setup, started, &stopping);
    goodbye(&mut stream);
    // `talk` goes away at the end of this function, and its claim with it,
    // which logs the account out.
}

/// Everything after TLS: the login, then character select, until one side
/// hangs up, the server stops, or the account is logged in from somewhere
/// else.
fn converse(stream: &mut TlsStream, talk: &mut Conversation, setup: &Setup, started: Instant,
            stopping: &AtomicBool) {
    let peer = talk.peer;
    if send(stream, &protocol::hello()).is_err() {
        return;
    }

    // Bytes that have arrived and aren't a whole packet yet.
    let mut incoming: Vec<u8> = Vec::new();
    let mut chunk = [0u8; READ_CHUNK];

    loop {
        if stopping.load(Ordering::SeqCst) {
            return;
        }
        if let Some(claim) = &talk.claim {
            if claim.kicked() {
                scribe::info(Channel::Security,
                             &format!("{} was logged out of {} from somewhere else.", peer, talk.username));
                let _ = send(stream, &protocol::logged_out_elsewhere());
                return;
            }
        }
        if talk.stage.logging_in() && started.elapsed() >= LOGIN_DEADLINE {
            scribe::info(Channel::NetTcp,
                         &format!("{} didn't log in within {} seconds.  Closing it.",
                                  peer, LOGIN_DEADLINE.as_secs()));
            refuse(stream, peer);
            return;
        }
        if let Some(asked_at) = talk.asked_at {
            if asked_at.elapsed() >= CHOICE_DEADLINE {
                scribe::info(Channel::NetTcp,
                             &format!("{} didn't say what to do about the other session within {} seconds.  \
                             Closing it.", peer, CHOICE_DEADLINE.as_secs()));
                return;
            }
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
                    if talk.stage.logging_in() {
                        refuse(stream, peer);
                    }
                    return;
                }
            };
            if !handle(stream, talk, setup, &packet) {
                return;
            }
        }
    }
}

/// Deals with one whole packet.  True to carry on, false when the
/// connection should close.
fn handle(stream: &mut TlsStream,
          talk: &mut Conversation,
          setup: &Setup,
          packet: &Packet) -> bool {
    let peer = talk.peer;
    let kind = PacketType::from_byte(packet.kind);

    match (talk.stage, kind) {
        (Stage::SecretWord, Some(PacketType::SecretWord)) => {
            match protocol::read_one_string(&packet.payload) {
                Ok(word) if word == protocol::SECRET_WORD => {
                    if send(stream, &protocol::awaiting_authentication()).is_err() {
                        return false;
                    }
                    talk.stage = Stage::Credentials;
                    true
                }
                // What they sent instead isn't logged.  It could be anything.
                _ => {
                    scribe::info(Channel::NetTcp,
                                 &format!("{} didn't know the secret word.",
                                          peer));
                    refuse(stream, peer);
                    false
                }
            }
        }

        (Stage::Credentials, Some(PacketType::AuthenticationRequest)) => credentials(stream,
                                                                                     talk,
                                                                                     setup,
                                                                                     packet),

        (Stage::Choosing, Some(PacketType::SessionChoice)) => {
            talk.asked_at = None;
            match protocol::read_session_choice(&packet.payload) {
                Ok(Choice::LogTheOtherOut) => {
                    let Some(claim) = sessions::take_over(&talk.username) else {
                        scribe::info(Channel::Security,
                                     &format!("{}'s other session left while {} was \
                                     choosing, and the server filled up.  Turned \
                                     them away.",
                                              talk.username,
                                              peer));
                        let _ = send(stream, &protocol::server_full());
                        return false;
                    };
                    talk.claim = Some(claim);
                    scribe::info(Channel::Security,
                                 &format!("{} logged the other session on {} out.",
                                          peer,
                                          talk.username));
                    welcome(stream, talk, setup)
                }
                Ok(Choice::Disconnect) => {
                    scribe::info(Channel::Security,
                                 &format!("{} left the other session on {} \
                                 alone and hung up.",
                                          peer,
                                          talk.username));
                    false
                }
                Err(problem) => {
                    scribe::info(Channel::NetTcp, &format!("{} sent {}.  \
                    Closing it.",
                                                           peer, problem));
                    false
                }
            }
        }

        (Stage::CharacterSelect, Some(PacketType::CreateCharacter)) => create(stream, talk, setup,
                                                                              packet),
        (Stage::CharacterSelect, Some(PacketType::DeleteCharacter)) => delete(stream, talk, setup,
                                                                              packet),
        (Stage::CharacterSelect, Some(PacketType::EnterWorld)) => enter_world(stream, talk, setup,
                                                                              packet),
        (Stage::CharacterSelect, Some(PacketType::RequestCharacterList)) => {
            // No payload.  Anything in it means the client doesn't agree with
            // us about the protocol, the same as leftover bytes anywhere else.
            if !packet.payload.is_empty() {
                scribe::info(Channel::NetTcp,
                             &format!("{} sent a RequestCharacterList with {} byte(s) in \
                             it.  Closing it.",
                                      peer, packet.payload.len()));
                return false;
            }
            send_list(stream, talk, setup)
        }

        (Stage::CharacterSelect | Stage::InWorld, Some(PacketType::SimpleTcpMesg)) => {
            match protocol::read_one_string(&packet.payload) {
                Ok(text) => {
                    // Rust note: `{:?}` prints the string in quotes, with any
                    // newline written as \n, so one message stays one log
                    // line whatever is in it.
                    scribe::info(Channel::NetTcp,
                                 &format!("{} sent {:?}.  Sending it back.", peer, text));
                    send(stream, &protocol::simple_message(&text)).is_ok()
                }
                Err(problem) => {
                    scribe::info(Channel::NetTcp,
                                 &format!("{} sent a SimpleTcpMesg with {}.  Closing it.",
                                          peer, problem));
                    false
                }
            }
        }

        // Somebody who is in can send a packet we don't take at this point,
        // or from a later group before we know what to do with it.  That is
        // our shortcoming, not theirs.
        (Stage::CharacterSelect | Stage::InWorld, _) => {
            scribe::info(Channel::NetTcp,
                         &format!("{} sent packet type 0x{:02X}, which we don't handle \
                         here.  Ignored it.",
                                  peer, packet.kind));
            true
        }

        // Anything else before the answer to "already logged in" is out of
        // turn.  The password was right, so it isn't a failed login, and
        // there is no hold.
        (Stage::Choosing, _) => {
            scribe::info(Channel::NetTcp,
                         &format!("{} sent packet type 0x{:02X} instead of a session \
                         choice.  Closing it.",
                                  peer, packet.kind));
            false
        }

        // Anything else during the login is out of turn.
        (_, _) => {
            scribe::info(Channel::NetTcp,
                         &format!("{} sent packet type 0x{:02X} out of turn.",
                                  peer,
                                  packet.kind));
            refuse(stream, peer);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// The login
// ---------------------------------------------------------------------------

/// An AuthenticationRequest.  Checks the username and password, and either
/// lets the player in, asks them about the other session, or refuses.
fn credentials(stream: &mut TlsStream,
               talk: &mut Conversation,
               setup: &Setup,
               packet: &Packet) -> bool {
    let peer = talk.peer;

    // The clock starts the moment the attempt is in, and every path below
    // runs out the same floor before anything goes back.
    let arrived = Instant::now();
    let request = protocol::read_authentication_request(&packet.payload);
    let logged_in = match &request {
        Ok((username, password)) => log_in(username, password),
        Err(_) => None,
    };
    security::pad_login_time(arrived);

    match (&request, &logged_in) {
        (Ok(_), Some(username)) => {
            scribe::info(Channel::Security, &format!("{} logged in as {}.",
                                                     peer, username));
        }
        (Ok((username, _)), None) => {
            scribe::info(Channel::Security,
                         &format!("Login from {} as {} failed.",
                                  peer,
                                  name_for_log(username)));
        }
        (Err(problem), _) => {
            scribe::info(Channel::Security,
                         &format!("{} sent a login we couldn't read: {}.",
                                  peer,
                                  problem));
        }
    }

    let Some(username) = logged_in else {
        refuse(stream, peer);
        return false;
    };
    talk.username = username;

    match sessions::claim(&talk.username) {
        Ok(claim) => {
            talk.claim = Some(claim);
            welcome(stream, talk, setup)
        }
        Err(Refused::AlreadyOn) => {
            scribe::info(Channel::Security,
                         &format!("{} is already on somewhere else.  \
                         Asking {} what to do.",
                                  talk.username, peer));
            talk.stage = Stage::Choosing;
            talk.asked_at = Some(Instant::now());
            send(stream, &protocol::already_logged_in()).is_ok()
        }
        Err(Refused::Full) => {
            scribe::info(Channel::Security,
                         &format!("The server is full, with {} on.  Turned {} away.",
                                  sessions::MAX_LOGGED_IN,
                                  talk.username));
            let _ = send(stream, &protocol::server_full());
            false
        }
    }
}

/// Tries a username and password against the account files.  The account's
/// username (lowercase) if they are right, `None` if not.  It doesn't pad
/// the time -- the caller does that, on every path, this one included.
fn log_in(username: &str, password: &str) -> Option<String> {
    let mut account = match account::load_account(username) {
        Ok(Some(account)) => account,
        // No such account, a name that isn't allowed, or a damaged file.
        // All three are a plain failure.  They still pay for a hash, in the
        // same line as everybody else, so how long the answer takes says
        // nothing about which it was.
        Ok(None) | Err(_) => {
            security::verify_no_account(password);
            return None;
        }
    };

    if !security::verify_password(password, &account.password_hash_string) {
        return None;
    }

    // The password was right.  A login time we couldn't save isn't worth
    // turning the player away over.
    if let Err(error) = account::record_login(&mut account) {
        scribe::warn(Channel::Security,
                     &format!("Couldn't save the login time for {}: {}.  Letting \
                     them in anyway.",
                              account.username, error));
    }
    Some(account.username)
}

/// The player is in: the welcome, then their character list.
fn welcome(stream: &mut TlsStream, talk: &mut Conversation, setup: &Setup) -> bool {
    if send(stream, &protocol::authentication_result(true, protocol::SUCCESS_MESSAGE))
        .is_err() {
        return false;
    }
    talk.stage = Stage::CharacterSelect;
    send_list(stream, talk, setup)
}

/// A username the way it goes in the log.  Only a name that follows the
/// username rules gets written down.  Anything else could be somebody's
/// password typed into the wrong box -- a password needs a symbol, and a
/// username can't have one -- or junk meant to mess up the log.
fn name_for_log(username: &str) -> String {
    account::check_username(username).
        unwrap_or_else(|_| "a name that isn't allowed".to_string())
}

/// Sends the one failure answer and puts the address on hold.  Every way
/// out of a failed login comes through here.
fn refuse(stream: &mut TlsStream, peer: SocketAddr) {
    record_failure(peer.ip());
    let _ = send(stream,
                 &protocol::authentication_result(false, protocol::FAILURE_MESSAGE));
}

// ---------------------------------------------------------------------------
// Character select
// ---------------------------------------------------------------------------

/// A CreateCharacter.  Makes the character and sends the new list.
fn create(stream: &mut TlsStream, talk: &Conversation, setup: &Setup, packet: &Packet) -> bool {
    let Some(name) = one_name(talk, packet) else {
        return false;
    };
    let mut account = match current_account(talk) {
        Ok(account) => account,
        Err(message) => return send(stream,
                                    &protocol::character_result(false, &message))
            .is_ok(),
    };

    match (setup.calls.create)(&mut account, &name) {
        // The game logs the new character itself.
        Ok(_) => {
            let message = format!("{} made.",
                                  account::display_name(&name.to_ascii_lowercase()));
            send(stream, &protocol::character_result(true, &message))
                .is_ok() && send_list(stream, talk, setup)
        }
        Err(message) => {
            // `{:?}`, because the message can quote what the player typed.
            scribe::info(Channel::NetTcp,
                         &format!("{} on {} couldn't make a character: {:?}",
                                  talk.peer,
                                  talk.username,
                                  message));
            send(stream, &protocol::character_result(false, &message)).is_ok()
        }
    }
}

/// A DeleteCharacter.  Deletes the character and sends the new list.  The
/// client makes the player type the name out first, so this only takes the
/// name.
fn delete(stream: &mut TlsStream, talk: &Conversation, setup: &Setup, packet: &Packet) -> bool {
    let Some(name) = one_name(talk, packet) else {
        return false;
    };
    let mut account = match current_account(talk) {
        Ok(account) => account,
        Err(message) => return send(stream,
                                    &protocol::character_result(false, &message))
            .is_ok(),
    };

    match (setup.calls.delete)(&mut account, &name) {
        // The game logs the deletion itself.
        Ok(()) => {
            let message = format!("{} deleted.",
                                  account::display_name(&name.to_ascii_lowercase()));
            send(stream, &protocol::character_result(true, &message))
                .is_ok() && send_list(stream, talk, setup)
        }
        Err(message) => {
            scribe::info(Channel::NetTcp,
                         &format!("{} on {} couldn't delete a character: {:?}",
                                  talk.peer,
                                  talk.username,
                                  message));
            send(stream, &protocol::character_result(false, &message)).is_ok()
        }
    }
}

/// An EnterWorld.  Checks the character can be played, makes the login
/// token, and sends it with the UDP port.  The token itself is never
/// logged.
fn enter_world(stream: &mut TlsStream, talk: &mut Conversation,
               setup: &Setup, packet: &Packet) -> bool {
    let Some(name) = one_name(talk, packet) else {
        return false;
    };
    let account = match current_account(talk) {
        Ok(account) => account,
        Err(message) => return send(stream,
                                    &protocol::character_result(false, &message))
            .is_ok(),
    };
    
    let character = name.to_ascii_lowercase();

    // A character whose file is missing or damaged gets a kick, not a
    // refusal: there's nothing the player can do about it but tell an
    // admin.  The check call only hands back a sentence, so the list is
    // how we tell "damaged" from "not on this account".
    let damaged = (setup.calls.list)(&account)
        .iter()
        .any(|summary| summary.shortname == character && !summary.playable);
    if damaged {
        scribe::info(Channel::Security,
                     &format!("{} on {} was kicked: {}'s player file is missing or damaged.",
                              talk.peer,
                              talk.username,
                              account::display_name(&character)));
        let _ = send(stream, &protocol::verbal_kick(KickReason::CorruptPlayerFile));
        return false;
    }

    if let Err(message) = (setup.calls.check)(&account, &name) {
        scribe::info(Channel::NetTcp,
                     &format!("{} on {} couldn't play a character: {:?}", talk.peer, talk.username, message));
        return send(stream, &protocol::character_result(false, &message)).is_ok();
    }

    let token = match &talk.claim {
        Some(claim) => sessions::issue_token(claim, &character),
        None => Err("the connection isn't logged in".to_string()),
    };
    match token {
        Ok(token) => {
            scribe::info(Channel::Security,
                         &format!("{} on {} picked {}, and has a login token.", talk.peer, talk.username,
                                  account::display_name(&character)));
            talk.stage = Stage::InWorld;
            send(stream, &protocol::world_ticket(&token, setup.udp_port)).is_ok()
        }
        Err(problem) => {
            scribe::error(Channel::Security,
                          &format!("No login token for {} on {}: {}.", talk.peer, talk.username, problem));
            send(stream, &protocol::character_result(false, OUR_FAULT)).is_ok()
        }
    }
}

/// The name in a CreateCharacter, DeleteCharacter or EnterWorld.  `None`
/// means the packet couldn't be read, it has been logged, and the
/// connection should close.
fn one_name(talk: &Conversation, packet: &Packet) -> Option<String> {
    match protocol::read_one_string(&packet.payload) {
        Ok(name) => Some(name),
        Err(problem) => {
            scribe::info(Channel::NetTcp,
                         &format!("{} sent packet type 0x{:02X} with {}.  Closing it.",
                                  talk.peer, packet.kind, problem));
            None
        }
    }
}

/// The player's account, read fresh from the file.  The file is the truth,
/// and the Launcher can change it while the player is on.  An `Err` is a
/// message for the player.
fn current_account(talk: &Conversation) -> Result<Account, String> {
    match account::load_account(&talk.username) {
        Ok(Some(account)) => Ok(account),
        // Deleted while they were on.
        // TODO(delete-account): the session should probably just end.
        Ok(None) => Err("Your account can't be found.".to_string()),
        // load_account() has logged what is wrong with it.
        Err(_) => Err(OUR_FAULT.to_string()),
    }
}

/// Sends the character list: how many slots, and everything the list
/// shows about each character in them.  The game reads the player files
/// (the `list` call), since this crate can't.
fn send_list(stream: &mut TlsStream, talk: &Conversation, setup: &Setup) -> bool {
    let account = match current_account(talk) {
        Ok(account) => account,
        Err(message) => return send(stream,
                                    &protocol::character_result(false, &message))
            .is_ok(),
    };
    let characters = (setup.calls.list)(&account);
    let slots = account::MAX_CHARACTERS.min(u8::MAX as usize) as u8;
    send(stream, &protocol::character_list(slots, &characters)).is_ok()
}

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

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