//! File:     stratum-networking/src/sessions.rs
//! Project:  Stratum Networking
//! Author:   Jacob Chacko
//!
//! Who is logged in.  One entry per account, held by the connection that
//! logged it in, so an account is never on twice at once.  A second login
//! gets asked whether to log the first one out, or to hang up and leave it
//! alone (so a shared account doesn't kick your brother off because you
//! wanted to play).
//!
//! It is also where the server's player limit lives: MAX_LOGGED_IN
//! accounts on at once, counting everybody past the password, in character
//! select or in the world.  The next one gets told the server is full.
//! Taking over your own session doesn't count, because it doesn't add
//! anybody.
//!
//! The entry is also where an account's login token lives, once the player
//! has picked a character, and where its UDP packets come from once the
//! token has been used.  udp.rs looks tokens up here.  In memory only: a
//! restart forgets every one of them, on purpose.
//!
//! A token connects once.  After that the UDP side knows the player by
//! their address, and when it lets them go (they went quiet, or logged
//! out) the token goes with it.
//!
//! A connection holds its entry as a `Claim`.  When the connection ends,
//! the claim goes away and takes the entry with it, whichever way the
//! connection ended.  Logging a session out from somewhere else doesn't
//! wait for the old connection to notice.  The new one takes the entry
//! over on the spot and raises the old one's `kicked` flag, and the old
//! one sees it within one read wait and closes.  Each entry has a number,
//! so the old claim going away later can't take the new entry with it.
//!
//! The work is done by the `_in` functions, on a map and a number handed
//! to them, so the tests can run them without touching the real list.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use stratum_tools::fingerprinter;

/// One logged-in account.
struct Session {
    /// Which claim holds it.  A new one gets a new number.
    id: u64,
    /// Raised when somebody logs this session out from somewhere else.
    kicked: Arc<AtomicBool>,
    /// The login token and the character it is for, once one is picked.
    /// A new one replaces the old.
    ticket: Option<(String, String)>,
    /// Where the player's UDP packets come from, once the token has
    /// connected.  `None` until then.
    udp: Option<SocketAddr>,
}

/// The most accounts logged in at once.  Jacob's number, for now.  The TCP
/// side's MAX_CONNECTIONS is a little higher, so a full server still has
/// room to tell the next player so.
pub const MAX_LOGGED_IN: usize = 1;

/// Why claim() said no.
#[derive(Debug, PartialEq)]
pub enum Refused {
    /// The account is logged in somewhere else.  The player gets asked
    /// what to do.
    AlreadyOn,
    /// MAX_LOGGED_IN accounts are on already.
    Full,
}

/// Every logged-in account, by username.
// Rust note: the same LazyLock shape as the failure list in tcp.rs.  A
// HashMap can't be built before the program starts, so it gets built the
// first time anybody touches it.
static SESSIONS: LazyLock<Mutex<HashMap<String, Session>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The number the next claim gets.  Starts at 1 and only goes up.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A connection's hold on its account.  Letting go of it (the connection
/// ending, for any reason) takes the account off the list, unless somebody
/// else has taken it over since.
pub struct Claim {
    username: String,
    id: u64,
    kicked: Arc<AtomicBool>,
}

impl Claim {
    /// True once somebody has logged this session out from somewhere else.
    pub fn kicked(&self) -> bool {
        self.kicked.load(Ordering::SeqCst)
    }
}

// Rust note: the same trick as OpenConnection in tcp.rs.  `drop` runs when
// the claim goes away, so there is no way out of a connection that forgets
// to log the account out.
impl Drop for Claim {
    fn drop(&mut self) {
        let mut sessions = SESSIONS.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        release_in(&mut sessions, &self.username, self.id);
    }
}

/// Logs an account in, if it isn't on already and there is room.  An
/// account that is already on gets `AlreadyOn` even on a full server, so
/// its player can still take their own session over.
pub fn claim(username: &str) -> Result<Claim, Refused> {
    let mut sessions = SESSIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let kicked = claim_in(&mut sessions, username, id)?;
    Ok(Claim { username: username.to_string(), id, kicked })
}

/// Logs an account in whether it is on or not.  If it is, the other
/// session is told to go, and its token goes with it.  `None` only if the
/// other session left on its own while the player was choosing, and the
/// server filled up in the meantime.
pub fn take_over(username: &str) -> Option<Claim> {
    let mut sessions = SESSIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let kicked = take_over_in(&mut sessions, username, id)?;
    Some(Claim { username: username.to_string(), id, kicked })
}

