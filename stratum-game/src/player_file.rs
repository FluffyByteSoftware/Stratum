//! File:     stratum-game/src/player_file.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! The player file.  One JSON file per character, at
//! saved/players/<account>/<character>.plyr inside the content folder, and
//! it holds everything about the character that has to survive a logout or
//! a shutdown.  Flat files, one per player, in the LPC tradition.
//!
//! The account file only holds a reference to each character: its UUID and
//! its name.  This file is the character.  The two are joined by the UUID,
//! and loading checks that the file's UUID is the one the account points
//! at, and that its shortname is the file's name.
//!
//! A player file is always written with `diskman::write_later()`, never
//! `write_file()`.  DiskMan says never both on one path, and this is the
//! path that gets saved often once the game is running.
//!
//! This file will grow.  Effects, an inventory, a class.  So a field that
//! is missing from an older file gets a default instead of refusing the
//! file, and a character saved today still loads after the next field
//! arrives.  Only the UUID and the shortname can't be missing.
//!
//! What's on the disk today:
//!
//! ```text
//! {
//!   "uuid": "3f2a91c0-e4b7-4d1a-9c0e-2b7f5a6d8e10",
//!   "shortname": "aldric",
//!   "longname": "Aldric",
//!   "health": 100,
//!   "max_health": 100,
//!   "position": { "x": 0.0, "y": 0.0, "z": 0.0 },
//!   "rotation": { "x": 0.0, "y": 0.0, "z": 0.0, "w": 1.0 },
//!   "saved_at": 1790000000
//! }
//! ```
//!
//! The bottom of the file is the bridge to the world: `spawn()` makes an
//! actor out of a player file, and `read_back()` makes a player file out of
//! an actor, ready to save.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy_ecs::prelude::{Entity, World};
use serde::{Deserialize, Serialize};
use stratum_tools::{account, constellations, diskman};
use stratum_tools::diskman::StratumFile;
use stratum_tools::scribe::{self, Channel};

use crate::actor::{Actor, ActorName, Health, Player, Position, Rotation};

/// The file's extension, without the dot.
const PLAYER_EXTENSION: &str = "plyr";

/// Where a new character starts.  Placeholders until there is a world to
/// stand in and a class to say how tough it is.
const STARTING_HEALTH: i32 = 100;

/// A player file as it is on the disk.  Everything in it is plain data.
/// Turning it into an actor is `spawn()`'s job.
// Rust note: `#[serde(default)]` on a field means "if the file doesn't have
// this one, use the type's default".  `default = "full_health"` names a
// function to get the default from instead.  The two fields without either
// are the ones a file can't do without.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerFile {
    pub uuid: String,
    pub shortname: String,
    #[serde(default)]
    pub longname: String,
    #[serde(default = "full_health")]
    pub health: i32,
    #[serde(default = "full_health")]
    pub max_health: i32,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub rotation: Rotation,
    /// When the file was last saved, in seconds since 1970.
    #[serde(default)]
    pub saved_at: u64,
}

fn full_health() -> i32 {
    STARTING_HEALTH
}

/// A player file for a brand new character: full health, standing at the
/// origin, facing straight ahead, with its name shown capitalized.
pub fn new_player(uuid: &str, shortname: &str) -> PlayerFile {
    PlayerFile {
        uuid: uuid.to_string(),
        shortname: shortname.to_string(),
        longname: account::display_name(shortname),
        health: STARTING_HEALTH,
        max_health: STARTING_HEALTH,
        position: Position::default(),
        rotation: Rotation::default(),
        saved_at: 0,
    }
}

// ---------------------------------------------------------------------------
// Where the file lives
// ---------------------------------------------------------------------------

/// The path of a character's file: saved/players/<account>/<shortname>.plyr
/// inside the content folder.
pub fn player_path(account: &str, shortname: &str) -> PathBuf {
    player_path_in(&constellations::player_folder(), account, shortname)
}

/// The same, under whatever folder is handed in.  This is the one the tests
/// use, so they never ask Constellations for anything.
fn player_path_in(folder: &Path, account: &str, shortname: &str) -> PathBuf {
    folder.join(account).join(format!("{}.{}", shortname, PLAYER_EXTENSION))
}

