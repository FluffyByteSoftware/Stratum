//! File:     stratum-launcher/src/launcher.rs
//! Project:  Stratum Core
//! Author:   Jacob Chacko
//!
//! The Launcher, the admin's menu.  main() gets everything started, then
//! hands the terminal over to `launcher::run()`, and when that returns, the
//! server shuts down properly.  So Q is how the server gets stopped.
//!
//! The log lives in a file, and the menu can show the end of it (L).  Until
//! the server is started, Scribe's warnings and errors still print here,
//! since the admin is the one who should see them.  Once it is running the
//! terminal goes quiet, so fifty connection threads can't write over the
//! menu, and the log file is the only place anything goes.
//!
//! The main menu is the C# version's: an enum for the server state, and a
//! match on the letter and the state.
//!
//! Passwords are typed with the `rpassword` crate, so they never show on
//! the screen.  The standard library can't turn the terminal's echo off.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use stratum_tools::account::{self, Account};
use stratum_tools::constellations;
use stratum_tools::scribe::{self, Channel};
use stratum_tools::security;
use stratum_game::character;
use stratum_game::player_file;
use stratum_game::game_loop;
use stratum_networking::CharacterSummary;

/// Where the server is in its life.  It decides what S says and whether Q is
/// on the menu.
#[derive(Clone, Copy, PartialEq, Debug)]
enum ServerState {
    NeverStarted,
    Running,
    Stopped,
}

// ---------------------------------------------------------------------------
// The main menu
// ---------------------------------------------------------------------------

/// How much of the log L shows.
const VIEW_LOG_LINES: usize = 50;

/// How far back View log reads at a time.  50 log lines are about 6 KB, so
/// one chunk usually does it.
const VIEW_LOG_CHUNK_BYTES: u64 = 16 * 1024;

/// Runs the menu until the admin picks Q, or the terminal closes (Ctrl-D).
/// A closed terminal counts as Q, and stops the server first if it is
/// running, because nobody is left to pick Stop.
pub fn run() {
    say(&format!("The log is {}.", log_path().display()));

    let mut state = ServerState::NeverStarted;

    loop {
        show_main_menu(state);

        let line = match read_line("> ") {
            Some(line) => line,
            None => {
                if state == ServerState::Running {
                    stop_server();
                }
                break;
            }
        };

        match (choice_of(&line), state) {
            (Some('S'), ServerState::Running) => {
                stop_server();
                state = ServerState::Stopped;
            }
            (Some('S'), _) => {
                if  start_server() {
                    state = ServerState::Running;
                }
            }

            (Some('L'), _) => view_log(),
            (Some('W'), _) => account_menu(),
            (Some('C'), _) => config_menu(state == ServerState::Running),
            (Some('Q'), ServerState::Running) => {
                say("Stop the server first.  Q only works while it isn't running.");
            }
            (Some('Q'), _) => break,
            _ => say("That isn't one of the choices."),
        }
    }
}

fn show_main_menu(state: ServerState) {
    say("");
    say(&format!("Stratum Core -- the server is {}.", state_text(state)));
    say("");
    say(&format!("  S) {}", start_label(state)));
    say("  L) View the log");
    say("  W) Account management");
    say("  C) Config management");
    if state != ServerState::Running {
        say("  Q) Shut down");
    }
    say("");
}

/// What S says, which depends on where the server is.
fn start_label(state: ServerState) -> &'static str {
    match state {
        ServerState::NeverStarted => "Start server",
        ServerState::Running => "Stop server",
        ServerState::Stopped => "Restart server",
    }
}

fn state_text(state: ServerState) -> &'static str {
    match state {
        ServerState::NeverStarted => "not started",
        ServerState::Running => "running",
        ServerState::Stopped => "stopped",
    }
}

