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
//! The entry is also where an account's login token lives, once the player
//! has picked a character.  The UDP side will look tokens up here.  In
//! memory only: a restart forgets every one of them, on purpose.
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
    // TODO(udp): udp.rs looks a token up here when a client first knocks.
    // Until then nothing reads it.
    ticket: Option<(String, String)>,
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

/// Logs an account in, if it isn't on already.  `None` means it is, and
/// the player gets asked what to do.
pub fn claim(username: &str) -> Option<Claim> {
    let mut sessions = SESSIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let kicked = claim_in(&mut sessions, username, id)?;
    Some(Claim { username: username.to_string(), id, kicked })
}

/// Logs an account in whether it is on or not.  If it is, the other
/// session is told to go, and its token goes with it.
pub fn take_over(username: &str) -> Claim {
    let mut sessions = SESSIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let kicked = take_over_in(&mut sessions, username, id);
    Claim { username: username.to_string(), id, kicked }
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
            Ok(token)
        }
        // Somebody logged this session out between the pick and now.
        _ => Err("the account was taken over from somewhere else".to_string()),
    }
}

// ---------------------------------------------------------------------------
// The work, on whatever map is handed in
// ---------------------------------------------------------------------------

/// Each of these hands back the new entry's `kicked` flag.
fn claim_in(sessions: &mut HashMap<String, Session>, username: &str, id: u64) -> Option<Arc<AtomicBool>> {
    if sessions.contains_key(username) {
        return None;
    }
    Some(insert_in(sessions, username, id))
}

fn take_over_in(sessions: &mut HashMap<String, Session>, username: &str, id: u64) -> Arc<AtomicBool> {
    if let Some(old) = sessions.get(username) {
        old.kicked.store(true, Ordering::SeqCst);
    }
    insert_in(sessions, username, id)
}

/// Puts a fresh entry in, replacing any old one, token and all.
fn insert_in(sessions: &mut HashMap<String, Session>, username: &str, id: u64) -> Arc<AtomicBool> {
    let kicked = Arc::new(AtomicBool::new(false));
    sessions.insert(username.to_string(), Session { id, kicked: Arc::clone(&kicked), ticket: None });
    kicked
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

        assert!(claim_in(&mut sessions, "jacob", 1).is_some());
        assert!(claim_in(&mut sessions, "jacob", 2).is_none());
        assert!(claim_in(&mut sessions, "brother", 3).is_some());
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn taking_over_kicks_the_old_session() {
        let mut sessions = HashMap::new();

        let old_kicked = claim_in(&mut sessions, "jacob", 1).unwrap();
        let new_kicked = take_over_in(&mut sessions, "jacob", 2);

        assert!(old_kicked.load(Ordering::SeqCst));
        assert!(!new_kicked.load(Ordering::SeqCst));
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn the_old_session_leaving_late_leaves_the_new_one_alone() {
        let mut sessions = HashMap::new();

        claim_in(&mut sessions, "jacob", 1).unwrap();
        take_over_in(&mut sessions, "jacob", 2);

        // The kicked connection notices and goes, after the new one is in.
        release_in(&mut sessions, "jacob", 1);
        assert_eq!(sessions.len(), 1);

        release_in(&mut sessions, "jacob", 2);
        assert!(sessions.is_empty());
        assert!(claim_in(&mut sessions, "jacob", 3).is_some());
    }
}