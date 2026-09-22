//! File:     stratum-tools/src/account.rs
//! Project:  Stratum Core
//! Author:   Jacob Chacko
//!
//! Accounts.  One JSON file per account, named after the username, holding
//! the password hash and a reference to each character on the account: its
//! UUID and its name, and nothing more.  The characters themselves are the
//! game's, in files of their own.  The server admin makes and deletes the
//! accounts, from the Launcher's account menu.  This is a game between
//! friends, not a sign-up page.
//!
//! An account file is always written with `diskman::write_file()`, never
//! `write_later()`.  We want to know the account landed before we say it
//! exists, and the character files will be the `write_later()` kind.
//! DiskMan says never both on one path.
//!
//! Every account also gets its own UUID (`account_uid`), so anything that
//! needs to point at an account can do it without leaning on the username.
//!
//! Usernames are stored lowercase everywhere, file names included.  A
//! username is 4 to 16 letters, numbers and underscores, and there is one
//! account called "jacob" on the whole server.  At launch `start()` reads
//! the account folder to learn which usernames are taken.  Character names
//! have rules and a list of their own, in the game (stratum-game's
//! names.rs), and this file doesn't check them.
//!
//! Standard library plus serde and serde_json, which do the JSON.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::diskman::{self, StratumFile};
use crate::fingerprinter::new_uuid;
use crate::scribe::{self, Channel};
use crate::security;
use crate::constellations;

// ---------------------------------------------------------------------------
// The numbers
// ---------------------------------------------------------------------------
/// The account file's extension. The file for "jacob" is 'jacob.act' in the folder
/// Constellations says.
const ACCOUNT_EXTENSION: &str = "act";

const MIN_USERNAME_CHARS: usize = 4;
const MAX_USERNAME_CHARS: usize = 16;

/// The longest a real name is allowed to be.  Plenty for a real one, and
/// short enough that nobody pastes a novel into it.
const MAX_REAL_NAME_CHARS: usize = 64;

/// The longest an email address is allowed to be.  254 is the limit the
/// email standards themselves set.
const MAX_EMAIL_CHARS: usize = 254;

// ---------------------------------------------------------------------------
// The account
// ---------------------------------------------------------------------------

/// One account, the way it sits in the account file.
// Rust note: `Serialize` and `Deserialize` are serde's.  Asking for them
// here has serde write the code that turns this struct into JSON and back,
// so we never write a JSON parser.  The field names below are the names in
// the file.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Account {
    /// Always lowercase.  Also the file name.
    pub username: String,
    /// The line `security::hash_password()` handed back.  Never the password.
    pub password_hash_string: String,
    /// Empty if the player didn't give one.  It is there so the admin can
    /// reach the player, MUD style.  Nothing sends mail to it.  Stored as
    /// typed, and never logged.
    pub email: String,
    /// Empty if the player didn't give one.  Never logged.
    pub real_name: String,
    /// Month and day only, as "MM-DD", or empty.  No year, on purpose: a
    /// birthday note needs the day, and a full date of birth next to a real
    /// name and an email is more than a game between friends should keep.
    pub birthday: String,
    /// A UUID made when the account is.  It never changes, even if the
    /// username someday can.
    pub account_uid: String,
    /// In the order they were made.
    pub characters: Vec<CharacterRef>,
    /// Seconds since 1970, UTC.
    pub created_at: u64,
    /// Seconds since 1970, UTC.  0 until the first login.
    pub last_login: u64,
}

/// A reference to one character on an account: enough to list them at
/// login and find the character's file, and no more.  The character itself
/// is in the game's player file, under this UUID.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CharacterRef {
    pub uuid: String,
    /// Always lowercase.  `display_name()` is how it gets shown.
    pub name: String,
}

// ---------------------------------------------------------------------------
// Names in use
// ---------------------------------------------------------------------------

/// Every username on the server.  Built at launch by listing the account
/// folder, and kept up to date as accounts are made and deleted.  It is how
/// a new name gets checked against the whole server without opening every
/// file every time.
struct Names {
    usernames: HashSet<String>,
}

