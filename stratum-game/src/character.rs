//! File:     stratum-game/src/character.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! Making a character and deleting one.  This is where the pieces meet:
//! the name rules and the names list (names.rs), the UUID (Fingerprinter),
//! the player file (player_file.rs) and the account's reference to the
//! character (account.rs).  The Launcher's Add Character calls in here,
//! and later so will character creation over the network.
//!
//! The order matters, because the two files are written two different ways.
//! The player file goes through `write_later()`, which can't say whether it
//! landed, and the account goes through `write_file()`, which can.  So the
//! player file goes first, we wait for DiskMan to empty its cache, we read
//! the file back, and only then does the account get its reference.  If
//! the file never landed, the account never points at it.  If the account's
//! save fails after that, the player file gets deleted again.  Either way,
//! what is on the disk agrees with itself.
//!
//! The wait costs about 40 ms on the mechanical drive.  Making a character
//! is rare, and 40 ms is nothing to the admin at the menu.
//!
//! Nothing in here has a unit test.  Every function touches the names list,
//! DiskMan and Constellations, which tests aren't allowed near.  The test is
//! the Launcher: make a character, `cat` its file, restart, and see that the
//! name is still taken.

use stratum_tools::account::{self, Account};
use stratum_tools::{diskman, fingerprinter};
use stratum_tools::scribe::{self, Channel};

use crate::names;
use crate::player_file;

/// Makes a new character on an account: checks and takes the name, gives
/// it a UUID, writes its player file, waits for the file to land, and adds
/// the reference to the account.  Hands back the character's UUID.  An
/// `Err` is a message in words for whoever asked, and nothing is left
/// behind: the name is free again and no file points anywhere.
pub fn create_character(account: &mut Account, name: &str) -> Result<String, String> {
    // A full account is turned away before the name is even looked at.
    if account.characters.len() >= account::MAX_CHARACTERS {
        return Err(format!("This account already has {} characters, \
        which is the most there can be.", account::MAX_CHARACTERS));
    }

    let name = names::check_character_name(name)?;
    // Take the name first, so two characters being made at the same moment
    // can't both get it.  If anything after this fails, it goes back.
    names::reserve_name(&name)?;
    let result = write_new_character(account, &name);
    if result.is_err() {
        names::release_name(&name);
    }
    result
}

/// The rest of `create_character()`, once the name is ours.  Split out so
/// there is one place that hands the name back when something fails.
fn write_new_character(account: &mut Account, name: &str) -> Result<String, String> {
    let uuid = match fingerprinter::new_uuid() {
        Ok(uuid) => uuid,
        Err(error) => return Err(format!("Couldn't make a UUID: {}", error)),
    };

    let file = player_file::new_player(&uuid, name);
    player_file::save(&file, &account.username)?;

    // write_later() has come back, but the file is only in the cache.  Wait
    // for the writer to get it to the disk, then make sure it did.
    diskman::flush();
    match player_file::load(&account.username, name, &uuid) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return Err(format!("The player file for {} never reached the disk.  \
                                The log says why.", account::display_name(name)));
        }
        Err(problem) => return Err(problem),
    }

    if let Err(problem) = account::add_character(account, &uuid, name) {
        // The account doesn't point at the file, so the file goes.  If even
        // that fails, DiskMan has logged it, and the file is harmless: nothing
        // points at it, and the name is about to be free again.
        let _ = diskman::delete_file(&player_file::player_path(&account.username, name));
        return Err(problem);
    }

    scribe::info(Channel::World, &format!("Character {} made on account {} ({}).",
                                          account::display_name(name), account.username, uuid));
    Ok(uuid)
}

/// Deletes a character for good: its player file goes, the account's
/// reference goes, and its name is free to be taken again.  For the admin.
/// An `Err` is a message saying why not.
///
/// The file goes first and the reference second, the same way an account
/// is deleted.  If the file can't be deleted, nothing has changed.  If the
/// account's save fails after the file is gone, the account points at a
/// character with no file, which `player_file::load()` treats as no
/// character at all, and the next try at deleting it will finish the job.
pub fn delete_character(account: &mut Account, name: &str) -> Result<(), String> {
    let name = name.to_ascii_lowercase();
    let uuid = match account.characters.iter().find(|c| c.name == name) {
        Some(character) => character.uuid.clone(),
        None => return Err(format!("Account {} has no character called {}.",
                                   account.username, account::display_name(&name))),
    };

    // A save of this file could still be sitting in the cache, and
    // delete_file() refuses a path that is.  Let it land first.
    diskman::flush();
    match diskman::delete_file(&player_file::player_path(&account.username, &name)) {
        Ok(()) => {}
        // No file is fine: a first save that never landed leaves exactly this.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Couldn't delete the player file: {}", error)),
    }

    account::remove_character(account, &uuid)?;
    names::release_name(&name);

    scribe::info(Channel::World, &format!("Character {} deleted from account {}.",
                                          account::display_name(&name), account.username));
    Ok(())
}

/// Checks that a character can be played: the account points at it, and
/// its player file is there and can be trusted.  The network asks this
/// before it hands a player a login token.  An `Err` is a message for the
/// player.
pub fn check_character(account: &Account, name: &str) -> Result<(), String> {
    let name = name.to_ascii_lowercase();
    let shown = account::display_name(&name);
    let uuid = match account.characters.iter().find(|c| c.name == name) {
        Some(character) => character.uuid.clone(),
        None => return Err(format!("There is no character called {} on this account.", shown)),
    };

    match player_file::load(&account.username, &name, &uuid) {
        Ok(Some(_)) => Ok(()),
        // The account points at a file that isn't there.  A first save that
        // never landed leaves exactly this, and nothing has said so yet.
        Ok(None) => {
            scribe::warn(Channel::World, &format!("Account {} points at {}, and {} has no player file.",
                                                  account.username, shown, shown));
            Err(format!("{} can't be played right now.  Tell an admin.", shown))
        }
        // load() has already logged what is wrong with it.
        Err(_) => Err(format!("{} can't be played right now.  Tell an admin.", shown)),
    }
}