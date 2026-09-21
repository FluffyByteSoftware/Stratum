//! File:     src/scribe.rs
//! Project:  Stratum Core
//! Author:   Jacob Chacko
//!
//! Scribe is the logger.  Anything in the server can call it, and every
//! message goes two places: the terminal (in color) and a log file (plain).
//! The log file appends, and it rolls over to a new file at midnight UTC or
//! when it hits the size limit -- whichever comes first.
//!
//! All time in here is UTC, and every timestamp ends in Z to say so.
//!
//! Every line also ends with the file and line number it was logged from, so
//! we can walk straight from a message to the code that said it.
//!
//! No crates.  Standard library only, and that includes the calendar math,
//! which is the slow and obvious version on purpose.
//!
//! A call looks like this:
//!
//!     scribe::info(Channel::Core, "Listening on 7777");
//!
//! Scribe has to be up before anything else, and that includes the config
//! file.  So it starts on built-in defaults, and main() hands it the real
//! settings through `scribe::initialize()` once Constellations has loaded.
//! Scribe doesn't know Constellations exists.  main() is the only one who
//! knows both.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::panic::Location;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Priorities, channels and colors
// ---------------------------------------------------------------------------

/// How much a message matters.  Debug is the only one that can be hidden.
// Rust note: `#[derive(...)]` asks the compiler to write the boring code for
// us.  `Clone, Copy` means a Priority gets copied around like an int in C,
// and `PartialEq` is what lets us compare two of them with `==`.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq)]
pub enum Priority {
    Debug,
    Info,
    Warn,
    Error,
}

/// Which part of the server a message came from.  These six are the starting
/// set.  We will probably add more later.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq)]
pub enum Channel {
    World,
    Core,
    Tools,
    NetTcp,
    NetUdp,
    Security,
}

/// The terminal colors a priority can be given.  Which priority gets which
/// is set in the config file.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq)]
pub enum Color {
    Gray,
    White,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
}

/// Puts the terminal back to its normal color.
const ANSI_RESET: &str = "\x1b[0m";

/// The escape code that switches the terminal to a color.  Plain ANSI, which
/// every Linux terminal understands.
// Rust note: `&'static str` is a pointer to a string literal baked into the
// executable -- a `const char *` to a literal in C.  `'static` means it lives
// for the whole run and nobody ever frees it.
fn ansi_code(color: Color) -> &'static str {
    match color {
        Color::Gray => "\x1b[90m",
        Color::White => "\x1b[37m",
        Color::Red => "\x1b[31m",
        Color::Green => "\x1b[32m",
        Color::Yellow => "\x1b[33m",
        Color::Blue => "\x1b[34m",
        Color::Magenta => "\x1b[35m",
        Color::Cyan => "\x1b[36m",
    }
}

fn priority_tag(priority: Priority) -> &'static str {
    match priority {
        Priority::Debug => "DEBUG",
        Priority::Info => "INFO",
        Priority::Warn => "WARN",
        Priority::Error => "ERROR",
    }
}

