//! File:     src/security.rs
//! Project:  Stratum Core
//! Author:   Jacob Chacko
//!
//! Security.  Passwords now, and login tokens once there is a TCP side to
//! hand them out on.
//!
//! A password gets hashed with Argon2id and a random salt, and only the hash
//! is ever kept.  Hashing is one-way on purpose -- we can check a password,
//! and we can never get one back.  The hash is slow on purpose too (about 85
//! ms on the dev machine), because a slow hash is what makes a stolen account
//! file expensive to crack.  Every login pays that cost, so we measured it
//! before picking a number.  The benchmark is at the bottom of the file.
//!
//! What goes in the account file is one line of text in the standard "PHC
//! string" format, and it carries its own settings:
//!
//!     $argon2id$v=19$m=65536,t=2,p=1$<salt>$<hash>
//!
//! So when we make the hash slower in a year, old accounts still check
//! against their old settings, and nobody gets locked out.
//!
//! This file hands back strings and checks strings.  It never reads or
//! writes an account file -- that is the accounts code's job, through
//! DiskMan.  And it never logs a password, not even a wrong one.
//!
//! A login that fails always says the same thing, whether the name or the
//! password was wrong.  That is the caller's job.  Ours is the other half:
//! every login attempt takes the same amount of time, whichever path it
//! took, so nobody can tell a real name from a made-up one with a stopwatch.
//! See `pad_login_time()`.
//!
//! Standard library plus one crate: argon2, our first.  Nobody should write
//! their own password hash, and that includes us.

use std::thread;
use std::time::{Duration, Instant};

use argon2::password_hash::phc::PasswordHash;
use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};

use crate::scribe::{self, Channel};

// ---------------------------------------------------------------------------
// The numbers
// ---------------------------------------------------------------------------

/// How much memory one hash uses, in KiB.  This is the knob that matters:
/// memory is what makes a graphics card's job expensive.  64 MiB and 2
/// passes measured about 85 ms on the dev machine, which is roughly four
/// times OWASP's floor.  When we want a harder hash, raise this before
/// raising the passes.
const HASH_MEMORY_KIB: u32 = 64 * 1024;

/// How many passes over that memory.  More passes, more time, same memory.
const HASH_PASSES: u32 = 2;

/// Lanes.  The crate only runs them in parallel with an extra feature that
/// pulls in another crate, so more than one buys nothing here.
const HASH_LANES: u32 = 1;

/// How long every login attempt takes, at least, in milliseconds.  It has
/// to be comfortably above the hash (85 ms, worst 90, more when two run at
/// once), or a slow hash pokes out over the top and the timing leak is back.
/// It also caps every connection at a few guesses a second.
const MIN_LOGIN_MILLIS: u64 = 150;

/// The password rules.  Jacob's.  Between the two lengths, printable ASCII
/// only (anything on a US keyboard, spaces included), and at least one
/// digit, one capital letter and one symbol.
///
/// The maximum isn't about the hash -- ten million characters hash in the
/// same 85 ms as eight, because Argon2 boils the password down with a quick
/// hash first.  It is about how big a "password" we let a client send us.
const MIN_PASSWORD_CHARS: usize = 8;
const MAX_PASSWORD_CHARS: usize = 128;

// ---------------------------------------------------------------------------
// What the rest of the server calls
// ---------------------------------------------------------------------------

/// The settings every new hash is made with.
fn current_params() -> Params {
    // Rust note: Params::new() says no to settings Argon2 doesn't allow.  Ours
    // are constants that the tests check, so `expect` can't actually trip
    // here -- and if it somehow does, "can't make a hash" is worth stopping
    // the server over.
    Params::new(HASH_MEMORY_KIB, HASH_PASSES, HASH_LANES, None)
        .expect("Argon2 turned down Security's hash settings")
}

