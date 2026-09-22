# Stratum

Stratum is a game server written in Rust.  It is the authority for a small-scale RPG set in a persistent voxel world -- the server decides what is true, and the client (Godot) shows it.  The game content will eventually be written in our own scripting language, modeled on LPC.

## State of things

A hobby project by one person, and early days.  What exists is the plumbing: a logger, a config file, safe file writes, password hashing, accounts, and an admin's menu in the terminal.  The server starts, reads its settings and its account files, and hands the terminal to the menu, where the admin can make and manage accounts and change settings.  Nothing listens on a port yet, so "Start server" doesn't start anything.  Networking is next.  Things will be missing, things will break, and things will change.

## The plan, briefly

- UDP for game traffic, TCP for logins.  TLS on the TCP side before a real password ever crosses it.
- Built for about 50 players at peak.  This is not an MMO.
- The world is chunked into zones and generated procedurally.
- Flat files in the LPC tradition.  No database.  A crash rolls players back to their last save.  It never corrupts one.
- All time is UTC.
- As few dependencies as we can get away with.  So far that is four: `argon2` (nobody should write their own password hash, and that includes us), `serde` and `serde_json` (the account files are JSON), and `rpassword` (keeping a typed password off the screen).

## Layout

A Cargo workspace, with room for three crates:

```
├── Cargo.toml              The workspace.
├── stratum-tools/          The tools everything shares: the logger (Scribe),
│                           the config (Constellations), file writes (DiskMan),
│                           passwords (Security) and accounts.
├── stratum-launcher/       The program: starts the tools, runs the admin's menu.
├── stratum-networking/     The TCP and UDP sides.  Not written yet.
└── ai/                     The project paperwork (see below).
```

Probe, a C# console program that pretends to be a game client so the server can be tested without Godot, lives in its own repo.

## Building and running

You need Rust (edition 2024, so 1.85 or newer).  From the repo root:

```
cargo run
cargo test
```

Run it from a real terminal (Konsole, or whatever yours is), not an IDE's Run button.  The menu needs a real terminal to hide passwords.

When the menu comes up, a second window opens with the log in it.  The command that opens it is `LOG_WINDOW_COMMAND` in the config file, which starts out as `konsole -e` because that is what the author's machine has.  Change it to your own terminal's "run this command" form (`gnome-terminal --`, `xterm -e`), or leave it empty and run this in another terminal yourself:

```
tail -n +1 -F /opt/stratum/content/logs/latest.log
```

The server keeps its files under `/opt/stratum/content/` (the `CONTENT_FOLDER` setting), and makes the folders it needs inside it at launch.  On most Linux machines `/opt` belongs to root.  Hand the folder to your own user first:

```
sudo mkdir -p /opt/stratum
sudo chown -R $USER:$USER /opt/stratum
```

The first run writes a config file to `/opt/stratum/content/config/stratum.conf`.  That path is fixed, even if `CONTENT_FOLDER` points somewhere else, because the config file can't tell us where the config file is.  Its built-in defaults are the author's dev machine for now, so the addresses will want changing.  The server writes the file back out at every shutdown, so edit it while the server is stopped, or pick Reload in the menu before you quit.

Developed on Linux.  Nothing has been tried anywhere else.

## Documents

The code is written in sessions with an AI assistant (Claude), in a few separate projects.  The files in `ai/` are what it reads at the start of every session and rewrites at the end.

- `ai/STATUS.md` -- where things are, and what the next session is for.
- `ai/TODO.md` -- what we owe and what we'd like to try.
- `ai/WRITINGSTYLE.md` -- how the documents and code comments are supposed to read.
- `ai/core/` and `ai/networking/` -- each project's layout and its standing instructions.

## Author

Jacob Chacko

## License

Not decided yet.