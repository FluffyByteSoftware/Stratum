# Stratum

Stratum is a game server written in Rust.  It is the authority for a small-scale RPG set in a persistent voxel world -- the server decides what is true, and the client (Godot) shows it.  The game content will eventually be written in our own scripting language, modeled on LPC.

## State of things

A hobby project by one person, and early days.  What exists is the plumbing: a logger, a config file, safe file writes, password hashing, accounts, and an admin's menu in the terminal.  The server starts, reads its settings and its account files, and hands the terminal to the menu, where the admin can make and manage accounts and change settings.  "Start server" opens a TCP port, and every connection gets a thread of its own, a TLS handshake and a login against the account files.  A player who gets in stays connected, but there is nothing to do yet: no characters to pick, no world, no UDP.  Things will be missing, things will break, and things will change.

## The plan, briefly

- UDP for game traffic, TCP for logins.  The TCP side is TLS from the first byte, so a password never crosses the wire in the clear.  The TCP connection stays open afterwards for chat, and as the fallback.
- Plain threads, not async.  One thread per connection.  We measured 50 of them against a pretend game loop, and the game loop didn't notice.
- Built for about 50 players at peak.  This is not an MMO.
- The world is chunked into zones and generated procedurally.
- Flat files in the LPC tradition.  No database.  A crash rolls players back to their last save.  It never corrupts one.
- All time is UTC.
- As few dependencies as we can get away with.  So far that is six: `argon2` (nobody should write their own password hash, and that includes us), `serde` and `serde_json` (the account files are JSON), `rpassword` (keeping a typed password off the screen), and `rustls` and `rcgen` (TLS, and making its certificate -- nobody should write their own of those either).

## Layout

A Cargo workspace with three crates:

```
├── Cargo.toml              The workspace.
├── docs/PROTOCOL.md        What the server and a client say to each other, byte
│                           by byte.
├── stratum-tools/          The tools everything shares: the logger (Scribe),
│                           the config (Constellations), file writes (DiskMan),
│                           passwords (Security) and accounts.
├── stratum-networking/     The TCP side (a listener, TLS and the login, so far)
│                           and the UDP side (not written yet).
└── stratum-launcher/       The program: starts the tools, runs the admin's menu.
```

Probe, a C# console program that pretends to be a game client so the server can be tested without Godot, lives in its own repo.

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

The first "Start server" also makes a TLS certificate and key in `saved/ssl/`.  `key.pem` is readable only by you, and should stay that way.  `cert.pem` is what a client needs a copy of.  The certificate is self-signed, so a client has to trust that exact file rather than asking anybody to vouch for it.  To check it from the command line:

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

The menu is where you left it.  Quit with Q from the menu, not by closing the session -- Q is what saves the config and finishes any writes.

Developed on Linux.  Nothing has been tried anywhere else.

## Author

Jacob Chacko

## License

Not decided yet.