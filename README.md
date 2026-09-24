# Stratum

Stratum is a game server written in Rust.  It is the authority for a small-scale RPG set in a persistent voxel world -- the server decides what is true, and the client (Godot) shows it.  The game content will eventually be written in our own scripting language, modeled on LPC.

## State of things

A hobby project by one person, and early days.  What exists is mostly the plumbing: a logger, a config file, safe file writes, password hashing, accounts, and an admin's menu in the terminal.  The server starts, reads its settings and its account files, and hands the terminal to the menu, where the admin can make and manage accounts, give them characters, and change settings.  Each character is saved in a file of its own.  "Start server" opens a TCP port, and every connection gets a thread of its own, a TLS handshake and a login against the account files.  A player who gets in sees their characters (three slots), and can make one, delete one, or pick one to play.  Picking one hands them a token, which their client sends in its first UDP packet, and the server lets them in.  Password checks run one at a time, on a thread of their own, so a rush of logins can't stall the game (we measured that before building it), and the server takes 50 players at once.  Behind the door, the world ticks: 20 times a second while the server runs, caught up when a tick runs late, and so far with nothing in it.  That is as far as anybody gets, because nothing hands a player through yet.  Things will be missing, things will break, and things will change.

## The plan, briefly

- UDP for game traffic, TCP for logins.  The TCP side is TLS from the first byte, so a password never crosses the wire in the clear.  The TCP connection stays open afterwards for chat, and as the fallback.
- Plain threads, not async.  One thread per connection.  We measured 50 of them against a pretend game loop, and the game loop didn't notice.
- The game runs on a tick, every 50 ms, on a thread of its own.  Five ticks make a round, and each piece of work takes its turn on one of them, so no single tick carries everything.  A tick that runs late isn't skipped: the ones it held up run straight after it until the clock catches up.
- Built for about 50 players at peak.  This is not an MMO.
- The world is chunked into zones and generated procedurally.
- Flat files in the LPC tradition.  No database.  A crash rolls players back to their last save.  It never corrupts one.
- All time is UTC.
- Everything that lives in the world is held in an entity component system: an entity is just an id, and the data (a name, a position, health) are components attached to it.  A player's character and an NPC are the same kind of thing with a different component saying who drives it.
- As few dependencies as we can get away with.  So far that is seven: `argon2` (nobody should write their own password hash, and that includes us), `serde` and `serde_json` (the account and character files are JSON), `rpassword` (keeping a typed password off the screen), `rustls` and `rcgen` (TLS, and making its certificate -- nobody should write their own of those either), and `bevy_ecs` (the entity component system out of the Bevy engine, used on its own).  We measured it first: a pretend game tick with 50 players and 500 NPCs took well under a millisecond.

## Layout

A Cargo workspace with five crates:

```
├── Cargo.toml              The workspace.
├── docs/PROTOCOL.md        What the server and a client say to each other, byte
│                           by byte.
├── stratum-tools/          The tools everything shares: the logger (Scribe),
│                           the config (Constellations), file writes (DiskMan),
│                           passwords (Security), UUIDs (Fingerprinter) and
│                           accounts.
├── stratum-networking/     The TCP side (a listener, TLS, the login and
│                           character select) and the UDP side (a player's
│                           first packet, so far).
├── stratum-game/           The game: what lives in the world, the character
│                           files, the character names, making and deleting
│                           characters, and the game loop that runs the tick.
│                           The world itself comes later.
├── stratum-cycle/          The tick's clock.  It says when the next tick is
│                           due, and depends on nothing.
└── stratum-launcher/       The program: starts the tools, runs the admin's menu.
```

Probe, a C# console program that pretends to be a game client so the server can be tested without Godot, lives in its own repo.  So does tick-sim, the benchmark that measured the tick, the logins and saving before any of them were built: github.com/FluffyByteSoftware/tick-sim.  Its `RESULTS.md` has the numbers.

## Building and running

You need Rust (edition 2024, so 1.85 or newer) and a C compiler, because the TLS crypto (`ring`) has some C and assembly in it.  From the repo root:

```
cargo run
cargo test
```

Run it from a real terminal, not an IDE's Run button.  The menu needs a real terminal to hide passwords.

The log goes to a file, and the menu's L) shows the last 50 lines of it.  While the server isn't running, warnings and errors also print in the terminal; once it is running, nothing does, so the log can't write over the menu.  To watch it live, run this in another terminal:

```
tail -n +1 -F /opt/stratum/content/logs/latest.log
```

The server keeps its files under `/opt/stratum/content/` (the `CONTENT_FOLDER` setting), and makes the folders it needs inside it at launch.  On most Linux machines `/opt` belongs to root.  Hand the folder to your own user first:

```
sudo mkdir -p /opt/stratum
sudo chown -R $USER:$USER /opt/stratum
```

The first run writes a config file to `/opt/stratum/content/config/stratum.conf`.  That path is fixed, even if `CONTENT_FOLDER` points somewhere else, because the config file can't tell us where the config file is.  Its built-in defaults are the author's dev machine for now, so the addresses will want changing: `TCP_HOST_ADDRESS` and `TCP_PORT` are what "Start server" listens on.  The server writes the file back out at every shutdown, so edit it while the server is stopped, or pick Reload in the menu before you quit.

The first "Start server" also makes a TLS certificate and key in `saved/ssl/`.  `key.pem` is readable only by you, and should stay that way.  `cert.pem` is what a client needs a copy of -- including a friend's machine across the internet, which is where you find out you forgot.  The certificate is self-signed, so a client has to trust that exact file rather than asking anybody to vouch for it.  To check it from the command line:

```
openssl s_client -connect <address>:<port> -CAfile /opt/stratum/content/saved/ssl/cert.pem </dev/null
```

Delete both files and a new pair gets made at the next start -- and every client needs the new `cert.pem`.

### Running it on another machine

The menu needs a terminal, and SSH gives you one.  The trouble with a plain SSH session is that the server dies with it.  `tmux` (a "terminal multiplexer") fixes that: the session lives on the server's machine, and you come and go.

```
ssh you@server
tmux new -s stratum
cd /path/to/stratum.server
cargo run --release
```

If L) in the menu isn't enough, split the window with `Ctrl-b %` and run the `tail` above in the other half; `Ctrl-b` and an arrow key moves between the two halves.  `Ctrl-b d` detaches, and the server keeps running after you log out.  To get back to it:

```
ssh you@server
tmux attach -t stratum
```

The menu is where you left it.  Quit with Q from the menu, not by closing the session -- Q is what saves the config and finishes any writes.  Ctrl-C does nothing, on purpose, so it can't skip that by accident.  If the menu itself is stuck, Ctrl-\ still kills it.

Developed on Linux.  Nothing has been tried anywhere else.

## Author

Jacob Chacko

## License

Not decided yet.