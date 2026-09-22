//! File:     src/launcher.rs
//! Project:  Stratum Core
//! Author:   Jacob Chacko
//!
//! The Launcher, the admin's menu.  main() gets everything started, then
//! hands the terminal over to `launcher::run()`, and when that returns, the
//! server shuts down properly.  So Q is how the server gets stopped.
//!
//! While the menu is up, Scribe stays out of this terminal.  The log gets
//! its own window instead, opened with LOG_WINDOW_COMMAND from the config
//! file (`konsole -e` on the dev machine) running `tail -F` on the log.
//!
//! The main menu is the C# version's: an enum for the server state, and a
//! match on the letter and the state.  Starting and stopping are stubs for
//! now, because there is no networking to start.
//!
//! Passwords are typed with the `rpassword` crate, so they never show on
//! the screen.  The standard library can't turn the terminal's echo off.

use std::io::{self, Write};
use std::process::{Command, Stdio};
use stratum_tools::account::{self, Account};
use stratum_tools::constellations;
use stratum_tools::scribe::{self, Channel};
use stratum_tools::security;

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

/// Runs the menu until the admin picks Q, or the terminal closes (Ctrl-D).
/// A closed terminal counts as Q, and stops the server first if it is
/// running, because nobody is left to pick Stop.
pub fn run() {
    // Quiet first, so if the window won't open, the menu says so once and
    // the log line about it doesn't say it again.
    scribe::quiet_terminal(true);
    open_log_window();

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
                start_server();
                state = ServerState::Running;
            }
            (Some('W'), _) => account_menu(),
            (Some('C'), _) => config_menu(),
            (Some('Q'), ServerState::Running) => {
                say("Stop the server first.  Q only works while it isn't running.");
            }
            (Some('Q'), _) => break,
            _ => say("That isn't one of the choices."),
        }
    }

    // The shutdown lines come back to this terminal, so there is something
    // to watch while the config saves and DiskMan finishes up.
    scribe::quiet_terminal(false);
}

fn show_main_menu(state: ServerState) {
    say("");
    say(&format!("Stratum Core -- the server is {}.", state_text(state)));
    say("");
    say(&format!("  S) {}", start_label(state)));
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

// TODO(networking): starting and stopping the server is only a log line
// until there is a TCP and a UDP side to start.
fn start_server() {
    scribe::info(Channel::Core, "Server started.  (Nothing to start yet -- there is no networking.)");
    say("Server started.");
}

fn stop_server() {
    scribe::info(Channel::Core, "Server stopped.");
    say("Server stopped.");
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
        for pawn in &account.characters {
            say(&format!("    {:<12}  {}", account::display_name(&pawn.name), pawn.uuid));
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
        .map(|pawn| account::display_name(&pawn.name))
        .collect();
    format!("{} character(s): {}", names.len(), names.join(", "))
}

fn or_not_given(value: &str) -> &str {
    if value.is_empty() { "(not given)" } else { value }
}

// ---------------------------------------------------------------------------
// C) Config management
// ---------------------------------------------------------------------------

fn config_menu() {
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
            Some('3') => reload_settings(),
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
            say("Changed.  It is saved to the file when the server shuts down.");
            settings_changed();
        }
        Err(problem) => say(&format!("{}  Nothing changed.", problem)),
    }
}

fn reload_settings() {
    constellations::load();
    say("Reloaded.  Anything wrong with the file is in the log window.");
    settings_changed();
}

/// After a change or a reload.  Scribe gets its settings again, the same way
/// main() gave them the first time.  The log folder can't move on a running
/// server (it lives inside CONTENT_FOLDER), so the log window stays put.
fn settings_changed() {
    crate::initialize_scribe();
}

// ---------------------------------------------------------------------------
// The log window
// ---------------------------------------------------------------------------

/// Opens a second terminal that shows the log, using LOG_WINDOW_COMMAND with
/// `tail -n +1 -F <log folder>/latest.log` on the end.  `-n +1` starts at the
/// top of the current file, so nothing from today is missed even if the
/// window opens late, and `-F` follows the link to each new log file.
///
/// If the command is empty, or doesn't work, the admin gets the tail command
/// to run by hand.
// Rust note: `spawn()` starts the program and comes straight back without
// waiting for it.  The window outlives the server, which is what we want --
// it shows the shutdown lines too.
fn open_log_window() {
    let settings = constellations::get();
    let link = constellations::log_folder().join(scribe::LATEST_LINK_NAME);
    let by_hand = format!("tail -n +1 -F {}", link.display());

    let mut words = settings.log_window_command.split_whitespace();
    let program = match words.next() {
        Some(program) => program,
        None => {
            say(&format!("No log window (LOG_WINDOW_COMMAND is empty).  For the log, run this in \
another terminal:  {}", by_hand));
            return;
        }
    };

    // The terminal program's own chatter goes nowhere, or it would land in
    // the middle of the menu.
    let spawned = Command::new(program)
        .args(words)
        .args(["tail", "-n", "+1", "-F"])
        .arg(&link)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    match spawned {
        Ok(_) => scribe::info(Channel::Core, &format!("Opened the log window with: {}", 
                                                      settings.log_window_command)),
        Err(error) => {
            scribe::warn(Channel::Core, 
                         &format!("Couldn't open the log window with \"{}\": {}", 
                                  settings.log_window_command, error));
            say(&format!("Couldn't open the log window with \"{}\" ({}). \
                For the log, run this in \
                another terminal:  {}", settings.log_window_command, error, by_hand));
        }
    }
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

/// A menu choice: one character, any case, spaces around it ignored.
/// Anything else, including an empty line, is `None`.
fn choice_of(line: &str) -> Option<char> {
    let mut chars = line.trim().chars();
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
    }

    #[test]
    fn s_follows_the_state() {
        assert_eq!(start_label(ServerState::NeverStarted), "Start server");
        assert_eq!(start_label(ServerState::Running), "Stop server");
        assert_eq!(start_label(ServerState::Stopped), "Restart server");
    }
}