/// True for a name that is safe to use as part of a file path: letters,
/// digits and underscores only.  The account and the character name have
/// both been checked properly before they get here, but a name is about to
/// become a folder or a file, and this is what stops "../" from ever being
/// one, whatever went wrong upstream.
fn is_plain(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ---------------------------------------------------------------------------
// Saving and loading
// ---------------------------------------------------------------------------

/// Hands a player file to DiskMan's cache and comes straight back.  The
/// `saved_at` written to the disk is now, whatever the struct said.
///
/// `write_later()` can't say whether the file landed.  If it can't be
/// written, DiskMan logs it and drops it.  Anything that has to know the
/// file is on the disk (a character's very first save) calls
/// `diskman::flush()` afterwards and looks for the file.
pub fn save(file: &PlayerFile, account: &str) -> Result<(), String> {
    if !is_plain(account) || !is_plain(&file.shortname) {
        return Err(format!("Can't save a player file for {} on {}.", file.shortname, account));
    }

    let mut stamped = file.clone();
    stamped.saved_at = now_seconds();

    let text = player_text(&stamped)?;
    diskman::write_later(StratumFile {
        path: player_path(account, &file.shortname),
        contents: text.into_bytes(),
    });
    Ok(())
}

/// Reads a character's file back.  The `uuid` is the one the account points
/// at, and the file has to agree with it.
///
/// `Ok(None)` means there is no file, which is a real answer: the account
/// can point at a character whose first save never landed.  `Err` means
/// there is a file and it can't be trusted, and the details are in the log.
pub fn load(account: &str, shortname: &str, uuid: &str) -> Result<Option<PlayerFile>, String> {
    if !is_plain(account) || !is_plain(shortname) {
        return Ok(None);
    }

    let text = match diskman::read_text(&player_path(account, shortname)) {
        Ok(text) => text,
        Err(error) => {
            if error.kind() == io::ErrorKind::NotFound {
                return Ok(None);
            }
            // DiskMan logged the details.
            return Err(format!("Couldn't read the player file for {}.", shortname));
        }
    };

    match player_from_text(&text, shortname, uuid) {
        Ok(file) => Ok(Some(file)),
        Err(problem) => {
            scribe::error(Channel::World,
                          &format!("The player file for {} on {} is damaged: {}",
                                   shortname, account, problem));
            Err(format!("The player file for {} is damaged.", shortname))
        }
    }
}

/// A player file as the text that goes on the disk: JSON, indented, like
/// the account file.
pub fn player_text(file: &PlayerFile) -> Result<String, String> {
    match serde_json::to_string_pretty(file) {
        Ok(text) => Ok(text),
        Err(error) => Err(format!("Couldn't turn the player file for {} into JSON: {}",
                                  file.shortname, error)),
    }
}

/// The text of a player file, checked and turned back into a PlayerFile.
/// Missing fields get their defaults here.  A file under the wrong name, or
/// with the wrong UUID, is refused: that's a file that has been moved or
/// edited by hand, and the safe thing is to say so.
fn player_from_text(text: &str, shortname: &str, uuid: &str) -> Result<PlayerFile, String> {
    let mut file: PlayerFile = match serde_json::from_str(text) {
        Ok(file) => file,
        Err(error) => return Err(format!("it isn't a player file ({})", error)),
    };

    if file.shortname != shortname {
        return Err(format!("it says it is {}", file.shortname));
    }
    if file.uuid != uuid {
        return Err(format!("its UUID is {}, and the account points at {}", file.uuid, uuid));
    }

    // An old file with no longname shows the shortname capitalized, the way
    // a new character does.
    if file.longname.is_empty() {
        file.longname = account::display_name(&file.shortname);
    }

    Ok(file)
}

// ---------------------------------------------------------------------------
// The bridge to the world
// ---------------------------------------------------------------------------

/// Puts an actor into the world from a player file, and hands back its
/// entity.  The account's username and UUID go into the Player component,
/// because the file doesn't hold them: the file knows which character it
/// is, and the account knows whose.  The network parts of Player stay empty
/// until a login fills them in.
///
/// The health goes through `Health::new()`, so a file that says 150 out of
/// 100 comes into the world as 100 out of 100.
pub fn spawn(world: &mut World, file: &PlayerFile, account: &str, account_uid: &str) -> Entity {
    world.spawn((
        Actor,
        ActorName {
            shortname: file.shortname.clone(),
            longname: file.longname.clone(),
        },
        file.position,
        file.rotation,
        Health::new(file.health, file.max_health),
        Player {
            uuid: file.uuid.clone(),
            account_uid: account_uid.to_string(),
            account: account.to_string(),
            last_ip: None,
            current_ip: None,
            udp_port: None,
        },
    )).id()
}

/// Reads an actor back out of the world as a player file, ready for
/// `save()`.  `None` means the entity isn't a player, or is missing one of
/// the pieces a player has, and there is nothing to save.
// Rust note: the `?` after each `world.get()` hands back `None` on the spot
// if that component isn't there, so the function reads as five lookups and
// a struct instead of five nested `match`es.
pub fn read_back(world: &World, entity: Entity) -> Option<PlayerFile> {
    let player = world.get::<Player>(entity)?;
    let name = world.get::<ActorName>(entity)?;
    let position = world.get::<Position>(entity)?;
    let rotation = world.get::<Rotation>(entity)?;
    let health = world.get::<Health>(entity)?;

    Some(PlayerFile {
        uuid: player.uuid.clone(),
        shortname: name.shortname.clone(),
        longname: name.longname.clone(),
        health: health.current(),
        max_health: health.max(),
        position: *position,
        rotation: *rotation,
        saved_at: 0,
    })
}

// ---------------------------------------------------------------------------
// Small pieces
// ---------------------------------------------------------------------------

fn now_seconds() -> u64 {
    // Only fails on a clock set before 1970.  Same answer as the account
    // and Scribe give.
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

    const UUID: &str = "3f2a91c0-e4b7-4d1a-9c0e-2b7f5a6d8e10";

    #[test]
    fn a_new_character_starts_fresh() {
        let file = new_player(UUID, "aldric");

        assert_eq!(file.uuid, UUID);
        assert_eq!(file.shortname, "aldric");
        assert_eq!(file.longname, "Aldric");
        assert_eq!(file.health, STARTING_HEALTH);
        assert_eq!(file.max_health, STARTING_HEALTH);
        assert_eq!(file.position, Position::default());
        assert_eq!(file.rotation, Rotation::default());
    }

    #[test]
    fn the_path_is_the_account_folder_and_the_character_file() {
        let path = player_path_in(Path::new("/opt/stratum/content/saved/players"), "jacob", "aldric");
        assert_eq!(path, PathBuf::from("/opt/stratum/content/saved/players/jacob/aldric.plyr"));
    }

    #[test]
    fn names_that_could_wander_are_not_plain() {
        assert!(is_plain("aldric"));
        assert!(is_plain("jacob_2"));
        assert!(!is_plain(""));
        assert!(!is_plain("../etc"));
        assert!(!is_plain("a/b"));
        assert!(!is_plain("aldric.plyr"));
    }

    #[test]
    fn what_we_write_we_can_read() {
        let mut file = new_player(UUID, "aldric");
        file.longname = "Aldric the Unwashed".to_string();
        
        file.health = 42;
        
        file.position = Position { 
            x: 1.5, 
            y: -2.0, 
            z: 300.25 
        };
        
        file.rotation = Rotation { 
            x: 0.0, 
            y: std::f32::consts::FRAC_1_SQRT_2, 
            z: 0.0,
            w: std::f32::consts::FRAC_1_SQRT_2 
        };
        
        file.saved_at = 1790000000;

        let text = player_text(&file).unwrap();
        let back = player_from_text(&text, "aldric", UUID).unwrap();

        assert_eq!(back, file);
    }

    #[test]
    fn an_older_file_gets_defaults_for_what_it_lacks() {
        let text = format!("{{ \"uuid\": \"{}\", \"shortname\": \"aldric\" }}", UUID);
        let file = player_from_text(&text, "aldric", UUID).unwrap();

        assert_eq!(file, new_player(UUID, "aldric"));
    }

    #[test]
    fn a_file_without_its_uuid_or_name_is_refused() {
        let no_uuid = "{ \"shortname\": \"aldric\" }";
        assert!(player_from_text(no_uuid, "aldric", UUID).is_err());

        let no_name = format!("{{ \"uuid\": \"{}\" }}", UUID);
        assert!(player_from_text(&no_name, "aldric", UUID).is_err());

        assert!(player_from_text("this is not json", "aldric", UUID).is_err());
    }

    #[test]
    fn a_file_under_the_wrong_name_or_uuid_is_refused() {
        let text = player_text(&new_player(UUID, "aldric")).unwrap();

        assert!(player_from_text(&text, "bertram", UUID).is_err());
        assert!(player_from_text(&text, "aldric", "00000000-0000-4000-8000-000000000000").is_err());
        assert!(player_from_text(&text, "aldric", UUID).is_ok());
    }

    #[test]
    fn a_player_goes_into_the_world_and_comes_back_out() {
        let mut file = new_player(UUID, "aldric");
        file.health = 150;
        file.position = Position { x: 10.0, y: 20.0, z: 30.0 };

        let mut world = World::new();
        let entity = spawn(&mut world, &file, "jacob", "account-uid");

        let player = world.get::<Player>(entity).unwrap();
        assert_eq!(player.uuid, UUID);
        assert_eq!(player.account, "jacob");
        assert_eq!(player.account_uid, "account-uid");
        assert_eq!(player.current_ip, None);
        assert!(world.get::<Actor>(entity).is_some());

        let back = read_back(&world, entity).unwrap();
        // 150 out of 100 came into the world as 100 out of 100.
        assert_eq!(back.health, 100);
        assert_eq!(back.max_health, 100);
        assert_eq!(back.position, file.position);
        assert_eq!(back.longname, "Aldric");
    }

    #[test]
    fn an_entity_that_is_not_a_player_has_nothing_to_save() {
        let mut world = World::new();
        let entity = world.spawn((Actor, Position::default())).id();

        assert_eq!(read_back(&world, entity), None);
    }
}