/// Makes a login token for a claim's account and the character the player
/// picked, and keeps it.  Hands back the token.  An `Err` says why not.
pub fn issue_token(claim: &Claim, character: &str) -> Result<String, String> {
    let token = fingerprinter::new_token()
        .map_err(|error| format!("couldn't make a token: {}", error))?;

    let mut sessions = SESSIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match sessions.get_mut(&claim.username) {
        Some(session) if session.id == claim.id => {
            session.ticket = Some((token.clone(), character.to_string()));
            session.udp = None;
            Ok(token)
        }
        // Somebody logged this session out between the pick and now.
        _ => Err("the account was taken over from somewhere else".to_string()),
    }
}

/// Connects a player over UDP with the token from their WorldTicket.  Hands
/// back the account and the character, or `None` if nobody was handed that
/// token, or it has already connected from a different address.  A repeat
/// from the same address is fine: that is a client whose answer got lost,
/// asking again.
pub fn connect_udp(token: &str, from: SocketAddr) -> Option<(String, String)> {
    let mut sessions = SESSIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    connect_udp_in(&mut sessions, token, from)
}

/// Where an account's UDP packets come from.  `None` if the account isn't
/// on, or hasn't connected over UDP.  udp.rs asks this to find out whether
/// the TCP side still has the player.
pub fn udp_address(username: &str) -> Option<SocketAddr> {
    let sessions = SESSIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    sessions.get(username).and_then(|session| session.udp)
}

/// Ends an account's UDP session and spends its token, so the token can't
/// connect again.  Only if it is still connected from this address.
pub fn end_udp(username: &str, from: SocketAddr) {
    let mut sessions = SESSIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    end_udp_in(&mut sessions, username, from);
}

// ---------------------------------------------------------------------------
// The work, on whatever map is handed in
// ---------------------------------------------------------------------------

/// Each of these hands back the new entry's `kicked` flag.  Already on is
/// checked before full, on purpose (see claim()).
fn claim_in(sessions: &mut HashMap<String, Session>,
            username: &str,
            id: u64) -> Result<Arc<AtomicBool>, Refused> {
    if sessions.contains_key(username) {
        return Err(Refused::AlreadyOn);
    }
    if sessions.len() >= MAX_LOGGED_IN {
        return Err(Refused::Full);
    }
    Ok(insert_in(sessions, username, id))
}

fn take_over_in(sessions: &mut HashMap<String, Session>,
                username: &str,
                id: u64) -> Option<Arc<AtomicBool>> {
    match sessions.get(username) {
        Some(old) => old.kicked.store(true, Ordering::SeqCst),
        // Nobody to take over, so this adds somebody, and needs room.
        None if sessions.len() >= MAX_LOGGED_IN => return None,
        None => {}
    }
    Some(insert_in(sessions, username, id))
}

/// Puts a fresh entry in, replacing any old one, token and all.
fn insert_in(sessions: &mut HashMap<String, Session>, username: &str, id: u64) -> Arc<AtomicBool> {
    let kicked = Arc::new(AtomicBool::new(false));
    sessions.insert(username.to_string(), Session { id, kicked: Arc::clone(&kicked), ticket: None, udp: None });
    kicked
}

/// Finds the session holding this token.  MAX_LOGGED_IN players at most,
/// so looking at each one is fine.
fn connect_udp_in(sessions: &mut HashMap<String, Session>,
                  token: &str,
                  from: SocketAddr) -> Option<(String, String)> {
    for (username, session) in sessions.iter_mut() {
        let Some((ticket_token, character)) = &session.ticket else {
            continue;
        };
        if ticket_token != token {
            continue;
        }
        match session.udp {
            None => session.udp = Some(from),
            Some(address) if address == from => {}
            Some(_) => return None,
        }
        return Some((username.clone(), character.clone()));
    }
    None
}

fn end_udp_in(sessions: &mut HashMap<String, Session>, username: &str, from: SocketAddr) {
    if let Some(session) = sessions.get_mut(username) {
        if session.udp == Some(from) {
            session.udp = None;
            session.ticket = None;
        }
    }
}

