//! File:     stratum-tools/src/constellations.rs
//! Project:  Stratum Core
//! Author:   Jacob Chacko
//!
//! Constellations, the configuration.  At launch, reads the Stratum general
//! config file and keeps the settings where anything in the server can get at
//! them.  If there is no file, it writes one with the defaults in it.  At
//! shutdown writes whatever settings are in memory back to the file.
//!
//! The file is plain text, one KEY=VALUE per line, and we parse it by hand.
//! Everything we have is flat, so TOML would have cost us two crates to read
//! a dozen lines.
//!
//! A bad value never stops the server.  We complain through Scribe, keep the
//! default for that one setting, and carry on.
//!
//! main() calls `constellations::load()` right after Scribe is up, and
//! `constellations::save()` on the way out.  Reading a setting looks like this:
//!
//! ```text
//! let port = constellations::get().tcp_port;
//! ```
//!
//! Constellations never touches the disk itself.  Reading and writing the
//! file both go through DiskMan, which does the temp-file-and-rename dance
//! and says what went wrong if anything does.
//!
//! The file gets written whole every time we save.  So a comment somebody
//! added by hand is gone after the next shutdown, and so is any line we
//! complained about at launch.  The values survive.  The decoration doesn't.
//!
//! While the server runs, the Launcher's config menu can show the settings,
//! change one (`set()`), or read the file again (`load()` a second time).
//! CONTENT_FOLDER is the one setting that can't change on a running server.
//!
//! Constellations also knows the content folder's layout: which folders go
//! inside it, what they are called, and how to make them if they are
//! missing.  So nothing else in the server has to know where anything lives.
//!
//! Adding a setting means touching four places, all of them in this file: the
//! Settings struct, default_settings(), apply_setting() and file_text().

use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::diskman::{self, StratumFile};
use crate::scribe::{self, Channel, Color};

// ---------------------------------------------------------------------------
// Where the file lives
// ---------------------------------------------------------------------------

/// The folder the config files go in.  This can't be a setting, because the
/// config file can't tell us where the config file is.
const CONFIG_FOLDER: &str = "/opt/stratum/content/config";

/// The general config file.  It gets a folder to itself because "general"
/// suggests it won't be the only config file forever.
const CONFIG_FILE_NAME: &str = "stratum.conf";

fn config_path() -> PathBuf {
    PathBuf::from(CONFIG_FOLDER).join(CONFIG_FILE_NAME)
}

// ---------------------------------------------------------------------------
// Inside the content folder
// ---------------------------------------------------------------------------

// The folders we expect to find inside CONTENT_FOLDER.  The names are fixed.
// Only the content folder itself is a setting.
const LOG_SUBFOLDER: &str = "logs";
const ACCOUNT_SUBFOLDER: &str = "accounts";
const SSL_SUBFOLDER: &str = "saved/ssl";
const PLAYER_SUBFOLDER: &str = "saved/players";

// ---------------------------------------------------------------------------
// The settings
// ---------------------------------------------------------------------------

/// Every global setting the server has.  The names match the keys in the
/// config file, just in lowercase.
// Rust note: `Clone` is what lets get() hand out a copy of the whole struct.
// `PartialEq` lets the tests compare two of them with `==`.
#[derive(Clone, PartialEq)]
pub struct Settings {
    /// The folder everything the server keeps on disk goes in, apart from the
    /// config file.  Read once at launch.  See log_folder() and friends for
    /// what goes inside it.
    pub content_folder: PathBuf,
    /// The biggest a log file gets before Scribe starts a new one, in MB.
    pub max_log_size_mb: u64,
    /// The address the TCP side (authentication) listens on.
    pub tcp_host_address: IpAddr,
    pub tcp_port: u16,
    /// The address the UDP side (game traffic) listens on.
    pub udp_host_address: IpAddr,
    pub udp_port: u16,
    /// When this is false, Scribe drops Debug messages completely.
    pub show_debug: bool,
    pub color_debug: Color,
    pub color_info: Color,
    pub color_warn: Color,
    pub color_error: Color,
}