fn channel_tag(channel: Channel) -> &'static str {
    match channel {
        Channel::World => "World",
        Channel::Core => "Core",
        Channel::Tools => "Tools",
        Channel::NetTcp => "TCP",
        Channel::NetUdp => "UDP",
        Channel::Security => "Security",
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Everything about Scribe that is meant to be adjustable.  It is public so
/// that main() can fill one in and pass it to initialize().
pub struct ScribeConfig {
    /// When this is false, Debug messages are dropped completely.  They don't
    /// reach the terminal or the file.
    pub show_debug: bool,
    pub color_debug: Color,
    pub color_info: Color,
    pub color_warn: Color,
    pub color_error: Color,
    /// The folder the log files go in.
    pub log_dir: PathBuf,
    /// Once a log file would grow past this many bytes we start a new one.
    pub max_file_bytes: u64,
}

/// What Scribe runs on until initialize() hands it the real settings.  In a
/// normal launch that is only the first few lines.
fn default_config() -> ScribeConfig {
    ScribeConfig {
        show_debug: true,
        color_debug: Color::Green,
        color_info: Color::White,
        color_warn: Color::Yellow,
        color_error: Color::Red,
        log_dir: PathBuf::from("/opt/stratum/content/logs"),
        max_file_bytes: 10 * 1024 * 1024,
    }
}

// ---------------------------------------------------------------------------
// The one global Scribe
// ---------------------------------------------------------------------------

/// Scribe's working state.  There is exactly one of these, in SCRIBE below.
struct Scribe {
    config: ScribeConfig,
    /// The log file we are appending to.  `None` until the first message, and
    /// `None` for good if we gave up on the file.
    file: Option<File>,
    /// The day the open file belongs to, as days since 1970-01-01.  When
    /// today's number is different, it is time for a new file.
    file_day: u64,
    /// 0 for the first file of the day, then 1, 2, 3... each time the size
    /// limit forces another one.
    file_counter: u32,
    /// How big the open file is.  We keep count ourselves so we don't have to
    /// ask the file system on every line.
    file_bytes: u64,
    /// Set when the log file could not be opened or written.  After that we
    /// only print to the terminal.
    file_gave_up: bool,
}

// Rust note: there are no static classes, so this is a file-scope global, the
// same as in C.  The difference is the Mutex: the compiler will not let us
// touch what is inside without locking it first.  `Option<Scribe>` is either
// `None` or `Some(scribe)` -- think of a pointer that may be NULL, except we
// are forced to check.
static SCRIBE: Mutex<Option<Scribe>> = Mutex::new(None);

fn new_scribe() -> Scribe {
    Scribe {
        config: default_config(),
        file: None,
        file_day: 0,
        file_counter: 0,
        file_bytes: 0,
        file_gave_up: false,
    }
}

// ---------------------------------------------------------------------------
// What the rest of the server calls
// ---------------------------------------------------------------------------

/// Brings Scribe up.  main() calls this before anything else, so that
/// everything after it has somewhere to complain.
///
/// Scribe would start by itself on the first message anyway.  Calling this
/// first means a log folder we can't write to gets reported at launch and not
/// twenty minutes in.
#[track_caller]
pub fn start() {
    log(Priority::Info, Channel::Core, "Scribe is up.");
}

/// Swaps the built-in defaults for the real settings.  main() calls this once
/// Constellations has loaded.  It can be called again later if the settings
/// ever change while the server is running.
///
/// Colors, Debug visibility and the size limit take effect on the very next
/// message.  If the log folder changed, we let go of the file we had open,
/// and the next message opens today's file in the new folder.  A new folder
/// also gets a fresh try if we had given up on the old one.
///
/// The one thing I don't like about this: everything logged before this call
/// (normally "Scribe is up." and whatever Constellations had to say) has
/// already gone to the default folder.  So if the config file points the logs
/// somewhere else, every launch leaves a few lines behind in the old place.
/// The fix would be to hold the early lines in memory until we know where
/// they belong.  Not worth it while both folders are the same one.
#[track_caller]
pub fn initialize(config: ScribeConfig) {
    let mut guard = SCRIBE.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scribe = guard.get_or_insert_with(new_scribe);

    if config.log_dir != scribe.config.log_dir {
        // Rust note: there is no close call.  Putting `None` here throws the
        // old File away, and Rust closes a file when it is thrown away.
        scribe.file = None;
        scribe.file_gave_up = false;
    }

    scribe.config = config;

    // We have to give the lock back before we log, because log() takes the
    // same lock and asking for it twice would hang forever.
    drop(guard);

    log(Priority::Info, Channel::Core, "Scribe has its settings from the config file.");
}

#[allow(dead_code)]
#[track_caller]
pub fn debug(channel: Channel, message: &str) {
    log(Priority::Debug, channel, message);
}

#[allow(dead_code)]
#[track_caller]
pub fn info(channel: Channel, message: &str) {
    log(Priority::Info, channel, message);
}

#[track_caller]
pub fn warn(channel: Channel, message: &str) {
    log(Priority::Warn, channel, message);
}

#[track_caller]
pub fn error(channel: Channel, message: &str) {
    log(Priority::Error, channel, message);
}

/// Every message ends up here.  The four functions above are shorthand.
///
/// The lock is held for the whole call.  That is what keeps two threads from
/// writing over the top of each other's lines.
// Rust note: `#[track_caller]` is how we find out who called us.  With it on,
// `Location::caller()` gives the file and line of the call, not of this
// function.  It passes up through every function that also has the marker,
// which is why debug(), info(), warn(), error() and start() all carry it --
// without that, every message would claim it came from scribe.rs.
//
// There is no way to get the name of the calling function.  Rust doesn't
// have one short of macros, and file plus line already pins the exact spot.
#[track_caller]
pub fn log(priority: Priority, channel: Channel, message: &str) {
    let caller = Location::caller();

    // Rust note: if some thread panicked while it held the lock, Rust marks
    // the lock "poisoned" and `lock()` hands back an error.  A logger has to
    // keep working through exactly that kind of day, so we take the lock
    // anyway.
    let mut guard = SCRIBE.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Rust note: if the Option is still `None` this fills it in by calling
    // new_scribe().  Either way we get back a pointer to the Scribe inside.
    let scribe = guard.get_or_insert_with(new_scribe);

    if priority == Priority::Debug && !scribe.config.show_debug {
        return;
    }

    let now = utc_now();
    let line = format_line(&now, priority, channel, message, caller.file(), caller.line());

    write_to_terminal(&scribe.config, priority, &line);
    write_to_file(scribe, &now, &line);

    // Rust note: no unlock call.  The lock lets go when `guard` goes out of
    // scope, which is right here.
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// Builds one line of log.  It comes out like this:
///
///     2026-09-21T20:14:07.417Z [INFO ] [Core    ] Listening on 7777  (src/main.rs:31)
///
/// The tags are padded so the messages line up in a column.  The file and
/// line go last because they are a different length every time, and anywhere
/// else they would knock the messages out of line.
fn format_line(
    now: &UtcTime,
    priority: Priority,
    channel: Channel,
    message: &str,
    caller_file: &str,
    caller_line: u32,
) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z [{:<5}] [{:<8}] {}  ({}:{})",
        now.year,
        now.month,
        now.day,
        now.hour,
        now.minute,
        now.second,
        now.millis,
        priority_tag(priority),
        channel_tag(channel),
        message,
        caller_file,
        caller_line
    )
}