/// `None` until `start()` has read the account files.  While it is `None`
/// no new name can be taken, because we don't know which ones are free.
/// (If `start()` fails the application shuts down, so this is only a second
/// lock on the same door.)
static NAMES: Mutex<Option<Names>> = Mutex::new(None);

/// Lists the account folder and notes which usernames are taken.  main()
/// calls this once Scribe and Constellations are up.  The character names
/// are the game's to learn, from these accounts, right after this.
///
/// Hands back false if the account folder is there and can't be read.  We
/// can't tell which names are taken, so it isn't safe to run, and main()
/// shuts the application down.  A folder that isn't there yet is fine --
/// no folder, no accounts.
///
/// A damaged account file still has its username taken: the file is there,
/// under that name.  What's wrong with it comes out when it gets loaded.
pub fn start() -> bool {
    let mut names = Names {
        usernames: HashSet::new(),
    };

    // Rust note: `fs::read_dir` hands back the folder's entries one at a
    // time.  Listing a folder isn't writing one, so it doesn't need DiskMan.
    // Nothing gets opened here; the names come off the file names.

    let folder = constellations::account_folder();

    match fs::read_dir(&folder) {
        Ok(entries) => {
            for entry in entries {
                let path = match entry {
                    Ok(entry) => entry.path(),
                    Err(_) => continue,
                };

                // Only `.act` files.  A leftover `.act.tmp` ends in `.tmp`,
                // so it is skipped too.
                if path.extension() != Some(OsStr::new(ACCOUNT_EXTENSION)) {
                    continue;
                }

                let username = match path.file_stem() {
                    Some(stem) => stem.to_string_lossy().to_string(),
                    None => continue,
                };
                if check_username(&username) != Ok(username.clone()) {
                    scribe::warn(Channel::Security, &format!("{} isn't named like \
                    an account file. Skipped it.", path.display()));
                    continue;
                }

                names.usernames.insert(username);
            }
        }
        Err(error) => {
            // No folder means no accounts yet.  The first save makes it.
            if error.kind() != io::ErrorKind::NotFound {
                scribe::error(Channel::Security,
                              &format!("Can't read the account folder {}: {}",
                                       folder.display(), error));
                return false;
            }
        }
    }

    scribe::info(Channel::Security, &format!("Found {} account(s).", names.usernames.len()));

    let mut guard = NAMES.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(names);
    true
}

/// Takes a username, so nobody else can.  An `Err` says why not, in words
/// for whoever typed it.
fn reserve_name(username: &str) -> Result<(), String> {
    let mut guard = NAMES.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match guard.as_mut() {
        Some(names) => reserve_in(names, username),
        None => Err("The list of names in use couldn't be loaded, so no new names can be taken.".to_string()),
    }
}

/// Hands a username back: the account is gone, or never made it to the
/// disk.
fn release_name(username: &str) {
    let mut guard = NAMES.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(names) = guard.as_mut() {
        release_in(names, username);
    }
}

/// The part of `reserve_name()` that doesn't need the lock, so the tests can
/// run it on a `Names` of their own.
fn reserve_in(names: &mut Names, username: &str) -> Result<(), String> {
    // `insert` says false when the name was already in there.
    if names.usernames.insert(username.to_string()) {
        Ok(())
    } else {
        Err(format!("There is already an account called {}.", username))
    }
}

fn release_in(names: &mut Names, username: &str) {
    names.usernames.remove(username);
}

/// Every username on the server, in alphabetical order.  Straight from the
/// names list, so it doesn't touch the disk.  Empty if `start()` never ran.
pub fn list_usernames() -> Vec<String> {
    let guard = NAMES.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut usernames: Vec<String> = match guard.as_ref() {
        Some(names) => names.usernames.iter().cloned().collect(),
        None => Vec::new(),
    };
    drop(guard);

    // Rust note: a HashSet keeps no order at all, so we sort the copy.
    usernames.sort();
    usernames
}

// ---------------------------------------------------------------------------
// Making, loading and saving accounts
// ---------------------------------------------------------------------------