/// The built-in values.  These are what goes into a freshly generated config
/// file, and what a setting falls back to when the file has a bad value (or
/// no value) for it.
// TODO(defaults): these are the dev machine's values.  Once testing is done
// they turn into something more generic.
fn default_settings() -> Settings {
    Settings {
        content_folder: PathBuf::from("/opt/stratum/content"),
        max_log_size_mb: 500,
        tcp_host_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 84)),
        tcp_port: 9997,
        udp_host_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 84)),
        udp_port: 9998,
        show_debug: true,
        color_debug: Color::Green,
        color_info: Color::White,
        color_warn: Color::Yellow,
        color_error: Color::Red,
    }
}

// ---------------------------------------------------------------------------
// The one global copy
// ---------------------------------------------------------------------------

/// Constellations' working state.  There is exactly one of these, in
/// CONSTELLATIONS below.
#[derive(Clone)]
struct Constellations {
    settings: Settings,
    /// Set when there is a config file and we couldn't read it.  save() won't
    /// write while this is set.  If we never saw what was in the file, we have
    /// no business writing over it.
    file_unreadable: bool,
    /// The CONTENT_FOLDER that save() writes to the file.  Usually the same
    /// as the one in `settings`.  It differs when the file was edited and
    /// reloaded on a running server -- the server keeps its old folder, and
    /// the new one waits in here for the next launch.
    next_content_folder: PathBuf,
}

// Same arrangement as Scribe: a file-scope global behind a lock.  `None`
// means load() hasn't run yet.
static CONSTELLATIONS: Mutex<Option<Constellations>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// What the rest of the server calls
// ---------------------------------------------------------------------------

/// Reads the config file, or writes one if there isn't one.  main() calls
/// this once, right after Scribe is up.
///
/// Nothing in here can stop the server.  The worst case is that we can't
/// read the file and can't write one either, and then we say so in the Error
/// color and run on the built-in defaults.
///
/// Calling it a second time reads the file again and replaces the settings,
/// including anything `set()` changed since.  The Launcher's Reload does
/// that, so a hand edit to the file can be picked up without a restart.
/// Everything but CONTENT_FOLDER, which stays what it was at launch.
pub fn load() {
    let path = config_path();
    let mut settings = default_settings();
    let mut file_unreadable = false;

    // Whatever content folder this run started with, if it has started.
    let running_folder = {
        let guard = CONSTELLATIONS.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.as_ref().map(|constellations| constellations.settings.content_folder.clone())
    };

    match diskman::read_text(&path) {
        Ok(text) => {
            let problems = parse_text(&text, &mut settings);
            for problem in &problems {
                scribe::warn(Channel::Core,
                             &format!("{}, {}", path.display(), problem));
            }
            scribe::info(
                Channel::Core,
                &format!("Constellations loaded {}", path.display()),
            );
        }
        Err(error) => {
            if error.kind() == io::ErrorKind::NotFound {
                write_default_file(&settings);
            } else {
                // The file is there and we can't read it (permissions, most
                // likely).  We don't write over it, now or at shutdown.
                // Somebody's settings are in there.  DiskMan has already
                // said why, so all we add is what happens next.
                file_unreadable = true;
                scribe::error(
                    Channel::Core,
                    "Constellations couldn't read the config file.  \
                    Running on the built-in defaults.",
                );
            }
        }
    }

    // A reload can't move the content folder out from under a running
    // server -- the log file, the accounts and the names list all came out
    // of the old one.  So the old one stays, and the new one is kept for
    // save() to write back, so the edit isn't lost at shutdown.
    let next_content_folder = settings.content_folder.clone();
    // (If the file couldn't be read, `settings` is only the defaults, and
    // there is nothing to warn about.)
    if let Some(running_folder) = running_folder {
        if running_folder != settings.content_folder && !file_unreadable {
            scribe::warn(Channel::Core,
                         &format!("CONTENT_FOLDER is {} in the file now, but it only changes at \
                         launch.  This run keeps {}.", settings.content_folder.display(),
                                  running_folder.display()));
        }
        settings.content_folder = running_folder;
    }

    // We only take the lock now, after all the logging is done, and let go of
    // it straight away.  Nobody ever waits on Constellations while it is
    // waiting on Scribe.
    // Rust note: a "poisoned" lock means some thread panicked while holding
    // it.  The settings are still perfectly good, so we take it anyway.
    let mut guard = CONSTELLATIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(Constellations {
        settings,
        file_unreadable,
        next_content_folder,
    });
}

