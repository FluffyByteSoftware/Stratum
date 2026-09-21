# Stratum Core

Stratum Core is a game server written in Rust.  It is the authority for a small-scale RPG set in a persistent voxel world -- the server decides what is true, and the client (Godot) shows it.

"Core" is the driver.  It starts up, launches everything else the game needs, and gets the world ready for play.  The game content itself will eventually be written in our own scripting language, modeled on LPC, which Core will run.

## State of things

This is a hobby project by one person, and it has just started.  What exists today is the logger and the config file.  That is the whole server right now: it starts, reads its settings, logs a few lines, saves its settings and shuts down.  Nothing listens on a port yet.  Things will be missing, things will break, and things will change.

## What works

- **Scribe**, the logger.  Anything in the server can call it.  Messages go to the terminal in color and to a log file that starts fresh every day (or sooner, if it gets too big).  Every message carries a priority (Debug, Info, Warn, Error), a channel that says which part of the server it came from, and the file and line number that logged it.
- **Constellations**, the configuration.  It reads a plain `KEY=VALUE` config file at launch and keeps the settings where anything in the server can get at them.  If there is no file, it writes one with the defaults in it.  A bad value gets a warning in the log and falls back to its default -- it never stops the server.  At shutdown the settings in memory are written back to the file.

Both are standard library only.

## The plan, briefly

- UDP for game traffic, TCP for authentication.
- Built for about 50 players at peak.  This is not an MMO.
- The world is chunked into zones and generated procedurally.
- Everything is saved to flat files in the LPC tradition.  No database.
- Whole files are never written in place.  We write a temp file and rename it over the old one, so a crash halfway through a save can't eat anything.
- All time is UTC, and any time we display has a Z on the end.
- As few dependencies as we can get away with.  So far that is none.

## Building

You need Rust (edition 2024, so 1.85 or newer).  Then:

```
cargo run
cargo test
```

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
│   └── constellations.rs   Constellations, the configuration.
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
