//! File:     stratum-networking/src/udp.rs
//! Project:  Stratum Networking
//! Author:   Jacob Chacko
//!
//! The UDP side, where the game goes.  A player gets here with the token
//! from their WorldTicket, and from then on the game talks to them over UDP
//! while chat stays on their TCP connection.  The two are kept apart on
//! purpose: if one falls over, the other keeps going.
//!
//! UDP has no connections, so there are no connection threads.  One thread
//! reads every packet from every player, off one socket.  It knows who a
//! packet is from by the address and port it came from, which it learns
//! from the player's first packet, the Connect.  The token only goes over
//! once.
//!
//! Anything it can't use gets no answer at all: junk, a packet too big, and
//! anything but a Connect from an address it doesn't know.  A server that
//! answers strangers can be used to flood somebody else.
//!
//! A player who goes SILENCE_LIMIT without sending anything is let go, and
//! their token is spent.  So is a player whose TCP side has gone (they
//! logged out, or got logged out from somewhere else).  The thread checks
//! for both every SWEEP_EVERY.
//!
//! This is the first step.  What a player sends once they're in has
//! nowhere to go until there is a game loop, and the delivery modes come
//! after that.

use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use stratum_tools::{account, constellations};
use stratum_tools::scribe::{self, Channel};

use crate::client_version;
use crate::protocol::{self, ConnectAnswer, PacketType};
use crate::sessions;

/// How long the thread waits on a read before it looks around.  The same as
/// the TCP side's, so stop() is just as quick.
const READ_WAIT: Duration = Duration::from_millis(50);

/// How long a player can go without sending anything before we let them go.
/// A guess.  Clients send something every second, so this is ten of those
/// lost in a row.
const SILENCE_LIMIT: Duration = Duration::from_secs(10);

/// How often we look for players who went quiet or whose TCP side is gone.
const SWEEP_EVERY: Duration = Duration::from_secs(1);

/// How much we read in one go.  Bigger than MAX_UDP_BYTES on purpose: a
/// packet that doesn't fit gets cut to fit without a word, and then it
/// would look like one we take.  This way a too-big one comes in too big,
/// and gets refused.
const READ_BUFFER: usize = 2048;

/// The running UDP side.  Held in UDP below while the server is started.
struct UdpSide {
    /// Set by stop().  The thread checks it every READ_WAIT.
    stopping: Arc<AtomicBool>,
    listener: JoinHandle<()>,
}

// Rust note: the same shape as TCP in tcp.rs.  `None` means the UDP side
// isn't running.
static UDP: Mutex<Option<UdpSide>> = Mutex::new(None);

/// A player who has connected over UDP.  Only the UDP thread ever sees
/// these, so they need no lock.
struct Player {
    /// The account, lowercase.
    username: String,
    /// The character they're playing, for the log.
    character: String,
    last_heard: Instant,
}

/// Binds the address and port from the config file and starts the thread.
/// An `Err` says what went wrong, in words, and nothing is running.
pub fn start() -> Result<(), String> {
    let mut guard = UDP.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_some() {
        return Err("The UDP side is already running.".to_string());
    }

    let settings = constellations::get();
    let address = SocketAddr::new(settings.udp_host_address, settings.udp_port);
    let socket = UdpSocket::bind(address)
        .map_err(|error| format!("Couldn't listen on {} for UDP: {}", address, error))?;
    socket.set_read_timeout(Some(READ_WAIT))
        .map_err(|error| format!("Couldn't set up the UDP socket on {}: {}", address, error))?;

    let stopping = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stopping);
    let handle = thread::Builder::new()
        .name("udp-listener".to_string())
        .spawn(move || listen(socket, flag))
        .map_err(|error| format!("Couldn't start the UDP thread: {}", error))?;

    scribe::info(Channel::NetUdp, &format!("Listening on {}.", address));
    *guard = Some(UdpSide { stopping, listener: handle });
    Ok(())
}

/// Stops the thread and waits for it.  Does nothing if the UDP side isn't
/// running.
pub fn stop() {
    // Take the side out of the global first, so the lock isn't held while
    // we wait on the thread.
    let side = {
        let mut guard = UDP.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.take()
    };
    let Some(side) = side else {
        return;
    };

    // No knocking needed, unlike TCP.  The thread wakes every READ_WAIT
    // whether anything arrived or not.
    side.stopping.store(true, Ordering::SeqCst);
    let _ = side.listener.join();
    scribe::info(Channel::NetUdp, "Stopped listening.");
}