/// Hands back a copy of the settings.  It is a copy so that nobody sits on
/// the lock, and nobody has to think about who owns what.  It is a dozen
/// small values, so the copy costs nothing worth measuring.
///
/// If this gets called before load() it returns the built-in defaults.  So
/// there is no such thing as calling it too early, the same as Scribe.
pub fn get() -> Settings {
    let guard = CONSTELLATIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Rust note: `as_ref()` lets us look inside the Option without taking the
    // contents out of the global.
    match guard.as_ref() {
        Some(constellations) => constellations.settings.clone(),
        None => default_settings(),
    }
}

/// Changes one setting on the running server.  `line` is what the admin
/// typed, `KEY=VALUE`, and it goes through the same checks as a line in the
/// file.  An `Err` says what was wrong with it, in words, and nothing changed.
///
/// The change is in memory only until save() writes it at shutdown.  Nothing
/// else is told about it: the Launcher hands Scribe its settings again, and
/// the addresses and ports get picked up the next time the server starts.
#[track_caller]
pub fn set(line: &str) -> Result<(), String> {
    let mut guard = CONSTELLATIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // If load() never ran there is nothing to change yet.  It always has by
    // the time the Launcher is up, so this is a second lock on the door.
    let constellations = match guard.as_mut() {
        Some(constellations) => constellations,
        None => return Err("The settings haven't been loaded yet.".to_string()),
    };

    let result = set_in(&mut constellations.settings, line);
    drop(guard);

    if result.is_ok() {
        scribe::info(Channel::Core, &format!("Setting changed: {}", line.trim()));
    }
    result
}

/// The part of set() that doesn't need the lock or Scribe, so the tests can
/// run it on a Settings of their own.
fn set_in(settings: &mut Settings, line: &str) -> Result<(), String> {
    let (key, value) = match line.split_once('=') {
        Some(halves) => halves,
        None => return Err("A setting is typed as KEY=VALUE, like TCP_PORT=9997.".to_string()),
    };
    let key = key.trim().to_ascii_uppercase();
    if key == "CONTENT_FOLDER" {
        return Err("CONTENT_FOLDER only changes at launch.  Change it in the config file \
        and restart.".to_string());
    }
    apply_setting(settings, &key, value.trim())
}

/// Where Scribe keeps its log files.  Inside the content folder.
pub fn log_folder() -> PathBuf {
    get().content_folder.join(LOG_SUBFOLDER)
}

/// Where the account files live, one per account.  Inside the content folder.
pub fn account_folder() -> PathBuf {
    get().content_folder.join(ACCOUNT_SUBFOLDER)
}

/// Where the SSL certificate and key live, inside the content folder.  The
/// folder is made the first time the server runs, and the files are made
/// the first time TLS is started.  The files are never written again, and the
/// folder is never removed, so the admin can put their own files in there
/// if they want to.  The server refuses to start if only one of the two is
/// there, so the admin can't accidentally lock out all the clients.
pub fn ssl_folder() -> PathBuf {
    get().content_folder.join(SSL_SUBFOLDER)
}


/// Where the player files live, inside the content folder.  One folder in
/// here per account, named after the username, and one file in that per
/// character.  The account folders get made by DiskMan the first time one
/// of their characters is saved, so only this one gets made at launch.
pub fn player_folder() -> PathBuf {
    get().content_folder.join(PLAYER_SUBFOLDER)
}

/// Makes every folder the content folder is supposed to have, if it isn't
/// there already.  main() calls this once, after load().
///
/// Nothing in here stops the server.  A folder that can't be made is an
/// Error in the log (DiskMan says why), and whoever needs it finds out for
/// themselves -- the account folder stops the launch in account::start().
pub fn make_folders() {
    for folder in [log_folder(), account_folder(), ssl_folder(), player_folder()] {
        // Rust note: `let _ =` throws the result away on purpose.  DiskMan
        // has already logged anything that went wrong.
        let _ = diskman::make_folder(&folder);
    }
}

/// Every setting as a `KEY=VALUE` line, the way it would be written to the
/// file, without the comments.  For the Launcher's "Show the settings".
pub fn settings_text() -> String {
    let mut text = String::new();
    for line in file_text(&get()).lines() {
        if !line.is_empty() && !line.starts_with('#') {
            text.push_str(line);
            text.push('\n');
        }
    }
    text
}

