//! File:     stratum-game/src/names.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! Character names.  The rules a name has to follow, and the list of every
//! character name on the server, so no two characters are ever called the
//! same thing.  There is one Aldric, whichever account he is on.
//!
//! A character name is 4 to 12 letters, ASCII only.  It is stored lowercase
//! everywhere, file names included, and the game shows it with the first
//! letter capitalized (`account::display_name()`).  Usernames are a
//! separate thing with separate rules, and they live in `account.rs`.
//!
//! The list lives in memory and is empty every time the server starts, so
//! `start()` reads every account file at launch to learn which names are
//! taken.  main() calls it right after `account::start()`, which is what
//! reads the account folder.  From then on `reserve_name()` and
//! `release_name()` keep the list right as characters come and go.
//!
//! Nothing in here touches the disk except `start()`.  The list is the
//! only thing that has to be right for a new name to be safe.

use std::collections::HashSet;
use std::sync::Mutex;

use stratum_tools::account;
use stratum_tools::scribe::{self, Channel};

const MIN_CHARACTER_NAME_CHARS: usize = 4;
const MAX_CHARACTER_NAME_CHARS: usize = 12;

/// Every character name on the server, lowercase.
struct Names {
    taken: HashSet<String>,
}

/// `None` until `start()` has read the account files.  While it is `None`
/// no new name can be taken, because we don't know which ones are free.
static NAMES: Mutex<Option<Names>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// The rules
// ---------------------------------------------------------------------------

/// Says whether a character name is allowed, and hands it back lowercase if
/// it is.  4 to 12 letters, nothing else.  An `Err` is a message in words
/// for whoever typed it.  This says nothing about whether the name is
/// free; `reserve_name()` does that.
pub fn check_character_name(name: &str) -> Result<String, String> {
    if !name.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err("A character name can only have letters in it.".to_string());
    }

    let length = name.chars().count();
    if length < MIN_CHARACTER_NAME_CHARS || length > MAX_CHARACTER_NAME_CHARS {
        return Err(format!("A character name needs {} to {} letters.",
                           MIN_CHARACTER_NAME_CHARS, MAX_CHARACTER_NAME_CHARS));
    }

    Ok(name.to_ascii_lowercase())
}

// ---------------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------------

/// Reads every account and notes which character names are taken.  main()
/// calls this once, after `account::start()`, which is where the list of
/// accounts comes from.
///
/// A damaged account file has already been complained about by
/// `load_account()`.  Its character names can't be known, and that gets an
/// Error of its own, because one of them could be taken again.  The server
/// carries on either way.
pub fn start() {
    let mut names = Names { taken: HashSet::new() };

    for username in account::list_usernames() {
        match account::load_account(&username) {
            Ok(Some(found)) => {
                for character in &found.characters {
                    if !reserve_in(&mut names, &character.name) {
                        scribe::error(Channel::World,
                                      &format!("There is more than one character called {}.  \
                                                The second one is on account {}.",
                                               account::display_name(&character.name), username));
                    }
                }
            }
            // The file went away between account::start() and now.
            Ok(None) => {}
            Err(_) => {
                scribe::error(Channel::World,
                              &format!("Account {} can't be read, so its character names \
                                        aren't known.  One of them could be taken again.",
                                       username));
            }
        }
    }

    scribe::info(Channel::World, &format!("Found {} character name(s) in use.", names.taken.len()));

    let mut guard = NAMES.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(names);
}

/// Takes a name, so nobody else can.  The name has to have been through
/// `check_character_name()` already, so it is lowercase.  An `Err` says why
/// not, in words for whoever typed it.
pub fn reserve_name(name: &str) -> Result<(), String> {
    let mut guard = NAMES.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match guard.as_mut() {
        Some(names) => {
            if reserve_in(names, name) {
                Ok(())
            } else {
                Err(format!("There is already a character called {}.", account::display_name(name)))
            }
        }
        None => Err("The list of character names couldn't be loaded, so no new names can be \
                     taken.".to_string()),
    }
}

/// Hands a name back: the character is gone, or never made it to the disk.
pub fn release_name(name: &str) {
    let mut guard = NAMES.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(names) = guard.as_mut() {
        release_in(names, name);
    }
}

/// The part of `reserve_name()` that doesn't need the lock, so the tests
/// can run it on a `Names` of their own.  True if the name was free.
fn reserve_in(names: &mut Names, name: &str) -> bool {
    // `insert` says false when the name was already in there.
    names.taken.insert(name.to_string())
}

fn release_in(names: &mut Names, name: &str) {
    names.taken.remove(name);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_names() {
        assert_eq!(check_character_name("Aldric"), Ok("aldric".to_string()));
        assert_eq!(check_character_name("ALDRIC"), Ok("aldric".to_string()));
        assert_eq!(check_character_name("abcd"), Ok("abcd".to_string()));
        assert_eq!(check_character_name("abcdefghijkl"), Ok("abcdefghijkl".to_string()));

        assert!(check_character_name("abc").is_err());
        assert!(check_character_name("abcdefghijklm").is_err());
        assert!(check_character_name("").is_err());
        assert!(check_character_name("aldric2").is_err());
        assert!(check_character_name("al_dric").is_err());
        assert!(check_character_name("al dric").is_err());
        assert!(check_character_name("aldr\u{e9}c").is_err());
    }

    #[test]
    fn a_name_is_taken_once() {
        let mut names = Names { taken: HashSet::new() };

        assert!(reserve_in(&mut names, "aldric"));
        assert!(!reserve_in(&mut names, "aldric"));
        assert!(reserve_in(&mut names, "bertram"));
        assert_eq!(names.taken.len(), 2);
    }

    #[test]
    fn a_released_name_is_free_again() {
        let mut names = Names { taken: HashSet::new() };

        assert!(reserve_in(&mut names, "aldric"));
        release_in(&mut names, "aldric");
        assert!(reserve_in(&mut names, "aldric"));

        // Releasing a name that was never taken is nothing.
        release_in(&mut names, "nobody");
        assert_eq!(names.taken.len(), 1);
    }
}