/// Takes an account off the list, but only if the entry is still the one
/// with this number.
fn release_in(sessions: &mut HashMap<String, Session>, username: &str, id: u64) {
    let ours = match sessions.get(username) {
        Some(session) => session.id == id,
        None => false,
    };
    if ours {
        sessions.remove(username);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_account_is_only_on_once() {
        let mut sessions = HashMap::new();

        assert!(claim_in(&mut sessions, "jacob", 1).is_ok());
        assert_eq!(claim_in(&mut sessions, "jacob", 2).err(), Some(Refused::AlreadyOn));
        assert!(claim_in(&mut sessions, "brother", 3).is_ok());
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn taking_over_kicks_the_old_session() {
        let mut sessions = HashMap::new();

        let old_kicked = claim_in(&mut sessions, "jacob", 1).unwrap();
        let new_kicked = take_over_in(&mut sessions, "jacob", 2).unwrap();

        assert!(old_kicked.load(Ordering::SeqCst));
        assert!(!new_kicked.load(Ordering::SeqCst));
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn the_old_session_leaving_late_leaves_the_new_one_alone() {
        let mut sessions = HashMap::new();

        claim_in(&mut sessions, "jacob", 1).unwrap();
        take_over_in(&mut sessions, "jacob", 2).unwrap();

        // The kicked connection notices and goes, after the new one is in.
        release_in(&mut sessions, "jacob", 1);
        assert_eq!(sessions.len(), 1);

        release_in(&mut sessions, "jacob", 2);
        assert!(sessions.is_empty());
        assert!(claim_in(&mut sessions, "jacob", 3).is_ok());
    }

    /// A server with MAX_LOGGED_IN players on, called player0, player1 and
    /// so on.
    fn full() -> HashMap<String, Session> {
        let mut sessions = HashMap::new();
        for number in 0..MAX_LOGGED_IN {
            claim_in(&mut sessions, &format!("player{}", number), number as u64).unwrap();
        }
        sessions
    }

    #[test]
    fn a_full_server_turns_the_next_one_away() {
        let mut sessions = full();

        assert_eq!(claim_in(&mut sessions, "jacob", 100).err(), Some(Refused::Full));
        assert_eq!(sessions.len(), MAX_LOGGED_IN);

        // Somebody leaves, and there is room again.
        release_in(&mut sessions, "player0", 0);
        assert!(claim_in(&mut sessions, "jacob", 101).is_ok());
    }

    #[test]
    fn on_a_full_server_you_can_still_take_over_your_own_session() {
        let mut sessions = full();

        // Already on comes first, so the player gets asked, not turned away.
        assert_eq!(claim_in(&mut sessions, "player7", 100).err(), Some(Refused::AlreadyOn));
        assert!(take_over_in(&mut sessions, "player7", 101).is_some());
        assert_eq!(sessions.len(), MAX_LOGGED_IN);

        // But if their old session left while they were choosing, and
        // somebody else filled its place, there is nothing to take over.
        release_in(&mut sessions, "player8", 8);
        claim_in(&mut sessions, "jacob", 102).unwrap();
        assert!(take_over_in(&mut sessions, "player8", 103).is_none());
    }

    /// An account with a token for Aldric, the way issue_token() leaves it.
    fn with_ticket(token: &str) -> HashMap<String, Session> {
        let mut sessions = HashMap::new();
        claim_in(&mut sessions, "jacob", 1).unwrap();
        sessions.get_mut("jacob").unwrap().ticket = Some((token.to_string(), "aldric".to_string()));
        sessions
    }

    #[test]
    fn a_token_connects_from_one_address() {
        let mut sessions = with_ticket("abc");
        let home: SocketAddr = "10.0.0.5:50000".parse().unwrap();
        let elsewhere: SocketAddr = "10.0.0.6:50000".parse().unwrap();
        let expected = Some(("jacob".to_string(), "aldric".to_string()));

        assert_eq!(connect_udp_in(&mut sessions, "abc", home), expected);
        // The answer got lost and the client asked again.
        assert_eq!(connect_udp_in(&mut sessions, "abc", home), expected);
        // Somebody else with a copy of the token.
        assert_eq!(connect_udp_in(&mut sessions, "abc", elsewhere), None);
    }

    #[test]
    fn a_token_nobody_was_handed_is_refused() {
        let mut sessions = with_ticket("abc");
        let home: SocketAddr = "10.0.0.5:50000".parse().unwrap();

        assert_eq!(connect_udp_in(&mut sessions, "abd", home), None);
        assert_eq!(connect_udp_in(&mut sessions, "", home), None);
    }

    #[test]
    fn a_token_is_spent_when_its_udp_session_ends() {
        let mut sessions = with_ticket("abc");
        let home: SocketAddr = "10.0.0.5:50000".parse().unwrap();
        let elsewhere: SocketAddr = "10.0.0.6:50000".parse().unwrap();
        connect_udp_in(&mut sessions, "abc", home).unwrap();

        // Ending it from the wrong address does nothing.
        end_udp_in(&mut sessions, "jacob", elsewhere);
        assert_eq!(sessions["jacob"].udp, Some(home));

        end_udp_in(&mut sessions, "jacob", home);
        assert_eq!(connect_udp_in(&mut sessions, "abc", home), None);
        // The account itself is still on, back in character select.
        assert_eq!(sessions.len(), 1);
    }
}