/// Writes the settings that are in memory back to the config file.  main()
/// calls this once, on the way out.
///
/// Only a shutdown that gets as far as main() saves.  A crash or a kill
/// doesn't, and that is fine -- the file is still whatever it was at launch.
///
/// Like load(), nothing in here can stop the server.  If the write fails we
/// say so, and the old file is still there untouched.
pub fn save() {
    let guard = CONSTELLATIONS.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let copy = guard.clone();

    // Rust note: `drop` gives the lock back right now, and not at the end of
    // the function.  Same reason as in load() -- we don't hold our lock while
    // we talk to Scribe or the disk.
    drop(guard);

    let constellations = match copy {
        Some(constellations) => constellations,
        None => {
            scribe::warn(
                Channel::Core,
                "Constellations were asked to save before it ever loaded.  \
                Nothing to save.",
            );
            return;
        }
    };

    let path = config_path();

    // The file gets the content folder it should start with next time,
    // which isn't always the one this run is using.
    let mut settings = constellations.settings;
    settings.content_folder = constellations.next_content_folder;

    if constellations.file_unreadable {
        scribe::warn(
            Channel::Core,
            &format!(
                "Constellations never managed to read {}, so it is not \
                going to write over it.  Settings not saved.",
                path.display()
            ),
        );
        return;
    }

    match write_config_file(&settings) {
        Ok(()) => {
            scribe::info(
                Channel::Core,
                &format!("Constellations saved {}", path.display()),
            );
        }
        Err(_) => {
            // DiskMan has already logged what went wrong.
            scribe::error(
                Channel::Core,
                "Constellations couldn't save the settings.  \
                If there was a config file, it hasn't been touched.",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Reading the file
// ---------------------------------------------------------------------------

/// Goes through the text of a config file one line at a time and puts every
/// good value into `settings`.  Anything it didn't like comes back as a list
/// of complaints, one per bad line, and the setting on that line is left
/// alone.
///
/// It doesn't touch the disk and it doesn't log.  That is on purpose -- it is
/// what lets `cargo test` run the parser without a config file or a Scribe.
///
/// The rules: blank lines and lines starting with # are skipped.  Everything
/// else is KEY=VALUE, split at the first `=`.  Spaces around the key and the
/// value are thrown away, and the key can be in any case.  If a key shows up
/// twice, the later one wins.
fn parse_text(text: &str, settings: &mut Settings) -> Vec<String> {
    let mut problems = Vec::new();
    let mut line_number = 0;

    for raw_line in text.lines() {
        line_number += 1;

        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Rust note: `split_once` cuts the line in two at the first `=` and
        // gives us both halves, or `None` if there was no `=` to cut at.
        let (key, value) = match line.split_once('=') {
            Some(halves) => halves,
            None => {
                problems.push(format!(
                    "line {}: \"{}\" isn't KEY=VALUE.  Ignored.",
                    line_number, line
                ));
                continue;
            }
        };

        let key = key.trim().to_ascii_uppercase();
        let value = value.trim();

        match apply_setting(settings, &key, value) {
            Ok(()) => {}
            Err(problem) => {
                problems.push(format!("line {}: {}  Ignored.", line_number, problem));
            }
        }
    }

    problems
}

/// Puts one value into the settings, if the key is one we know and the value
/// makes sense for it.  If not, the settings are left alone and the complaint
/// comes back as the error.  The complaint doesn't say what happens next,
/// because that depends on who asked: a line in the file is ignored, and a
/// change typed into the Launcher just doesn't happen.
// Rust note: the `?` on the end of each line means "if that failed, stop here
// and return its error to whoever called us".  So a bad value never gets as
// far as the assignment.
fn apply_setting(settings: &mut Settings, key: &str, value: &str) -> Result<(), String> {
    match key {
        "CONTENT_FOLDER" => settings.content_folder = parse_folder(key, value)?,
        "MAX_LOG_SIZE_MB" => settings.max_log_size_mb = parse_size_mb(key, value)?,
        "TCP_HOST_ADDRESS" => settings.tcp_host_address = parse_address(key, value)?,
        "TCP_PORT" => settings.tcp_port = parse_port(key, value)?,
        "UDP_HOST_ADDRESS" => settings.udp_host_address = parse_address(key, value)?,
        "UDP_PORT" => settings.udp_port = parse_port(key, value)?,
        "SHOW_DEBUG" => settings.show_debug = parse_bool(key, value)?,
        "COLOR_DEBUG" => settings.color_debug = parse_color(key, value)?,
        "COLOR_INFO" => settings.color_info = parse_color(key, value)?,
        "COLOR_WARN" => settings.color_warn = parse_color(key, value)?,
        "COLOR_ERROR" => settings.color_error = parse_color(key, value)?,
        _ => {
            return Err(format!("There is no setting called {}.", key));
        }
    }

    Ok(())
}

/// A whole number of megabytes, 1 or more.  Zero would have Scribe starting a
/// new file for every line.
fn parse_size_mb(key: &str, value: &str) -> Result<u64, String> {
    match value.parse::<u64>() {
        Ok(size) if size >= 1 => Ok(size),
        _ => Err(bad_value(key, value, "a whole number of megabytes, 1 or more")),
    }
}

/// A full path, starting from `/`.  A relative one would depend on which
/// folder the server happened to be started from.
fn parse_folder(key: &str, value: &str) -> Result<PathBuf, String> {
    if !Path::new(value).is_absolute() {
        return Err(bad_value(key, value, "a full path, like /opt/stratum/content"));
    }
    Ok(PathBuf::from(value))
}

/// An IP address, v4 or v6.  Not a host name -- we would have to go and look
/// a name up, and a server should know its own address.
fn parse_address(key: &str, value: &str) -> Result<IpAddr, String> {
    match value.parse::<IpAddr>() {
        Ok(address) => Ok(address),
        Err(_) => Err(bad_value(key, value, "an IP address like 10.0.0.84")),
    }
}

/// 1 to 65535.  Port 0 means "pick one for me", which is no use to a server
/// that people have to find.
fn parse_port(key: &str, value: &str) -> Result<u16, String> {
    match value.parse::<u16>() {
        Ok(port) if port >= 1 => Ok(port),
        _ => Err(bad_value(key, value, "a port number, 1 to 65535")),
    }
}

fn parse_bool(key: &str, value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(bad_value(key, value, "true or false")),
    }
}

fn parse_color(key: &str, value: &str) -> Result<Color, String> {
    match value.to_ascii_lowercase().as_str() {
        "gray" => Ok(Color::Gray),
        "white" => Ok(Color::White),
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "yellow" => Ok(Color::Yellow),
        "blue" => Ok(Color::Blue),
        "magenta" => Ok(Color::Magenta),
        "cyan" => Ok(Color::Cyan),
        _ => Err(bad_value(key, value, COLOR_LIST)),
    }
}

/// Every complaint about a value reads the same way, so it is built in one
/// place.
fn bad_value(key: &str, value: &str, wanted: &str) -> String {
    format!(
        "{} is \"{}\", and it needs to be {}.",
        key, value, wanted
    )
}

// ---------------------------------------------------------------------------
// Writing the file
// ---------------------------------------------------------------------------

/// The colors, the way they are spelled in the config file.
const COLOR_LIST: &str = "gray, white, red, green, yellow, blue, magenta or cyan";

fn color_name(color: Color) -> &'static str {
    match color {
        Color::Gray => "gray",
        Color::White => "white",
        Color::Red => "red",
        Color::Green => "green",
        Color::Yellow => "yellow",
        Color::Blue => "blue",
        Color::Magenta => "magenta",
        Color::Cyan => "cyan",
    }
}

/// The complete text of a config file holding these settings, comments and
/// all.  Whatever this writes, parse_text() has to be able to read back, and
/// there is a test that holds us to it.
fn file_text(settings:&Settings) -> String {
    let mut text = String::new();

    text.push_str("# Stratum general config file.\n");
    text.push_str("#\n");
    text.push_str("# Constellations writes this file -- once if there isn't one, \
    and again\n");
    text.push_str("# every time the server shuts down.  So change the values while \
    the server\n");
    text.push_str("# is stopped, or change them here and pick Reload in the \
    Launcher's config\n");
    text.push_str("# menu.  Either way, don't get attached to any comments you \
    add.  They won't\n");
    text.push_str("# survive the next shutdown.\n");
    text.push_str("#\n");
    text.push_str("# One setting per line, KEY=VALUE.  A line that starts with # is \
    a comment.\n");
    text.push_str("# A # after a value is not a comment -- it becomes part of the \
    value.\n");
    text.push_str("# A setting that is missing, or has a value we can't make sense \
    of, falls\n");
    text.push_str("# back to the built-in default, and the server log says so.\n");
    text.push_str("\n");

    text.push_str("# The folder the server keeps everything in: logs/, accounts/ and \
    saved/ssl/.\n");
    text.push_str("# Missing folders get made at launch.  This one only changes at \
    launch, and it\n");
    text.push_str("# doesn't move this file, which always lives in \
    /opt/stratum/content/config/.\n");
    text.push_str(&format!("CONTENT_FOLDER={}\n", settings.content_folder.display()));
    text.push_str("\n");

    text.push_str("# The biggest a log file gets before Scribe starts a new one, in \
    megabytes.\n");
    text.push_str(&format!("MAX_LOG_SIZE_MB={}\n", settings.max_log_size_mb));
    text.push_str("\n");

    text.push_str("# Where the TCP side (authentication) listens.  An IP address, \
    not a name.\n");
    text.push_str(&format!("TCP_HOST_ADDRESS={}\n", settings.tcp_host_address));
    text.push_str(&format!("TCP_PORT={}\n", settings.tcp_port));
    text.push_str("\n");

    text.push_str("# Where the UDP side (game traffic) listens.\n");
    text.push_str(&format!("UDP_HOST_ADDRESS={}\n", settings.udp_host_address));
    text.push_str(&format!("UDP_PORT={}\n", settings.udp_port));
    text.push_str("\n");

    text.push_str("# true or false.  When it is false, Debug messages are \
    dropped completely.\n");
    text.push_str("# They don't reach the terminal or the log file.\n");
    text.push_str(&format!("SHOW_DEBUG={}\n", settings.show_debug));
    text.push_str("\n");

    text.push_str("# The terminal color for each priority.  The choices are:\n");
    text.push_str(&format!("# {}.\n", COLOR_LIST));
    text.push_str(&format!("COLOR_DEBUG={}\n", color_name(settings.color_debug)));
    text.push_str(&format!("COLOR_INFO={}\n", color_name(settings.color_info)));
    text.push_str(&format!("COLOR_WARN={}\n", color_name(settings.color_warn)));
    text.push_str(&format!("COLOR_ERROR={}\n", color_name(settings.color_error)));
    text.push_str("\n");
    text
}

/// Hands the settings to DiskMan as a complete config file.  DiskMan makes
/// the folder if it has to, writes a temp file and renames it over the old
/// one, so a save that dies halfway leaves the old file alone.  If anything
/// fails, DiskMan logs it and we get the error back.
// Rust note: `#[track_caller]` passes our caller's location through to
// DiskMan's log lines, so they point at save() or write_default_file() and
// not at this little function.
#[track_caller]
fn write_config_file(settings: &Settings) -> io::Result<()> {
    diskman::write_file(&StratumFile {
        path: config_path(),
        contents: file_text(settings).into_bytes(),
    })
}

/// There was no config file, so we make one.  If that fails too we say so
/// and move on.  The server runs on the same defaults either way, it just
/// doesn't have a file to show for it.
fn write_default_file(settings: &Settings) {
    let path = config_path();

    match write_config_file(settings) {
        Ok(()) => {
            scribe::info(
                Channel::Core,
                &format!(
                    "No config file found, so Constellations wrote one with \
                    the defaults: {}",
                    path.display()
                ),
            );
        }
        Err(_) => {
            // DiskMan has already logged what went wrong.
            scribe::error(
                Channel::Core,
                "No config file found, and Constellations couldn't write one.  \
                Running on the built-in defaults.",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Rust note: Settings can't be printed (Color has no Debug), so these use
    // `assert!(a == b)` and not `assert_eq!`, which wants to print both sides
    // when it fails.

    #[test]
    fn what_we_write_we_can_read() {
        // Start from settings that are different from the defaults in every
        // field.  Otherwise a line that silently failed to parse would still
        // "match".
        let written = Settings {
            content_folder: PathBuf::from("/tmp/somewhere else/content"),
            max_log_size_mb: 7,
            tcp_host_address: "127.0.0.1".parse().unwrap(),
            tcp_port: 1111,
            udp_host_address: "::1".parse().unwrap(),
            udp_port: 2222,
            show_debug: false,
            color_debug: Color::Cyan,
            color_info: Color::Blue,
            color_warn: Color::Magenta,
            color_error: Color::Gray,
        };

        let mut read_back = default_settings();
        let problems = parse_text(&file_text(&written), &mut read_back);

        assert!(problems.is_empty());
        assert!(read_back == written);
    }

    #[test]
    fn jacobs_file_reads_clean() {
        let text = "CONTENT_FOLDER=/opt/stratum/content\n\
                    MAX_LOG_SIZE_MB=500\n\
                    TCP_HOST_ADDRESS=10.0.0.84\n\
                    TCP_PORT=9997\n\
                    UDP_HOST_ADDRESS=10.0.0.84\n\
                    UDP_PORT=9998\n";

        let mut settings = default_settings();
        let problems = parse_text(text, &mut settings);

        assert!(problems.is_empty());
        assert!(settings == default_settings());
    }

    #[test]
    fn spaces_case_comments_and_blank_lines() {
        let text = "# a comment\n\n   \n  tcp_port =  1234  \r\nColor_Warn = CYAN\n";

        let mut settings = default_settings();
        let problems = parse_text(text, &mut settings);

        assert!(problems.is_empty());
        assert_eq!(settings.tcp_port, 1234);
        assert!(settings.color_warn == Color::Cyan);
    }

    #[test]
    fn bad_values_keep_the_default() {
        let text = "COLOR_ERROR=purpel\n\
                    MAX_LOG_SIZE_MB=big\n\
                    MAX_LOG_SIZE_MB=0\n\
                    TCP_PORT=99999\n\
                    UDP_PORT=0\n\
                    TCP_HOST_ADDRESS=localhost\n\
                    SHOW_DEBUG=maybe\n\
                    CONTENT_FOLDER=\n\
                    CONTENT_FOLDER=stratum/content\n";

        let mut settings = default_settings();
        let problems = parse_text(text, &mut settings);

        assert_eq!(problems.len(), 9);
        assert!(settings == default_settings());
    }

    #[test]
    fn a_bad_line_does_not_spoil_the_good_ones() {
        let text = "TCP_PORT=4000\nthis is not a setting\nWIBBLE=3\nUDP_PORT=4001\n";

        let mut settings = default_settings();
        let problems = parse_text(text, &mut settings);

        assert_eq!(problems.len(), 2);
        assert!(problems[0].starts_with("line 2:"));
        assert!(problems[1].starts_with("line 3:"));
        assert_eq!(settings.tcp_port, 4000);
        assert_eq!(settings.udp_port, 4001);
    }

    #[test]
    fn the_later_one_wins() {
        let mut settings = default_settings();
        let problems = parse_text("TCP_PORT=1\nTCP_PORT=2\n", &mut settings);

        assert!(problems.is_empty());
        assert_eq!(settings.tcp_port, 2);
    }

    #[test]
    fn a_value_can_have_an_equals_sign_in_it() {
        let mut settings = default_settings();
        let problems = parse_text("CONTENT_FOLDER=/tmp/a=b/content\n", 
                                  &mut settings);

        assert!(problems.is_empty());
        assert_eq!(settings.content_folder, PathBuf::from("/tmp/a=b/content"));
    }

    #[test]
    fn an_old_log_folder_line_is_ignored() {
        // Every config file written before CONTENT_FOLDER has one of these.
        let mut settings = default_settings();
        let problems = parse_text("LOG_FOLDER=/opt/stratum/content/logs/\n", 
                                  &mut settings);

        assert_eq!(problems.len(), 1);
        assert!(settings == default_settings());
    }

    #[test]
    fn a_change_typed_into_the_launcher() {
        let mut settings = default_settings();

        assert_eq!(set_in(&mut settings, "tcp_port = 1234"), Ok(()));
        assert_eq!(settings.tcp_port, 1234);

        // A bad value, an unknown key and no `=` all leave things alone.
        assert!(set_in(&mut settings, "TCP_PORT=99999").is_err());
        assert!(set_in(&mut settings, "WIBBLE=3").is_err());
        assert!(set_in(&mut settings, "TCP_PORT 4000").is_err());
        assert_eq!(settings.tcp_port, 1234);

        // The content folder can't be changed from the Launcher at all, not
        // even to a good value.
        assert!(set_in(&mut settings, "content_folder=/tmp/elsewhere").is_err());

        let mut expected = default_settings();
        expected.tcp_port = 1234;
        assert!(settings == expected);
    }
}