/// The thread.  Reads a packet, deals with it, and every SWEEP_EVERY lets go
/// of anybody who should go.
fn listen(socket: UdpSocket, stopping: Arc<AtomicBool>) {
    let mut players: HashMap<SocketAddr, Player> = HashMap::new();
    let mut buffer = [0u8; READ_BUFFER];
    let mut last_sweep = Instant::now();

    while !stopping.load(Ordering::SeqCst) {
        match socket.recv_from(&mut buffer) {
            Ok((size, from)) => heard(&socket, &mut players, &buffer[..size], from),
            // Rust note: on Linux a read that runs out of time comes back as
            // WouldBlock, and elsewhere as TimedOut.  Either one only means
            // nothing arrived.
            Err(error) if error.kind() == io::ErrorKind::WouldBlock
                || error.kind() == io::ErrorKind::TimedOut => {}
            Err(error) => {
                scribe::warn(Channel::NetUdp, &format!("Read failed: {}.", error));
                // A failure that keeps happening would otherwise fill the
                // log as fast as the disk allows.
                thread::sleep(Duration::from_millis(100));
            }
        }

        if last_sweep.elapsed() >= SWEEP_EVERY {
            sweep(&mut players);
            last_sweep = Instant::now();
        }
    }
}

/// One packet, from one address.
fn heard(socket: &UdpSocket,
         players: &mut HashMap<SocketAddr, Player>,
         bytes: &[u8],
         from: SocketAddr) {
    let Ok(packet) = protocol::take_datagram(bytes) else {
        return;
    };

    if let Some(player) = players.get_mut(&from) {
        player.last_heard = Instant::now();
        if packet.kind == PacketType::Connect as u8 {
            // Our answer got lost, and the client is asking again.
            send(socket, from, &protocol::connect_result(ConnectAnswer::Accepted));
        }
        // TODO(game-loop): anything else a connected player sends goes up
        // the game loop's queue.  Until there is one, it only keeps the
        // player from going quiet.
        return;
    }

    // A stranger only gets anywhere with a Connect.
    if packet.kind != PacketType::Connect as u8 {
        return;
    }
    let Ok((version, token)) = protocol::read_connect(&packet.payload) else {
        return;
    };

    // The version first.  An old client gets told so, whatever its token.
    if !client_version::accepted(&version) {
        scribe::info(Channel::NetUdp,
                     &format!("Turned away client version {:?} from {}.",
                              version,
                              from));
        send(socket, from, &protocol::connect_result(ConnectAnswer::Outdated));
        return;
    }

    match sessions::connect_udp(&token, from) {
        Some((username, character)) => {
            scribe::info(Channel::Security,
                         &format!("{} is in the world as {}, over UDP from {}.",
                                  username,
                                  account::display_name(&character),
                                  from));
            players.insert(from, Player { username, character, last_heard: Instant::now() });
            send(socket, from, &protocol::connect_result(ConnectAnswer::Accepted));
        }
        None => {
            // Never the token.  Whether it was a stranger's guess or a
            // copy, the log doesn't need it.
            scribe::info(Channel::Security, &format!("Refused a UDP connect from {}.", from));
            send(socket, from, &protocol::connect_result(ConnectAnswer::Refused));
        }
    }
}

fn send(socket: &UdpSocket, to: SocketAddr, bytes: &[u8]) {
    if let Err(error) = socket.send_to(bytes, to) {
        scribe::warn(Channel::NetUdp, &format!("Couldn't send to {}: {}.", to, error));
    }
}

/// Lets go of every player whose TCP side is gone, and every one who has
/// been quiet for SILENCE_LIMIT.
fn sweep(players: &mut HashMap<SocketAddr, Player>) {
    // Rust note: retain() keeps the entries the closure says true for and
    // drops the rest.
    players.retain(|address, player| {
        // Logged out, or logged out from somewhere else.  Either way the
        // session that owned this address is gone, and its token with it.
        if sessions::udp_address(&player.username) != Some(*address) {
            scribe::info(Channel::NetUdp,
                         &format!("{} ({}) left the world.  Their session ended.",
                                  player.username,
                                  account::display_name(&player.character)));
            return false;
        }

        if player.last_heard.elapsed() >= SILENCE_LIMIT {
            sessions::end_udp(&player.username, *address);
            scribe::info(Channel::NetUdp,
                         &format!("{} ({}) went quiet for {} seconds.  Their token is spent.",
                                  player.username,
                                  account::display_name(&player.character),
                                  SILENCE_LIMIT.as_secs()));
            // TODO(udp-timeout): tell the TCP side, so the player gets sent
            // back to character select.  That packet doesn't exist yet.
            return false;
        }

        true
    });
}