/// Makes a new account and writes its file.  For the admin.  The username
/// can come in any case -- it gets stored lowercase.  The password has to
/// follow Security's rules, and only its hash is kept.  The email, the real
/// name and the birthday can all be left empty.
///
/// An `Err` is a message for the admin saying what went wrong.
pub fn create_account(username: &str, password: &str, email: &str, real_name: &str, birthday: &str)
                      -> Result<Account, String> {
    let username = check_username(username)?;
    security::check_password_rules(password)?;
    check_email(email)?;
    check_real_name(real_name)?;
    check_birthday(birthday)?;

    // Take the name first, so two accounts being made at the same moment
    // can't both get it.  If anything after this fails, it goes back.
    reserve_name(&username)?;
    let result =
        write_new_account(username.clone(), password, email, real_name, birthday);
    if result.is_err() {
        release_name(&username);
    }
    result
}

/// The rest of `create_account()`, once the name is ours.  Split out so there
/// is one place that hands the name back when something fails.
fn write_new_account(username: String, password: &str, email: &str, real_name: &str, birthday: &str)
                     -> Result<Account, String> {
    // The list of names should already have caught this.  But if the file is
    // already there, that name is taken, whatever the list says.  And if we
    // couldn't even look (anything but "not found"), we don't make an account
    // on top of a file we couldn't see.  DiskMan has already logged that one.
    match diskman::read_text(&account_path(&username)) {
        Ok(_) => {
            return Err(format!("There is already an account called {}.", username));
        }
        Err(error) => {
            if error.kind() != io::ErrorKind::NotFound {
                return Err(format!("Couldn't check whether {} already exists.", username));
            }
        }
    }

    let account_uid = match new_uuid() {
        Ok(uuid) => uuid,
        Err(error) => return Err(format!("Couldn't make a UUID: {}", error)),
    };

    // About 85 ms.  That is the point of it.
    let password_hash_string = security::hash_password(password)?;

    let account = Account {
        username,
        password_hash_string,
        email: email.to_string(),
        real_name: real_name.to_string(),
        birthday: birthday.to_string(),
        account_uid,
        characters: Vec::new(),
        created_at: now_seconds(),
        last_login: 0,
    };

    if let Err(error) = save_account(&account) {
        return Err(format!("Couldn't save the account file: {}", error));
    }

    scribe::info(Channel::Security, &format!("Account {} created.", account.username));
    Ok(account)
}

/// Reads an account in from its file.
///
/// `Ok(None)` means there is no account by that name.  For a login that is
/// just a wrong guess, not an error, and it gets the same "AUTHENTICATION
/// FAILED" as a wrong password.  `Err` means the file is there and we can't
/// use it.  That has already been logged, and a login should treat it as a
/// plain failure too.
pub fn load_account(username: &str) -> Result<Option<Account>, String> {
    // A name that breaks the rules can't have an account, so it never gets
    // anywhere near a file path.  This is what stops somebody logging in as
    // "../../etc/passwd".
    let username = match check_username(username) {
        Ok(username) => username,
        Err(_) => return Ok(None),
    };

    let text = match diskman::read_text(&account_path(&username)) {
        Ok(text) => text,
        Err(error) => {
            if error.kind() == io::ErrorKind::NotFound {
                return Ok(None);
            }
            // DiskMan logged the details.
            return Err(format!("Couldn't read the account file for {}.", username));
        }
    };

    match account_from_text(&text, &username) {
        Ok(account) => Ok(Some(account)),
        Err(problem) => {
            scribe::error(Channel::Security,
                          &format!("The account file for {} is damaged: {}",
                                   username, problem));
            Err(format!("The account file for {} is damaged.", username))
        }
    }
}