/// Hashes a new password, with a fresh random salt, and hands back the line
/// that goes in the account file.  About 85 ms.
///
/// The salt comes from the operating system's random source, through the
/// crate.  If that fails (it doesn't, on Linux) there is no safe way to make
/// a hash, so we say so on the Security channel and hand back the error.
/// The caller should not create the account.
///
/// This doesn't check the password rules.  Call `check_password_rules()`
/// first, and tell the player what they got wrong.
// No customer until the TCP side exists, so the compiler is told to be
// quiet.
#[allow(dead_code)]
#[track_caller]
pub fn hash_password(password: &str) -> Result<String, String> {
    match hash_with(&current_params(), password) {
        Ok(stored) => Ok(stored),
        Err(reason) => {
            scribe::error(Channel::Security,
                          &format!("Security couldn't hash a password ({}).  No \
                          account should be created until this is fixed.",
                                   reason));
            Err(reason)
        }
    }
}

/// Checks a typed password against the line from the account file.  True if
/// it matches.  About 85 ms, whether it matches or not.
///
/// The settings come out of the stored line, not out of the constants above,
/// so an account hashed under old settings still checks.
///
/// A stored line that can't be read as a hash is a corrupt account file, and
/// that gets an Error line on the Security channel (without the line itself
/// in it -- a salt and a hash have no business in a log).  The password is
/// wrong as far as the caller is concerned.
// No customer until the TCP side exists, so the compiler is told to be
// quiet.
#[allow(dead_code)]
#[track_caller]
pub fn verify_password(password: &str, stored: &str) -> bool {
    match check_password(password, stored) {
        Ok(matched) => matched,
        Err(reason) => {
            scribe::error(Channel::Security,
                          &format!("Security was handed a stored password hash it \
                          can't read ({}).  That account file is damaged.  \
                          Treating the password as wrong.", reason));
            false
        }
    }
}

/// Says whether a new password is allowed, and if not, why, in words meant
/// for the player.  Doesn't log, doesn't hash.
// No customer until the TCP side exists, so the compiler is told to be
// quiet.
#[allow(dead_code)]
pub fn check_password_rules(password: &str) -> Result<(), String> {
    // Rust note: `chars().count()` counts characters and `len()` counts
    // bytes.  They are the same for ASCII, which is all we allow, but the
    // length is checked first and the ASCII rule second, so it has to be
    // the one that is right either way.
    let length = password.chars().count();
    if length < MIN_PASSWORD_CHARS {
        return Err(format!("A password needs at least {} characters.",
                           MIN_PASSWORD_CHARS));
    }
    if length > MAX_PASSWORD_CHARS {
        return Err(format!("A password can't be longer than {} characters.",
                           MAX_PASSWORD_CHARS));
    }

    // Printable ASCII is space (32) through tilde (126).  Nothing outside
    // that, because the same accented letter can arrive as two different
    // byte sequences, and then a password that looks right doesn't match.
    if !password.chars().all(|c| (' '..='~').contains(&c)) {
        return Err("A password can only use letters, digits, spaces and the \
        symbols on a US keyboard.".to_string());
    }

    if !password.chars().any(|c| c.is_ascii_digit()) {
        return Err("A password needs at least one digit.".to_string());
    }
    if !password.chars().any(|c| c.is_ascii_uppercase()) {
        return Err("A password needs at least one capital letter.".to_string());
    }
    if !password.chars().any(|c| c.is_ascii_punctuation()) {
        return Err("A password needs at least one symbol, like ! or #.".to_string());
    }

    Ok(())
}

/// Makes a login attempt take at least MIN_LOGIN_MILLIS from `started`,
/// however much or little work it did.  The caller notes the clock when the
/// attempt arrives, does the work (or skips it, for a name that doesn't
/// exist), and calls this before answering.
///
/// A real name costs a hash and a made-up name costs nothing, and without
/// this the reply time says which is which.  With it, every attempt answers
/// at the same moment.  The waiting happens on the connection's own thread,
/// so it only ever holds up the one client who is logging in.
// No customer until the TCP side exists, so the compiler is told to be
// quiet.
#[allow(dead_code)]
pub fn pad_login_time(started: Instant) {
    let floor = Duration::from_millis(MIN_LOGIN_MILLIS);
    let spent = started.elapsed();
    if spent < floor {
        thread::sleep(floor - spent);
    }
}