/// Starts the game loop, then networking.  True when both are running.  A
/// refusal comes with its reason in words, and the admin sees it in the
/// menu.
///
/// The game loop goes first, so the world is ticking before anybody can get
/// in.  If networking won't start, the loop stops again, so the server is
/// either all the way up or not up at all.
///
/// The terminal goes quiet only once the start has worked, so anything
/// networking has to warn about on the way up still shows here.
fn start_server() -> bool {
    let (to_game, from_net) = std::sync::mpsc::channel();

    if let Err(reason) = game_loop::start(from_net) {
        let text = format!("The server didn't start.  {}", reason);
        scribe::error(Channel::Core, &text);
        return false;
    }

    // The game's character functions, handed to networking, which can't
    // see the game itself.
    let calls = stratum_networking::CharacterCalls {
        create: character::create_character,
        delete: character::delete_character,
        check: character::check_character,
        list: list_characters,
    };
    match stratum_networking::start(calls, to_game) {
        Ok(()) => {
            scribe::info(Channel::Core, "Server started.");
            say("Server started.");
            scribe::quiet_terminal(true);
            true
        }
        Err(reason) => {
            game_loop::stop();
            let text = format!("The server didn't start.  {}", reason);
            scribe::error(Channel::Core, &text);
            false
        }
    }
}