/// Writes an account out to its file, through `diskman::write_file()`.
/// Doesn't come back until the file is safe on the disk.
pub fn save_account(account: &Account) -> io::Result<()> {
    // The username is about to become a file name.  Everything that makes an
    // Account in here has already checked it, but this is the last door
    // before the disk, so it gets checked again.
    let username = match check_username(&account.username) {
        Ok(username) => username,
        Err(problem) => return Err(io::Error::new(io::ErrorKind::InvalidInput, problem)),
    };
    if username != account.username {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,
                                  "A username has to be stored lowercase."));
    }

    let text = match account_to_text(account) {
        Ok(text) => text,
        Err(problem) => return Err(io::Error::new(io::ErrorKind::InvalidData, problem)),
    };

    diskman::write_file(&StratumFile {
        path: account_path(&account.username),
        contents: text.into_bytes(),
    })
}

/// Deletes an account for good: the file goes, and the username is free to
/// be taken again.  For the admin.  An `Err` is a message saying why not,
/// and nothing was deleted.
///
/// The file goes first and the name second.  That way a name is never free
/// while a file that uses it is still on the disk.
///
/// A damaged account file is refused.  Its characters can't be read, so
/// nothing could be done about them.  Somebody has to look at that file by
/// hand.
// TODO(delete-account): the account's player files and its character names
// are the game's, and this can't see the game.  Deleting an account leaves
// its saved/players/<account>/ folder behind and its character names taken
// until the next launch.  Something above both has to do the whole job.
pub fn delete_account(username: &str) -> Result<(), String> {
    let account = match load_account(username) {
        Ok(Some(account)) => account,
        Ok(None) => return Err(format!("There is no account called {}.",
                                       username.to_ascii_lowercase())),
        // load_account() has already logged what is wrong with it.
        Err(problem) => return Err(format!("{}  It wasn't deleted.", problem)),
    };

    match diskman::delete_file(&account_path(&account.username)) {
        Ok(()) => {}
        // Gone between the read and now.  The end result is the same.
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!("Couldn't delete the account file: {}", error));
        }
    }

    release_name(&account.username);

    scribe::info(Channel::Security, &format!("Account {} deleted, with {} character(s).",
                                             account.username, account.characters.len()));
    Ok(())
}

/// Adds a character's reference to the account and saves the account.  The
/// name has already been checked and taken by the game, and the UUID is
/// the one the game gave the character's file.  Nothing is checked here.
/// If the save fails, the reference comes back off again, so what is in
/// memory still matches what is on the disk.
pub fn add_character(account: &mut Account, uuid: &str, name: &str) -> Result<(), String> {
    account.characters.push(CharacterRef {
        uuid: uuid.to_string(),
        name: name.to_string(),
    });

    if let Err(error) = save_account(account) {
        account.characters.pop();
        return Err(format!("Couldn't save the account file: {}", error));
    }

    scribe::info(Channel::Security, &format!("Character {} added to account {}.",
                                             display_name(name), account.username));
    Ok(())
}

/// Takes a character's reference off the account and saves the account.
/// By UUID, since a name can change and a UUID can't.  If the save fails,
/// the reference goes back on.  The character's file and its name are the
/// game's to deal with, before or after this.
pub fn remove_character(account: &mut Account, uuid: &str) -> Result<(), String> {
    let position = match account.characters.iter().position(|c| c.uuid == uuid) {
        Some(position) => position,
        None => return Err(format!("Account {} has no character with the UUID {}.",
                                   account.username, uuid)),
    };
    let removed = account.characters.remove(position);

    if let Err(error) = save_account(account) {
        account.characters.insert(position, removed);
        return Err(format!("Couldn't save the account file: {}", error));
    }

    scribe::info(Channel::Security, &format!("Character {} removed from account {}.",
                                             display_name(&removed.name), account.username));
    Ok(())
}

/// Gives the account a new password and saves it.  For the admin, when a
/// player has forgotten theirs.  The new password has to follow Security's
/// rules, and only its hash is kept, the same as when the account was made.
/// If the save fails, the old hash goes back, so what is in memory still
/// matches what is on the disk, and the old password still works.
pub fn change_password(account: &mut Account, new_password: &str) -> Result<(), String> {
    security::check_password_rules(new_password)?;

    // About 85 ms.
    let new_hash = security::hash_password(new_password)?;
    let old_hash = std::mem::replace(&mut account.password_hash_string, new_hash);

    if let Err(error) = save_account(account) {
        account.password_hash_string = old_hash;
        return Err(format!("Couldn't save the account file: {}", error));
    }

    scribe::info(Channel::Security, &format!("Password changed for account {}.",
                                             account.username));
    Ok(())
}