fn write_to_terminal(config: &ScribeConfig, priority: Priority, line: &str) {
    let color = match priority {
        Priority::Debug => config.color_debug,
        Priority::Info => config.color_info,
        Priority::Warn => config.color_warn,
        Priority::Error => config.color_error,
    };

    // Rust note: `println!` panics if the terminal has gone away, and a
    // logger should never be the thing that kills the server.  `writeln!`
    // returns the error instead, and `let _ =` says we are ignoring it on
    // purpose.
    let mut terminal = io::stdout();
    let _ = writeln!(terminal, "{}{}{}", ansi_code(color), line, ANSI_RESET);
}

fn write_to_file(scribe: &mut Scribe, now: &UtcTime, line: &str) {
    if scribe.file_gave_up {
        return;
    }

    // The +1 is the newline.
    let line_bytes = line.len() as u64 + 1;

    // First message of the run, or the date changed since the last one.
    // Either way we want today's file.  If the server was restarted partway
    // through the day we pick up the newest of today's files and keep
    // appending to it.
    if scribe.file.is_none() || scribe.file_day != now.days {
        scribe.file_day = now.days;
        scribe.file_counter = find_latest_counter(&scribe.config, now);
        open_log_file(scribe, now);
        if scribe.file_gave_up {
            return;
        }
    }

    // Size limit.  The `> 0` check is there so one giant line can't send us
    // into making a fresh file for every message.
    if scribe.file_bytes > 0 && scribe.file_bytes + line_bytes > scribe.config.max_file_bytes {
        scribe.file_counter += 1;
        open_log_file(scribe, now);
        if scribe.file_gave_up {
            return;
        }
    }

    let file = match scribe.file.as_mut() {
        Some(file) => file,
        None => return,
    };

    let result = writeln!(file, "{}", line);
    match result {
        Ok(()) => scribe.file_bytes += line_bytes,
        Err(problem) => {
            let reason = format!("Can't write to the log file: {}.  \
            Terminal only from here on.", problem);
            give_up_on_file(scribe, &reason);
        }
    }
}

/// The path of one log file.  The first file of a day is `2026.09.21.log`.
/// If the size limit forces more on the same day they are `2026.09.21.1.log`,
/// `2026.09.21.2.log` and so on.  Year first, so a folder listing sorts by
/// date.  We never rename a file once it exists.
fn log_file_path(config: &ScribeConfig, now: &UtcTime, counter: u32) -> PathBuf {
    let name = if counter == 0 {
        format!("{:04}.{:02}.{:02}.log", now.year, now.month, now.day)
    } else {
        format!("{:04}.{:02}.{:02}.{}.log", now.year, now.month, now.day, counter)
    };
    config.log_dir.join(name)
}

/// Finds the highest numbered log file that already exists for today, so a
/// restart carries on where the last run stopped.
fn find_latest_counter(config: &ScribeConfig, now: &UtcTime) -> u32 {
    let mut counter = 0;
    while log_file_path(config, now, counter + 1).exists() {
        counter += 1;
    }
    counter
}

/// Opens (or creates) the log file that `file_day` and `file_counter` point
/// at, making the log folder first if it isn't there.
fn open_log_file(scribe: &mut Scribe, now: &UtcTime) {
    match fs::create_dir_all(&scribe.config.log_dir) {
        Ok(()) => {}
        Err(problem) => {
            let reason = format!(
                "Can't create the log folder {}: {}.  Terminal only from here on.",
                scribe.config.log_dir.display(),
                problem
            );
            give_up_on_file(scribe, &reason);
            return;
        }
    }

    let path = log_file_path(&scribe.config, now, scribe.file_counter);
    let opened = OpenOptions::new().create(true).append(true).open(&path);

    match opened {
        Ok(file) => {
            // An existing file already has something in it, and that counts
            // against the size limit.
            scribe.file_bytes = match file.metadata() {
                Ok(metadata) => metadata.len(),
                Err(_) => 0,
            };
            scribe.file = Some(file);
        }
        Err(problem) => {
            let reason = format!(
                "Can't open the log file {}: {}.  Terminal only from here on.",
                path.display(),
                problem
            );
            give_up_on_file(scribe, &reason);
        }
    }
}