/// What the character list shows for each character on an account, for
/// networking, which can't read a player file itself.  A character whose
/// file is missing or damaged still goes in, marked as not playable, so
/// the player can see it and delete it.  load() has already logged a
/// damaged one.
fn list_characters(account: &Account) -> Vec<CharacterSummary> {
    let mut list = Vec::new();
    for character in &account.characters {
        let summary = match player_file::load(&account.username, &character.name, &character.uuid) {
            Ok(Some(file)) => CharacterSummary {
                shortname: character.name.clone(),
                longname: file.longname,
                playable: true,
                x: file.position.x,
                y: file.position.y,
                z: file.position.z,
            },
            _ => CharacterSummary {
                shortname: character.name.clone(),
                longname: account::display_name(&character.name),
                playable: false,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        };
        list.push(summary);
    }
    list
}

/// Stops networking, then the game loop.  Networking goes first, so nothing
/// new can come in while the world winds down.  The terminal comes back
/// before either, so a warning from the stop lands in front of the admin
/// who asked for it.
fn stop_server() {
    scribe::quiet_terminal(false);
    stratum_networking::stop();
    game_loop::stop();
    scribe::info(Channel::Core, "Server stopped.");
    say("Server stopped.");
}

// ---------------------------------------------------------------------------
// L) View the log
// ---------------------------------------------------------------------------

/// The `latest.log` link in the log folder, which Scribe keeps pointed at
/// the file it is writing.
fn log_path() -> PathBuf {
    constellations::log_folder().join(scribe::LATEST_LINK_NAME)
}

/// Shows the last 50 lines of the log, plain, the way they are in the file.
fn view_log() {
    let path = log_path();
    match last_lines(&path, VIEW_LOG_LINES, VIEW_LOG_CHUNK_BYTES) {
        Ok(lines) if lines.is_empty() => say("The log is empty."),
        Ok(lines) => {
            say("");
            for line in &lines {
                say(line);
            }
        }
        Err(problem) => say(&format!("Can't read the log at {}: {}.", path.display(), problem)),
    }
}

/// The last `count` lines of a file.  A log file can be hundreds of
/// megabytes, so this doesn't read the whole thing: it reads from the end,
/// `chunk_bytes` at a time, until it has seen more newlines than it needs
/// or it has reached the top.  The first line of what it read is probably
/// only the tail end of a line, but with one more newline than lines wanted
/// the ones we keep are all whole.
///
/// This goes to the file directly, not through DiskMan.  The log is Scribe's,
/// and Scribe is DiskMan's one exception already.
fn last_lines(path: &Path, count: usize, chunk_bytes: u64) -> io::Result<Vec<String>> {
    let mut file = File::open(path)?;
    let mut end = file.metadata()?.len();
    let mut bytes: Vec<u8> = Vec::new();

    loop {
        let start = end.saturating_sub(chunk_bytes);
        let mut chunk = vec![0u8; (end - start) as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut chunk)?;

        // The new chunk goes in front of what we already had.
        chunk.extend_from_slice(&bytes);
        bytes = chunk;
        end = start;

        let newlines = bytes.iter().filter(|&&byte| byte == b'\n').count();
        if newlines > count || start == 0 {
            break;
        }
    }

    // Rust note: `from_utf8_lossy` swaps any bytes that aren't valid UTF-8
    // for a placeholder character instead of failing.  A log that was cut
    // off mid-character by a crash still shows.
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<String> = text.lines().map(|line| line.to_string()).collect();
    let skip = lines.len().saturating_sub(count);
    Ok(lines.into_iter().skip(skip).collect())
}

// ---------------------------------------------------------------------------
// W) Account management
// ---------------------------------------------------------------------------

fn account_menu() {
    loop {
        say("");
        say("Account management");
        say("");
        say("  1) Make account");
        say("  2) Delete account");
        say("  3) List accounts");
        say("  4) Finger account");
        say("  5) Change password");
        say("  6) Add character to account");
        say("  B) Back");
        say("");

        let line = match read_line("> ") {
            Some(line) => line,
            None => return,
        };

        match choice_of(&line) {
            Some('1') => make_account(),
            Some('2') => delete_account(),
            Some('3') => list_accounts(),
            Some('4') => finger_account(),
            Some('5') => change_password(),
            Some('6') => add_character(),
            Some('B') => return,
            _ => say("That isn't one of the choices."),
        }
    }
}

/// Asks for everything a new account needs, one thing at a time.  Anything
/// that breaks a rule says why and gets asked again, so one typo doesn't
/// mean starting over.  An empty username or password backs out.
fn make_account() {
    let username = loop {
        let typed = match read_line("Username (Enter to cancel): ") {
            Some(typed) if !typed.is_empty() => typed,
            _ => return,
        };
        match account::check_username(&typed) {
            Ok(username) => {
                if account::list_usernames().contains(&username) {
                    say(&format!("There is already an account called {}.", username));
                } else {
                    break username;
                }
            }
            Err(problem) => say(&problem),
        }
    };

    let password = match ask_new_password() {
        Some(password) => password,
        None => return,
    };

    let email = match ask_optional("Email (Enter to skip): ", account::check_email) {
        Some(email) => email,
        None => return,
    };
    let real_name = match ask_optional("Real name (Enter to skip): ", account::check_real_name) {
        Some(real_name) => real_name,
        None => return,
    };
    let birthday = match ask_optional("Birthday, MM-DD (Enter to skip): ", account::check_birthday) {
        Some(birthday) => birthday,
        None => return,
    };

    // create_account() checks everything again.  The name could have been
    // taken in the time it took to type the rest.
    match account::create_account(&username, &password, &email, &real_name, &birthday) {
        Ok(account) => say(&format!("Account {} made.", account.username)),
        Err(problem) => say(&format!("{}  No account was made.", problem)),
    }
}

/// Makes a character on an account.  The name has to follow the game's
/// rules and be free on the whole server.  A name that isn't says why and
/// gets asked again, so one typo doesn't mean starting over.  Enter on its
/// own backs out.
fn add_character() {
    let mut account = match ask_for_account() {
        Some(account) => account,
        None => return,
    };

    loop {
        let typed = match read_line("Character name (Enter to cancel): ") {
            Some(typed) if !typed.is_empty() => typed,
            _ => return,
        };

        match character::create_character(&mut account, &typed) {
            Ok(_) => {
                say(&format!("Character {} made.  {} now has {}.",
                             account::display_name(&typed.to_ascii_lowercase()),
                             account.username, characters_text(&account)));
                return;
            }
            Err(problem) => say(&problem),
        }
    }
}

/// A new password, typed twice without showing on the screen.  `None` means
/// the admin backed out (an empty password, or Ctrl-D).
fn ask_new_password() -> Option<String> {
    loop {
        let first = read_password("Password (Enter to cancel): ")?;
        if let Err(problem) = security::check_password_rules(&first) {
            say(&problem);
            continue;
        }

        let second = read_password("Password again: ")?;
        if first == second {
            return Some(first);
        }
        say("Those don't match.  Once more, from the top.");
    }
}

/// Asks for something that can be left empty.  Hands back what was typed
/// once it passes `check`, or empty if Enter was pressed on its own.  `None`
/// means the terminal closed.
// Rust note: `check: fn(&str) -> Result<(), String>` means "hand me a
// function that takes a string and says Ok or what is wrong".  It is how
// the email, real name and birthday questions share one loop.
fn ask_optional(prompt: &str, check: fn(&str) -> Result<(), String>) -> Option<String> {
    loop {
        let typed = read_line(prompt)?;
        match check(&typed) {
            Ok(()) => return Some(typed),
            Err(problem) => say(&problem),
        }
    }
}

/// Shows what is on the account, then asks for the username a second time
/// before anything is deleted.  There is no undo.
fn delete_account() {
    let account = match ask_for_account() {
        Some(account) => account,
        None => return,
    };

    say(&format!("{} has {}.", account.username, characters_text(&account)));
    let confirm = match read_line("Type the username again to delete it for good (Enter to cancel): ") {
        Some(confirm) => confirm,
        None => return,
    };
    if confirm.to_ascii_lowercase() != account.username {
        say("That doesn't match.  Nothing was deleted.");
        return;
    }

    match account::delete_account(&account.username) {
        Ok(()) => say(&format!("Account {} deleted.", account.username)),
        Err(problem) => say(&problem),
    }
}

/// Every account and the characters on it.  This opens every account file,
/// which is fine for a game between friends.
fn list_accounts() {
    let usernames = account::list_usernames();
    if usernames.is_empty() {
        say("No accounts yet.");
        return;
    }

    say("");
    for username in &usernames {
        match account::load_account(username) {
            Ok(Some(account)) => say(&format!("  {:<16}  {}", username, characters_text(&account))),
            // A damaged file has already been logged.
            _ => say(&format!("  {:<16}  (the file can't be read -- see the log)", username)),
        }
    }
    say(&format!("{} account(s).", usernames.len()));
}

/// Everything on one account except the password hash.  This is shown on
/// the admin's screen and never logged, so the email and real name are fine
/// here.
fn finger_account() {
    let account = match ask_for_account() {
        Some(account) => account,
        None => return,
    };

    say("");
    say(&format!("  Username:     {}", account.username));
    say(&format!("  Account UID:  {}", account.account_uid));
    say(&format!("  Real name:    {}", or_not_given(&account.real_name)));
    say(&format!("  Email:        {}", or_not_given(&account.email)));
    say(&format!("  Birthday:     {}", or_not_given(&account.birthday)));
    say(&format!("  Made:         {}", scribe::time_text(account.created_at)));
    if account.last_login == 0 {
        say("  Last login:   never");
    } else {
        say(&format!("  Last login:   {}", scribe::time_text(account.last_login)));
    }

    if account.characters.is_empty() {
        say("  Characters:   none");
    } else {
        say("  Characters:");
        for character in &account.characters {
            say(&format!("    {:<12}  {}", account::display_name(&character.name), character.uuid));
        }
    }
}

/// A new password for an existing account, typed twice, the same as when
/// the account was made.  The old one isn't asked for -- this is the admin
/// fixing a forgotten password, not a player changing their own.
fn change_password() {
    let mut account = match ask_for_account() {
        Some(account) => account,
        None => return,
    };
    let password = match ask_new_password() {
        Some(password) => password,
        None => return,
    };

    match account::change_password(&mut account, &password) {
        Ok(()) => say(&format!("Password changed for {}.", account.username)),
        Err(problem) => say(&format!("{}  The password wasn't changed.", problem)),
    }
}


/// Asks for a username and loads that account.  Says why if it can't, and
/// hands back `None`, the same as for Enter on its own.
fn ask_for_account() -> Option<Account> {
    let typed = read_line("Username (Enter to cancel): ")?;
    if typed.is_empty() {
        return None;
    }

    match account::load_account(&typed) {
        Ok(Some(account)) => Some(account),
        Ok(None) => {
            say(&format!("There is no account called {}.", typed.to_ascii_lowercase()));
            None
        }
        Err(problem) => {
            say(&problem);
            None
        }
    }
}

/// "2 characters: Aldric, Mira", or "no characters".
fn characters_text(account: &Account) -> String {
    if account.characters.is_empty() {
        return "no characters".to_string();
    }
    let names: Vec<String> = account.characters.iter()
        .map(|character| account::display_name(&character.name))
        .collect();
    format!("{} character(s): {}", names.len(), names.join(", "))
}

fn or_not_given(value: &str) -> &str {
    if value.is_empty() { "(not given)" } else { value }
}

// ---------------------------------------------------------------------------
// C) Config management
// ---------------------------------------------------------------------------

fn config_menu(running: bool) {
    loop {
        say("");
        say("Config management");
        say("");
        say("  1) Show the settings");
        say("  2) Change a setting");
        say("  3) Reload the config file");
        say("  B) Back");
        say("");

        let line = match read_line("> ") {
            Some(line) => line,
            None => return,
        };

        match choice_of(&line) {
            Some('1') => {
                say("");
                // settings_text() ends in a newline already.
                let _ = write!(io::stdout(), "{}", constellations::settings_text());
            }
            Some('2') => change_setting(),
            Some('3') => reload_settings(running),
            Some('B') => return,
            _ => say("That isn't one of the choices."),
        }
    }
}

fn change_setting() {
    let line = match read_line("KEY=VALUE (Enter to cancel): ") {
        Some(line) if !line.is_empty() => line,
        _ => return,
    };

    match constellations::set(&line) {
        Ok(()) => {
            if constellations::will_save() {
                say("Changed.  It is saved to the file when the server shuts down.");
            } else {
                say("Changed, for this run only.  The config file couldn't be read, so \
                nothing gets saved to it at shutdown.");
            }
            settings_changed();
        }
        Err(problem) => say(&format!("{}  Nothing changed.", problem)),
    }
}

/// Reads the config file again, and shows its complaints here in the menu.
/// Scribe's terminal is quiet while it reads, so that while the server is
/// stopped they don't show twice (once as log lines, once here).  Then it
/// goes back to how it was: quiet while the server runs, not while it
/// doesn't.
fn reload_settings(running: bool) {
    scribe::quiet_terminal(true);
    let complaints = constellations::load();
    scribe::quiet_terminal(running);

    if complaints.is_empty() {
        say("Reloaded.  No complaints.");
    } else {
        say("Reloaded, with these complaints (they are in the log too):");
        for complaint in &complaints {
            say(&format!("  {}", complaint));
        }
    }
    settings_changed();
}

/// After a change or a reload.  Scribe gets its settings again, the same way
/// main() gave them the first time.
fn settings_changed() {
    crate::initialize_scribe();
}

// ---------------------------------------------------------------------------
// The terminal
// ---------------------------------------------------------------------------

/// One line of the menu.  Like Scribe, this never panics if the terminal
/// has gone away.
fn say(text: &str) {
    let _ = writeln!(io::stdout(), "{}", text);
}

/// Shows the prompt and reads one line, with the spaces and the newline
/// trimmed off.  `None` means the terminal closed (Ctrl-D, or a pipe that
/// ran out).
fn read_line(prompt: &str) -> Option<String> {
    let mut stdout = io::stdout();
    let _ = write!(stdout, "{}", prompt);
    // Rust note: the prompt has no newline, so it sits in a buffer until
    // something pushes it out.  `flush()` does that before we wait.
    let _ = stdout.flush();

    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) => None,
        Ok(_) => Some(line.trim().to_string()),
        Err(_) => None,
    }
}