/// Stamps the account with the time of this login and saves it.  Only for a
/// login that got the password right.
pub fn record_login(account: &mut Account) -> io::Result<()> {
    account.last_login = now_seconds();
    save_account(account)
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

/// Says whether a username is allowed, and hands it back lowercase if it
/// is.  4 to 16 characters: letters, numbers and underscores, and it can't
/// start with an underscore.  An `Err` is a message
/// in words for whoever typed it.
pub fn check_username(username: &str) -> Result<String, String> {
    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("A username can only have letters, numbers and underscores in it.".to_string());
    }
    if username.starts_with('_') {
        return Err("A username can't start with an underscore.".to_string());
    }

    let length = username.chars().count();
    if length < MIN_USERNAME_CHARS || length > MAX_USERNAME_CHARS {
        return Err(format!("A username needs {} to {} characters.",
                           MIN_USERNAME_CHARS, MAX_USERNAME_CHARS));
    }

    Ok(username.to_ascii_lowercase())
}

/// Says whether an email address is allowed.  Empty is fine -- not every
/// account needs one.  Otherwise it is only a sanity check: one `@` with
/// something on both sides, no spaces, plain ASCII.  It doesn't prove the
/// address works.  Nothing but sending a mail to it can do that.
pub fn check_email(email: &str) -> Result<(), String> {
    if email.is_empty() {
        return Ok(());
    }

    if email.chars().count() > MAX_EMAIL_CHARS {
        return Err(format!("An email address can't be longer than {} characters.",
                           MAX_EMAIL_CHARS));
    }

    // `is_ascii_graphic()` is anything printable except the space.
    if !email.chars().all(|c| c.is_ascii_graphic()) {
        return Err("An email address can't have spaces or unusual characters in it.".to_string());
    }

    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err("That doesn't look like an email address.".to_string());
    }

    Ok(())
}

/// Says whether a real name is allowed.  Empty is fine.  Otherwise up to 64
/// characters of anything but control characters (tabs, newlines and the
/// like).  Real names have spaces, hyphens, apostrophes and accents in them,
/// so none of those are refused.
pub fn check_real_name(real_name: &str) -> Result<(), String> {
    if real_name.chars().count() > MAX_REAL_NAME_CHARS {
        return Err(format!("A real name can't be longer than {} characters.",
                           MAX_REAL_NAME_CHARS));
    }
    if real_name.chars().any(|c| c.is_control()) {
        return Err("A real name can't have tabs or line breaks in it.".to_string());
    }
    Ok(())
}

/// Says whether a birthday is allowed.  Empty is fine.  Otherwise it has to
/// be "MM-DD", two digits each, and a day that month actually has.  The 29th
/// of February is allowed, since there is no year to say it isn't.
pub fn check_birthday(birthday: &str) -> Result<(), String> {
    if birthday.is_empty() {
        return Ok(());
    }

    let problem = "A birthday is the month and the day, like 03-14.".to_string();

    // Digit by digit, and not with `parse()`, because `parse()` would also
    // take things like "+3".
    let bytes = birthday.as_bytes();
    if bytes.len() != 5 || bytes[2] != b'-' {
        return Err(problem);
    }
    for position in [0, 1, 3, 4] {
        if !bytes[position].is_ascii_digit() {
            return Err(problem);
        }
    }

    // Rust note: `b'0'` is the byte for the character 0.  Taking it away
    // from a digit's byte leaves the digit's value.
    let month = (bytes[0] - b'0') as u32 * 10 + (bytes[1] - b'0') as u32;
    let day = (bytes[3] - b'0') as u32 * 10 + (bytes[4] - b'0') as u32;

    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 29,
        _ => return Err(problem),
    };
    if day < 1 || day > days_in_month {
        return Err(problem);
    }

    Ok(())
}

