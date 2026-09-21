//! File:     src/constellations.rs
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
//! eleven lines.
//!
//! A bad value never stops the server.  We complain through Scribe, keep the
//! default for that one setting, and carry on.
//!
//! main() calls `constellations::load()` right after Scribe is up, and
//! `constellations::save()` on the way out.  Reading a setting looks like this:
//!
//!     let port = constellations::get().tcp_port;
//!
//! The file gets written whole every time we save.  So a comment somebody
//! added by hand is gone after the next shutdown, and so is any line we
//! complained about at launch.  The values survive.  The decoration doesn't.
//!
//! Adding a setting means touching four places, all of them in this file: the
//! Settings struct, default_settings(), apply_setting() and file_text().

use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Mutex;

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

/// What the file is called while we are still writing it.  It has to be in
/// the same folder as the real one, because a rename only works within one
/// disk.
const TEMP_FILE_NAME: &str = "stratum.conf.tmp";

fn config_path() -> PathBuf {
    PathBuf::from(CONFIG_FOLDER).join(CONFIG_FILE_NAME)
}

// ---------------------------------------------------------------------------
// The settings
// ---------------------------------------------------------------------------

/// Every global setting the server has.  The names match the keys in the
/// config file, just in lowercase.
// Most of these have no customer yet, and the compiler complains about fields
// nobody reads, so we tell it to be quiet.
// Rust note: `Clone` is what lets get() hand out a copy of the whole struct.
// `PartialEq` lets the tests compare two of them with `==`.
#[allow(dead_code)]
#[derive(Clone, PartialEq)]
pub struct Settings {
    /// The biggest a log file gets before Scribe starts a new one, in MB.
    pub max_log_size_mb: u64,
    /// The folder Scribe keeps its log files in.
    pub log_folder: PathBuf,
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
        max_log_size_mb: 500,
        log_folder: PathBuf::from("/opt/stratum/content/logs/"),
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
/// Calling it a second time reads the file again and replaces the settings.
/// Nothing does that yet.
pub fn load() {
    let path = config_path();
    let mut settings = default_settings();
    let mut file_unreadable = false;

    match fs::read_to_string(&path) {
        Ok(text) => {
            let problems = parse_text(&text, &mut settings);
            for problem in &problems {
                scribe::warn(Channel::Core, &format!("{}, {}", path.display(), problem));
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
                // Somebody's settings are in there.
                file_unreadable = true;
                scribe::error(
                    Channel::Core,
                    &format!(
                        "Constellations can't read {} ({}).  Running on the built-in defaults.",
                        path.display(),
                        error
                    ),
                );
            }
        }
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
    });
}

/// Hands back a copy of the settings.  It is a copy so that nobody sits on
/// the lock, and nobody has to think about who owns what.  It is eleven small
/// values, so the copy costs nothing worth measuring.
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

    if constellations.file_unreadable {
        scribe::warn(
            Channel::Core,
            &format!(
                "Constellations never managed to read {}, so it is not going to write over it.  \
                Settings not saved.",
                path.display()
            ),
        );
        return;
    }

    match write_file(&path, &file_text(&constellations.settings)) {
        Ok(()) => {
            scribe::info(
                Channel::Core,
                &format!("Constellations saved {}", path.display()),
            );
        }
        Err(error) => {
            scribe::error(
                Channel::Core,
                &format!(
                    "Constellations can't save {} ({}).  The old file is still there.",
                    path.display(),
                    error
                ),
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
                problems.push(format!("line {}: {}", line_number, problem));
            }
        }
    }

    problems
}

/// Puts one value into the settings, if the key is one we know and the value
/// makes sense for it.  If not, the settings are left alone and the complaint
/// comes back as the error.
// Rust note: the `?` on the end of each line means "if that failed, stop here
// and return its error to whoever called us".  So a bad value never gets as
// far as the assignment.
fn apply_setting(settings: &mut Settings, key: &str, value: &str) -> Result<(), String> {
    match key {
        "MAX_LOG_SIZE_MB" => settings.max_log_size_mb = parse_size_mb(key, value)?,
        "LOG_FOLDER" => settings.log_folder = parse_folder(key, value)?,
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
            return Err(format!("there is no setting called {}.  Ignored.", key));
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

fn parse_folder(key: &str, value: &str) -> Result<PathBuf, String> {
    if value.is_empty() {
        return Err(bad_value(key, value, "a folder"));
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
        "{} is \"{}\", and it needs to be {}.  Keeping the default.",
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
fn file_text(settings: &Settings) -> String {
    let mut text = String::new();

    text.push_str("# Stratum general config file.\n");
    text.push_str("#\n");
    text.push_str("# Constellations writes this file -- once if there isn't one, \
    and again\n");
    text.push_str("# every time the server shuts down.  So change the values while \
    the server\n");
    text.push_str("# is stopped, and don't get attached to any comments you add.  \
    They won't\n");
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

    text.push_str("# The biggest a log file gets before Scribe starts a new one, in \
    megabytes.\n");
    text.push_str(&format!("MAX_LOG_SIZE_MB={}\n", settings.max_log_size_mb));
    text.push_str("\n");

    text.push_str("# The folder Scribe keeps its log files in.\n");
    text.push_str(&format!("LOG_FOLDER={}\n", settings.log_folder.display()));
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

    text
}

/// Makes the config folder if it isn't there, then writes the file.  Any
/// step failing comes back as the same kind of error.
///
/// We write to a temp file first and then rename it over the old one.  If the
/// server dies halfway through a save, and we had been writing straight into
/// the real file, then the next launch finds half a config.  This way the old
/// file isn't touched until the new one is complete.
fn write_file(path: &PathBuf, text: &str) -> io::Result<()> {
    let temp_path = PathBuf::from(CONFIG_FOLDER).join(TEMP_FILE_NAME);

    fs::create_dir_all(CONFIG_FOLDER)?;
    fs::write(&temp_path, text)?;
    fs::rename(&temp_path, path)?;
    Ok(())
}

/// There was no config file, so we make one.  If that fails too we say so
/// and move on.  The server runs on the same defaults either way, it just
/// doesn't have a file to show for it.
fn write_default_file(settings: &Settings) {
    let path = config_path();

    match write_file(&path, &file_text(settings)) {
        Ok(()) => {
            scribe::info(
                Channel::Core,
                &format!(
                    "No config file found, so Constellations wrote one with the defaults: {}",
                    path.display()
                ),
            );
        }
        Err(error) => {
            scribe::error(
                Channel::Core,
                &format!(
                    "No config file found, and Constellations can't write {} ({}).  \
                    Running on the built-in defaults.",
                    path.display(),
                    error
                ),
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
            max_log_size_mb: 7,
            log_folder: PathBuf::from("/tmp/somewhere else/logs"),
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
        let text = "MAX_LOG_SIZE_MB=500\n\
                    LOG_FOLDER=/opt/stratum/content/logs/\n\
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
                    LOG_FOLDER=\n";

        let mut settings = default_settings();
        let problems = parse_text(text, &mut settings);

        assert_eq!(problems.len(), 8);
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
        let problems = parse_text("LOG_FOLDER=/tmp/a=b/logs\n", &mut settings);

        assert!(problems.is_empty());
        assert_eq!(settings.log_folder, PathBuf::from("/tmp/a=b/logs"));
    }
}