/// Reads a password without showing it.  `None` for an empty one, for
/// Ctrl-D, and for a terminal that can't hide what is typed.  That last one
/// says so, because it is what happens when the server is run from an IDE's
/// output window instead of a real terminal.
fn read_password(prompt: &str) -> Option<String> {
    match rpassword::prompt_password(prompt) {
        Ok(password) if !password.is_empty() => Some(password),
        Ok(_) => None,
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => None,
        Err(error) => {
            say(&format!("Couldn't read a password here ({}).  Passwords need a real terminal \
-- run the server from Konsole.", error));
            None
        }
    }
}

/// A menu choice: one character, any case.  Spaces and stray control keys
/// (an Escape pressed by accident) are ignored.  Anything else, including
/// an empty line, is `None`.
fn choice_of(line: &str) -> Option<char> {
    let mut chars = line.chars().filter(|c| !c.is_whitespace() && !c.is_control());
    let first = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(first.to_ascii_uppercase())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices() {
        assert_eq!(choice_of("s"), Some('S'));
        assert_eq!(choice_of("  Q  "), Some('Q'));
        assert_eq!(choice_of("1"), Some('1'));

        assert_eq!(choice_of(""), None);
        assert_eq!(choice_of("   "), None);
        assert_eq!(choice_of("start"), None);
        assert_eq!(choice_of("sq"), None);
        
        assert_eq!(choice_of("\u{1b}q"), Some('Q'));  // Escape pressed by accident
    }

    #[test]
    fn s_follows_the_state() {
        assert_eq!(start_label(ServerState::NeverStarted), "Start server");
        assert_eq!(start_label(ServerState::Running), "Stop server");
        assert_eq!(start_label(ServerState::Stopped), "Restart server");
    }

    /// A file of our own under /tmp with `line 1` to `line N` in it, one a
    /// line.  Not the real log.
    fn numbered_file(name: &str, lines: usize, trailing_newline: bool) -> PathBuf {
        let path = std::env::temp_dir().join(format!("stratum_{}_{}.log", name, std::process::id()));
        let mut text = String::new();
        for number in 1..=lines {
            text.push_str(&format!("line {}\n", number));
        }
        if !trailing_newline {
            text.pop();
        }
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn last_lines_across_chunks() {
        // A 64 byte chunk is about 8 lines, so 50 lines takes several reads
        // and most of them start partway through a line.
        let path = numbered_file("chunks", 120, true);
        let lines = last_lines(&path, 50, 64).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(lines.len(), 50);
        assert_eq!(lines[0], "line 71");
        assert_eq!(lines[49], "line 120");
    }

    #[test]
    fn last_lines_of_a_short_file() {
        let path = numbered_file("short", 7, true);
        let lines = last_lines(&path, 50, 64).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(lines.len(), 7);
        assert_eq!(lines[0], "line 1");
        assert_eq!(lines[6], "line 7");
    }

    #[test]
    fn last_lines_without_a_trailing_newline() {
        // A crash can leave the last line unfinished.  It still counts.
        let path = numbered_file("cut", 120, false);
        let lines = last_lines(&path, 50, 64).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(lines.len(), 50);
        assert_eq!(lines[0], "line 71");
        assert_eq!(lines[49], "line 120");
    }

    #[test]
    fn last_lines_of_an_empty_file() {
        let path = numbered_file("empty", 0, true);
        let lines = last_lines(&path, 50, 64).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(lines.is_empty());
    }
}