/// A stored name the way the game shows it: "aldric" comes out "Aldric".
pub fn display_name(name: &str) -> String {
    let mut shown = String::new();
    for (position, c) in name.chars().enumerate() {
        if position == 0 {
            shown.push(c.to_ascii_uppercase());
        } else {
            shown.push(c);
        }
    }
    shown
}

// ---------------------------------------------------------------------------
// The file
// ---------------------------------------------------------------------------

fn account_path(username: &str) -> PathBuf {
    constellations::account_folder().join(format!("{}.{}", username, ACCOUNT_EXTENSION))
}

/// The account as the text that goes in its file.  Indented JSON, so a
/// person can read it.
fn account_to_text(account: &Account) -> Result<String, String> {
    match serde_json::to_string_pretty(account) {
        Ok(mut text) => {
            text.push('\n');
            Ok(text)
        }
        Err(error) => Err(error.to_string()),
    }
}

/// The text of an account file back into an Account.  `username` is the
/// name the file was found under, and the file has to agree with it.  This
/// doesn't touch the disk or log, which is what lets the tests run it.
fn account_from_text(text: &str, username: &str) -> Result<Account, String> {
    let account: Account = match serde_json::from_str(text) {
        Ok(account) => account,
        // Only the line and the column.  serde_json's own message can quote
        // a value out of the file, and the value could be the stored hash,
        // which has no business in a log.
        Err(error) => {
            return Err(format!("it isn't an account file serde can read (line {}, column {})",
                               error.line(), error.column()));
        }
    };

    if account.username != username {
        return Err(format!("it says it belongs to {}", account.username));
    }

    Ok(account)
}

// ---------------------------------------------------------------------------
// Small pieces
// ---------------------------------------------------------------------------