// ---------------------------------------------------------------------------
// The work
// ---------------------------------------------------------------------------

// These two do the hashing and don't log, which is what lets the tests run
// them.  The public functions above are the same thing plus the complaint.

/// The PHC string for `password` under `params`.  The error is the crate's
/// reason, as text.
fn hash_with(params: &Params, password: &str) -> Result<String, String> {
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params.clone());
    match argon2.hash_password(password.as_bytes()) {
        Ok(hash) => Ok(hash.to_string()),
        Err(problem) => Err(problem.to_string()),
    }
}

/// Whether `password` matches `stored`.  The error only means the stored
/// line couldn't be read as a hash -- a wrong password is `Ok(false)`.
fn check_password(password: &str, stored: &str) -> Result<bool, String> {
    // Rust note: `PasswordHash::new` parses the stored line and pulls the
    // algorithm, the settings, the salt and the hash out of it.  The `?`
    // hands its error to whoever called us if the line is garbage.
    let hash = PasswordHash::new(stored).map_err(|problem| problem.to_string())?;

    // The crate is happy to parse a line with the salt or the hash missing
    // off the end, and then it calls every password wrong.  That is a
    // damaged file, not a wrong password, and somebody should hear about it.
    if hash.salt.is_none() || hash.hash.is_none() {
        return Err("the salt or the hash is missing off the end".to_string());
    }

    // A default Argon2 here is fine, because verify takes every setting from
    // the stored line and ignores the ones it was built with.
    match Argon2::default().verify_password(password.as_bytes(), &hash) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::PasswordInvalid) => Ok(false),
        Err(problem) => Err(problem.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// None of these go down a path that logs.  The public functions log when
// something is broken, so the tests use the private ones underneath them.
// The real hash costs 85 ms, so most of these run at the cheapest setting
// Argon2 allows and one runs at the real one.

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest, fastest settings Argon2 accepts.  A hash at this cost
    /// is worthless as protection and takes well under a millisecond.
    fn cheap_params() -> Params {
        Params::new(Params::MIN_M_COST, 1, 1, None).unwrap()
    }

    #[test]
    fn the_right_password_matches_and_the_wrong_one_does_not() {
        let stored = hash_with(&cheap_params(), "Correct horse 1!").unwrap();

        assert_eq!(check_password("Correct horse 1!", &stored), Ok(true));
        assert_eq!(check_password("Correct horse 1?", &stored), Ok(false));
        assert_eq!(check_password("", &stored), Ok(false));
    }

    #[test]
    fn two_hashes_of_one_password_differ() {
        // A fresh salt every time, so a stolen file can't say which two
        // accounts share a password.
        let first = hash_with(&cheap_params(), "Same password 1!").unwrap();
        let second = hash_with(&cheap_params(), "Same password 1!").unwrap();

        assert_ne!(first, second);
        assert_eq!(check_password("Same password 1!", &first), Ok(true));
        assert_eq!(check_password("Same password 1!", &second), Ok(true));
    }

    #[test]
    fn the_stored_line_carries_the_real_settings() {
        // The one test at the real cost.  It checks what a new account file
        // will actually hold, and that the settings in it are ours.
        let stored = hash_with(&current_params(), "Real cost 1!").unwrap();

        assert!(stored.starts_with("$argon2id$v=19$m=65536,t=2,p=1$"),
                "the stored line was {}", stored);
        assert_eq!(stored.matches('$').count(), 5);
        // One `=` in `v=19` and three in the settings.  The salt and the
        // hash are unpadded base64, so they never have one.  That matters
        // because a KEY=VALUE file splits on the first `=`.
        assert_eq!(stored.matches('=').count(), 4);
        assert_eq!(check_password("Real cost 1!", &stored), Ok(true));
    }

    #[test]
    fn a_hash_checks_under_its_own_settings_not_ours() {
        // What happens to an old account after we raise the cost: the stored
        // line still says what it was hashed with, and that is what gets
        // used.  This one was made at the cheap setting, and current_params()
        // never enters into checking it.
        let stored = hash_with(&cheap_params(), "Old account 1!").unwrap();
        assert!(stored.contains("m=8,t=1,p=1"), "the stored line was {}", stored);
        assert_eq!(check_password("Old account 1!", &stored), Ok(true));
    }

    #[test]
    fn a_damaged_stored_line_is_an_error_not_a_match() {
        assert!(check_password("Anything 1!", "").is_err());
        assert!(check_password("Anything 1!", "not a hash at all").is_err());
        assert!(check_password("Anything 1!", "$argon2id$v=19$m=65536").is_err());
    }

    #[test]
    fn the_password_rules() {
        assert_eq!(check_password_rules("Abcdef1!"), Ok(()));
        assert_eq!(check_password_rules("correct Horse battery staple 1#"), Ok(()));

        assert!(check_password_rules("Abcde1!").is_err());
        assert!(check_password_rules(&format!("Abcdef1!{}", "x".repeat(121))).is_err());
        assert_eq!(check_password_rules(&format!("Abcdef1!{}", "x".repeat(120))), Ok(()));

        assert!(check_password_rules("abcdefg1!").is_err());
        assert!(check_password_rules("Abcdefgh!").is_err());
        assert!(check_password_rules("Abcdefgh1").is_err());

        // A tab, an accented letter and a newline are all outside printable
        // ASCII.
        assert!(check_password_rules("Abcdef1!\t").is_err());
        assert!(check_password_rules("Abcd\u{e9}f1!").is_err());
        assert!(check_password_rules("Abcdef1!\n").is_err());
    }

    #[test]
    fn our_settings_are_ones_argon2_accepts() {
        // current_params() uses `expect`, so this is the test that keeps that
        // from ever firing on a real server.
        let params = current_params();
        assert_eq!(params.m_cost(), HASH_MEMORY_KIB);
        assert_eq!(params.t_cost(), HASH_PASSES);
        assert_eq!(params.p_cost(), HASH_LANES);
    }

    #[test]
    fn a_login_never_answers_early() {
        // No work at all, which is the "no such name" path.
        let started = Instant::now();
        pad_login_time(started);
        assert!(started.elapsed() >= Duration::from_millis(MIN_LOGIN_MILLIS));

        // Work that already took longer than the floor doesn't wait again.
        let long_ago = Instant::now() - Duration::from_millis(MIN_LOGIN_MILLIS * 2);
        let started = Instant::now();
        pad_login_time(long_ago);
        assert!(started.elapsed() < Duration::from_millis(MIN_LOGIN_MILLIS / 2));
    }

    // -----------------------------------------------------------------------
    // The benchmark
    // -----------------------------------------------------------------------

    /// How many hashes we time at each setting.  The middle one is the number
    /// that counts, and the worst one says how moody the machine is.
    const BENCH_RUNS: usize = 7;

    /// The memory settings to try, in MiB.  19 is OWASP's floor for Argon2id,
    /// and the argon2 crate's default.  64 is the setting RFC 9106 suggests
    /// for machines short on memory.
    const BENCH_MEMORY_MIB: [u32; 5] = [19, 32, 46, 64, 128];

    /// How many passes over that memory.  More passes, more time, same memory.
    const BENCH_PASSES: [u32; 3] = [1, 2, 3];

    /// The setting used for the "several logins at once" part.  The crate's
    /// default, until the first half of the benchmark tells us better.
    const CROWD_MEMORY_MIB: u32 = 19;
    const CROWD_PASSES: u32 = 2;

    /// Times Argon2id at a range of settings, then times several hashes
    /// running at the same moment.  Not part of a normal `cargo test` -- run
    /// it by hand:
    ///
    ///     cargo test argon2_cost -- --ignored --nocapture
    ///
    /// The first half picks how slow one login should be.  The second half
    /// says how many logins the machine can hash at once before they start
    /// slowing each other down, which is the number a login queue would cap.
    #[test]
    #[ignore]
    fn argon2_cost_benchmark() {
        let cores = match thread::available_parallelism() {
            Ok(count) => count.get(),
            Err(_) => 1,
        };

        println!();
        println!("Argon2id, 1 lane, {} hashes per setting.  This machine has {} \
        core(s).", BENCH_RUNS, cores);
        println!();
        println!("  memory  passes    middle     worst");

        for memory_mib in BENCH_MEMORY_MIB {
            for passes in BENCH_PASSES {
                let mut times = time_hashes(memory_mib, passes, BENCH_RUNS);
                times.sort();
                println!("  {:>3} MiB  {:>4}   {:>6.1} ms  {:>6.1} ms", memory_mib,
                         passes, millis(times[BENCH_RUNS / 2]),
                         millis(times[BENCH_RUNS - 1]));
            }
        }

        println!();
        println!("Several at once, {} MiB and {} passes, {} hashes on each thread:",
                 CROWD_MEMORY_MIB, CROWD_PASSES, BENCH_RUNS);
        println!();
        println!("  at once    per hash    all done   memory in use");

        for at_once in [1, 2, 4, 8, 16] {
            let started = Instant::now();
            let mut all_times = Vec::new();

            // Rust note: `thread::scope` starts threads that are guaranteed to
            // finish before the scope ends.  Each one hands its list of times
            // back through `join()`.
            thread::scope(|scope| {
                let mut handles = Vec::new();
                for _ in 0..at_once {
                    handles.push(scope.spawn(|| {
                        time_hashes(CROWD_MEMORY_MIB, CROWD_PASSES, BENCH_RUNS)
                    }));
                }
                for handle in handles {
                    all_times.extend(handle.join().unwrap());
                }
            });

            all_times.sort();
            println!("  {:>7}   {:>6.1} ms  {:>7.1} ms   {:>5} MiB", at_once,
                     millis(all_times[all_times.len() / 2]),
                     millis(started.elapsed()),
                     at_once as u32 * CROWD_MEMORY_MIB);
        }
        println!();
    }

    /// Hashes a password `count` times at one setting and hands back how long
    /// each one took.  One untimed hash goes first, so the memory has been
    /// handed out once before the clock starts.
    fn time_hashes(memory_mib: u32, passes: u32, count: usize) -> Vec<Duration> {
        // Rust note: Params::new() says no to settings Argon2 doesn't allow,
        // and `expect` stops the benchmark with that message if it does.
        // None of ours should trip it.
        let params = Params::new(memory_mib * 1024, passes, 1, Some(32))
            .expect("Argon2 turned down the benchmark's settings");
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        // The salt would be random for real.  It makes no difference to the
        // time, so here it is sixteen sevens.
        let salt = [7u8; 16];
        let mut hash = [0u8; 32];

        argon2.hash_password_into(b"warming up", &salt, &mut hash)
            .expect("Argon2 couldn't hash the warm-up password");

        let mut times = Vec::new();
        for _ in 0..count {
            let started = Instant::now();
            argon2.hash_password_into(b"correct horse battery staple", &salt, &mut hash)
                .expect("Argon2 couldn't hash the test password");
            times.push(started.elapsed());
        }
        times
    }

    fn millis(duration: Duration) -> f64 {
        duration.as_secs_f64() * 1000.0
    }
}