/// Something went wrong with the log file.  We say so once, in the Error
/// color, and from then on Scribe only prints to the terminal.
///
/// This is probably too blunt -- a full disk that gets cleaned up an hour
/// later still leaves us without a log file until the next restart.  Good
/// enough for now.
fn give_up_on_file(scribe: &mut Scribe, reason: &str) {
    scribe.file = None;
    scribe.file_gave_up = true;

    // We can't go through log() for this.  We are already inside it, holding
    // the lock, and asking for the same lock twice would hang forever.
    let now = utc_now();
    // This complaint is Scribe's own, so the file and line are ours.
    let line = format_line(&now, Priority::Error, Channel::Core, reason, file!(), 
                           line!());
    write_to_terminal(&scribe.config, Priority::Error, &line);
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

/// A moment in UTC, broken into the pieces we print.
struct UtcTime {
    /// Whole days since 1970-01-01.  This is what we compare to tell that the
    /// date has changed.
    days: u64,
    year: u64,
    month: u64,
    day: u64,
    hour: u64,
    minute: u64,
    second: u64,
    millis: u64,
}

fn utc_now() -> UtcTime {
    // The only way this fails is a system clock set to before 1970.  If
    // somebody manages that, they get logs dated 1970.
    let since_1970 = SystemTime::now()
        .duration_since(UNIX_EPOCH).unwrap_or_else(|_| Duration::ZERO);
    utc_from_seconds(since_1970.as_secs(), since_1970.subsec_millis() as u64)
}

/// Turns "seconds since 1970" into a calendar date and a time of day.
///
/// The standard library stops at the seconds, so the calendar is ours to
/// work out.  There is a well known fast formula for this and it is a wall
/// of magic numbers.  This is the other way: walk forward from 1970 taking
/// off a year at a time, then a month at a time, until what is left is the
/// day of the month.  That is about 70 subtractions per message, which is
/// nothing next to the cost of the write that follows it.
fn utc_from_seconds(total_seconds: u64, millis: u64) -> UtcTime {
    let days = total_seconds / 86400;
    let seconds_today = total_seconds % 86400;

    let mut remaining = days;

    let mut year = 1970;
    loop {
        let year_length = if is_leap_year(year) { 366 } else { 365 };
        if remaining < year_length {
            break;
        }
        remaining -= year_length;
        year += 1;
    }

    let mut month = 1;
    loop {
        let month_length = days_in_month(year, month);
        if remaining < month_length {
            break;
        }
        remaining -= month_length;
        month += 1;
    }

    UtcTime {
        days,
        year,
        month,
        // `remaining` counts from 0 and the calendar counts from 1.
        day: remaining + 1,
        hour: seconds_today / 3600,
        minute: (seconds_today % 3600) / 60,
        second: seconds_today % 60,
        millis,
    }
}

/// Every fourth year, except the centuries, except every fourth century.
fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: u64, month: u64) -> u64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Rust note: `cargo test` builds and runs everything in here, and a normal
// `cargo build` leaves it out completely.  Hand-rolled calendar math is
// exactly the kind of thing that is wrong by one day and nobody notices until
// February, so it gets checked against dates we know.
#[cfg(test)]
mod tests {
    use super::*;

    fn date_of(total_seconds: u64) -> (u64, u64, u64) {
        let time = utc_from_seconds(total_seconds, 0);
        (time.year, time.month, time.day)
    }

    #[test]
    fn day_zero_is_new_years_1970() {
        assert_eq!(date_of(0), (1970, 1, 1));
    }

    #[test]
    fn last_second_of_a_day_is_still_that_day() {
        assert_eq!(date_of(86399), (1970, 1, 1));
        assert_eq!(date_of(86400), (1970, 1, 2));
    }

    #[test]
    fn leap_days_land_in_february() {
        // 2000 is the odd one: a century year that is still a leap year.
        assert_eq!(date_of(951782400), (2000, 2, 29));
        assert_eq!(date_of(1709164800), (2024, 2, 29));
        assert_eq!(date_of(1709251200), (2024, 3, 1));
    }

    #[test]
    fn the_day_we_wrote_this() {
        let time = utc_from_seconds(1790021647, 417);
        assert_eq!((time.year, time.month, time.day), (2026, 9, 21));
        assert_eq!((time.hour, time.minute, time.second), (20, 14, 7));
    }

    #[test]
    fn end_of_a_year() {
        assert_eq!(date_of(1798761599), (2026, 12, 31));
        assert_eq!(date_of(1798761600), (2027, 1, 1));
    }
}