fn now_seconds() -> u64 {
    // Only fails on a clock set before 1970.  Same answer as Scribe gives.
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since_1970) => since_1970.as_secs(),
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_account() -> Account {
        Account {
            username: "jacob".to_string(),
            // Not a real hash.  A real one costs 85 ms, and all we need here
            // is something with the same $ and = in it.
            password_hash_string:
            "$argon2id$v=19$m=65536,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2g".to_string(),
            email: "jacob@example.com".to_string(),
            real_name: "Jacob Chacko".to_string(),
            birthday: "03-14".to_string(),
            account_uid: "0c4e1f2a-6b3d-4e5f-a1b2-c3d4e5f60718".to_string(),
            characters: vec![
                CharacterRef { uuid: "3f2a91c0-e4b7-4d1a-9c0e-2b7f5a6d8e10".to_string(),
                    name: "aldric".to_string() },
                CharacterRef { uuid: "9b1c0e2d-77a4-4f10-8b3e-5d6c7a8b9c0d".to_string(),
                    name: "mira".to_string() },
            ],
            created_at: 1790021647,
            last_login: 0,
        }
    }

    #[test]
    fn usernames() {
        assert_eq!(check_username("jacob"), Ok("jacob".to_string()));
        assert_eq!(check_username("Jacob"), Ok("jacob".to_string()));
        assert_eq!(check_username("JACOB99"), Ok("jacob99".to_string()));
        assert_eq!(check_username("1234"), Ok("1234".to_string()));
        assert_eq!(check_username("sixteencharsxxxx"), Ok("sixteencharsxxxx".to_string()));

        assert!(check_username("abc").is_err());
        assert!(check_username("seventeencharsxxx").is_err());
        assert!(check_username("").is_err());
        assert_eq!(check_username("Jacob_C"), Ok("jacob_c".to_string()));
        assert_eq!(check_username("chacko_82"), Ok("chacko_82".to_string()));
        assert_eq!(check_username("chacko__"), Ok("chacko__".to_string()));
        assert!(check_username("_chacko82").is_err());
        assert!(check_username("____").is_err());
        assert!(check_username("jacob-c").is_err());
        assert!(check_username("jacob c").is_err());
        assert!(check_username("../../etc").is_err());
        assert!(check_username("jos\u{e9}").is_err());
    }

    #[test]
    fn emails() {
        assert_eq!(check_email(""), Ok(()));
        assert_eq!(check_email("jacob@example.com"), Ok(()));
        assert_eq!(check_email("a@b"), Ok(()));

        assert!(check_email("jacob").is_err());
        assert!(check_email("@example.com").is_err());
        assert!(check_email("jacob@").is_err());
        assert!(check_email("ja@cob@example.com").is_err());
        assert!(check_email("jacob @example.com").is_err());
        assert!(check_email(&format!("{}@example.com", "x".repeat(250))).is_err());
    }

    #[test]
    fn real_names() {
        assert_eq!(check_real_name(""), Ok(()));
        assert_eq!(check_real_name("Jacob Chacko"), Ok(()));
        assert_eq!(check_real_name("Mary-Ann O'Neil"), Ok(()));
        assert_eq!(check_real_name("Jos\u{e9}"), Ok(()));

        assert!(check_real_name("Jacob\nChacko").is_err());
        assert!(check_real_name("Jacob\tChacko").is_err());
        assert!(check_real_name(&"x".repeat(65)).is_err());
    }

    #[test]
    fn birthdays() {
        assert_eq!(check_birthday(""), Ok(()));
        assert_eq!(check_birthday("03-14"), Ok(()));
        assert_eq!(check_birthday("12-31"), Ok(()));
        assert_eq!(check_birthday("02-29"), Ok(()));

        assert!(check_birthday("02-30").is_err());
        assert!(check_birthday("04-31").is_err());
        assert!(check_birthday("13-01").is_err());
        assert!(check_birthday("00-10").is_err());
        assert!(check_birthday("01-00").is_err());
        assert!(check_birthday("3-14").is_err());
        assert!(check_birthday("03/14").is_err());
        assert!(check_birthday("+3-14").is_err());
        assert!(check_birthday("1990-03-14").is_err());
        assert!(check_birthday("\u{e9}3-14").is_err());
    }

    #[test]
    fn names_are_taken_once() {
        let mut names = Names {
            usernames: HashSet::new(),
        };

        assert_eq!(reserve_in(&mut names, "jacob"), Ok(()));
        assert!(reserve_in(&mut names, "jacob").is_err());

        // Handed back, it can be taken again.
        release_in(&mut names, "jacob");
        assert_eq!(reserve_in(&mut names, "jacob"), Ok(()));
    }

    #[test]
    fn display_names() {
        assert_eq!(display_name("aldric"), "Aldric");
        assert_eq!(display_name("m"), "M");
        assert_eq!(display_name(""), "");
    }

    #[test]
    fn account_survives_the_file() {
        let account = sample_account();
        let text = account_to_text(&account).unwrap();
        let back = account_from_text(&text, "jacob").unwrap();

        assert_eq!(back, account);
        assert!(text.contains("\"password_hash_string\": \"$argon2id$v=19$m=65536,t=2,p=1$"));
    }

    #[test]
    fn file_under_the_wrong_name_is_refused() {
        let text = account_to_text(&sample_account()).unwrap();
        assert!(account_from_text(&text, "someoneelse").is_err());
    }

    #[test]
    fn damaged_files_are_refused() {
        assert!(account_from_text("", "jacob").is_err());
        assert!(account_from_text("{ \"username\": \"jacob\" }", "jacob").is_err());

        // Half a file.
        let text = account_to_text(&sample_account()).unwrap();
        assert!(account_from_text(&text[..text.len() / 2], "jacob").is_err());
    }

    #[test]
    fn damage_report_never_quotes_the_hash() {
        // The hash moved into a field that wants a number.  serde_json's own
        // message would quote it back at us.  Ours mustn't.
        let text =
            "{ \"username\": \"jacob\", \
            \"password_hash_string\": \"x\",\
             \"email\": \"\", \
             \"real_name\": \"\", \
             \"birthday\": \"\", \
             \"account_uid\": \"x\", \
             \"characters\": [], \
               \"created_at\": \"$argon2id$v=19$secret\", \
               \"last_login\": 0 }";
        let problem = account_from_text(text, "jacob").unwrap_err();
        assert!(!problem.contains("argon2"));
    }
}