# Stratum Core

Stratum Core is a game server written in Rust.  It is the authority for a small-scale RPG set in a persistent voxel world -- the server decides what is true, and the client (Godot) shows it.

"Core" is the driver.  It starts up, launches everything else the game needs, and gets the world ready for play.  The game content itself will eventually be written in our own scripting language, modeled on LPC, which Core will run.

## State of things

This is a hobby project by one person, and it has just started.  What exists today is the logger, the config file, the thing that writes files to disk, and the password hashing.  That is the whole server right now: it starts, reads its settings, logs a few lines, saves its settings and shuts down.  Nothing listens on a port yet, and nothing has an account yet.  Things will be missing, things will break, and things will change.

## What works

- **Scribe**, the logger.  Anything in the server can call it.  Messages go to the terminal in color and to a log file that starts fresh every day (or sooner, if it gets too big).  Every message carries a priority (Debug, Info, Warn, Error), a channel that says which part of the server it came from, and the file and line number that logged it.
- **Constellations**, the configuration.  It reads a plain `KEY=VALUE` config file at launch and keeps the settings where anything in the server can get at them.  If there is no file, it writes one with the defaults in it.  A bad value gets a warning in the log and falls back to its default -- it never stops the server.  At shutdown the settings in memory are written back to the file.
- **DiskMan**, the Disk Manager.  Every file the server replaces goes through it.  It writes a temp file, forces it onto the disk, renames it over the old one, then forces the folder onto the disk too, so neither a crash nor a power cut can leave half a file behind.  Saves that the game can't wait on go into a cache, and a background thread writes them out in batches.  On a spinning disk, 50 saves take about half a second that way, and the game never waits for any of it.
- **Security**, the passwords.  A new password is hashed with Argon2id and a random salt, and only the hash is kept -- we can check a password and we can never get one back.  The hash is slow on purpose (about 85 ms), which is what makes a stolen account file expensive to crack.  Every login attempt takes the same amount of time whether the name was wrong or the password was, so nobody can tell which from outside.  Nothing calls it yet; it is waiting for accounts and for the TCP side.

The first three are standard library only.  Security uses the `argon2` crate, which is the project's one dependency.  Nobody should write their own password hash, and that includes us.

## The plan, briefly

- UDP for game traffic, TCP for authentication.  TLS on the TCP side before a real password ever crosses it.
- Built for about 50 players at peak.  This is not an MMO.
- The world is chunked into zones and generated procedurally.
- Everything is saved to flat files in the LPC tradition.  No database.
- Whole files are never written in place.  A crash rolls players back to their last save that made it to the disk.  It never corrupts one.
- All time is UTC, and any time we display has a Z on the end.
- As few dependencies as we can get away with.  So far that is one.

## Building

You need Rust (edition 2024, so 1.85 or newer).  Then:

```
cargo run
cargo test
```

`cargo test` runs in under a second.  There are also two benchmarks that don't run with it.  One times 50 saves on whatever drive `/opt/stratum` lives on, and the other times the password hash at a range of settings on your processor:

```
cargo test fifty_saves -- --ignored --nocapture
cargo test argon2_cost -- --ignored --nocapture
```

Cargo.toml builds the hashing crates optimized even in a debug build.  Without that a debug build hashes twenty times slower than the real thing and the benchmark measures the wrong server.

The server keeps its files under `/opt/stratum/content/` -- logs in `logs/`, the config file in `config/` -- and on most Linux machines `/opt` belongs to root.  Either hand the folder to your own user first:

```
sudo mkdir -p /opt/stratum
sudo chown -R $USER:$USER /opt/stratum
```

...or don't, and the server will tell you in red that it can't write there, and carry on with the built-in defaults and the terminal.

Developed on Linux.  Nothing has been tried anywhere else.

## The config file

The first run writes `/opt/stratum/content/config/stratum.conf`, with a comment above every setting.  It looks like this:

```
MAX_LOG_SIZE_MB=500
LOG_FOLDER=/opt/stratum/content/logs/
TCP_HOST_ADDRESS=10.0.0.84
TCP_PORT=9997
```

Two things to know.  The built-in defaults are the author's dev machine right now, so the addresses will want changing.  And the server writes the file back out every time it shuts down, so edit it while the server is stopped, and don't get attached to any comments you add.

## Layout

```
├── Cargo.toml
├── src/
│   ├── main.rs             Entry point.  Starts the pieces in order.
│   ├── scribe.rs           Scribe, the logger.
│   ├── constellations.rs   Constellations, the configuration.
│   ├── diskman.rs          DiskMan, the Disk Manager.
│   └── security.rs         Security, the password hashing.
└── ai/                     The project paperwork (see below).
```

## Documents

The code is written in sessions with an AI assistant (Claude).  The files in `ai/` are what it reads at the start of every session and rewrites at the end, so they are also the most honest record of where the project is.

- `STATUS.md` -- where the project is and what happened each session.
- `TODO.md` -- what we owe and what we'd like to try.
- `PROJECT_CORE.md` -- the layout of Core, file by file, and the decisions made so far.
- `PROJECT_CORE_INSTRUCTIONS.md` -- the standing instructions the assistant works from.
- `WRITINGSTYLE.md` -- how the documents and code comments are supposed to read.

## Author

Jacob Chacko

## License

Not decided yet.