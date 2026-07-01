# Stratum

I'm building a multiplayer RPG server from scratch in C# — persistent, shared
voxel world, aiming for around 25 people online at once. It's a real project I
actually want to finish, but it's also very deliberately a learning exercise:
I'm hand-writing the low-level stuff — auth, wire protocol, the eventual ECS —
instead of reaching for an existing framework, because the point is as much
understanding *how* this works as it is shipping a game.

Nothing about the design is final. Things get ripped out and redone when I
learn something better mid-build. That's kind of the whole deal.

## Stack

- **.NET 10** everywhere — server processes *and* the shared library. (Used to
  have `Shared` pinned to netstandard2.1 for Unity's IL2CPP AOT compiler. Not
  anymore — see below.)
- **Godot 4.7 (Mono)** — the client. Was Unity 6 until recently.
- **LiteNetLib 2.1.4** — UDP transport.
- **BouncyCastle** — Ed25519 keys, Argon2id password hashing.
- **System.Text.Json + Newtonsoft.Json**, both in play. STJ is the default
  lean going forward; Newtonsoft survives in exactly one place (server-side
  config loading) because it was already there and working, not because
  anything forces it anymore.

## Unity → Godot

Worth explaining, since it ended up touching the whole `Shared` library.

The client has hundreds of animated FBX assets, and Unity kept mangling the
import — bone names and hierarchies breaking, tracks silently dropping. Got
tired of fighting it, tried the same content in Godot 4.7 via direct glTF
(`.glb`) import instead of going through FBX, and it just worked. No lossy
conversion hop in the middle, animations came in clean.

That was the whole trigger. The server never cared what engine renders the
client — it just speaks a wire protocol — so the switch touched zero server
design. What it *did* unlock: `Shared` no longer needs to compile under
Unity's IL2CPP AOT floor, so it's net10.0 now like everything else, and
System.Text.Json is usable client-side too (Godot's Mono export is JIT, not
AOT, so the reflection-based STJ that IL2CPP used to strip out just works).

## Architecture

Star topology. The client only ever talks to Sentinel. Zones never talk to
each other — everything routes through ZoneManager. Every box below is its
own process.

- **LoginServer** — TLS/TCP. Ed25519 key auth for the normal case, Argon2id
  password fallback for first run / expired key. Also hosts character
  creation — an account with no character gets held open on the same
  connection to finish creating one before a token is ever minted.
- **Sentinel** — UDP front door. Validates the session token LoginServer
  issued, tracks live sessions, runs the protocol version check, echoes a
  keep-alive ping. Eventually routes real gameplay traffic between clients and
  zones — that half isn't built yet.
- **Core** — the operator console. Account management, starts/stops
  LoginServer and Sentinel together, and runs a startup reconciler that heals
  the account↔character link if it ever drifts out of sync.
- **ZoneManager** — master clock, cross-zone coordination, zone lifecycle.
  *Not built.*
- **Zones** — one process per zone, authoritative simulation. *Not built.*

## Solution layout

```
Stratum.slnx
├── SystemTools/    net10.0  — disk/log infra, accounts, characters, crypto, config
├── Shared/         net10.0  — wire packets, packet IDs, shared enums (Species, etc.)
├── Networking/     net10.0  — TCP + UDP transport, packet dispatcher
├── LoginServer/    net10.0  — auth + character-create exe
├── Sentinel/       net10.0  — UDP front door exe
├── Core/           net10.0  — operator console exe
└── Probe/          net10.0  — standing regression tool (six legs)
```

**Rule of thumb:** if the client would never need it, it doesn't belong in
`Shared`. `Shared` is the contract between client and server, nothing else.

## Authentication

- **TCP (LoginServer):** key auth for returning players — sign a timestamp
  with your Ed25519 key. Password auth is the fallback, and it also rotates
  in a fresh keypair on success. Neither path mints a token unless the
  account already owns a character; accounts without one get routed into
  character creation on the same connection instead.
- **UDP (Sentinel):** the session token rides inside the LiteNetLib
  connection request itself — there's no separate typed auth packet, the
  connection request *is* the auth request.
- **Version check:** right after UDP auth, Sentinel and the client trade
  protocol version strings. Mismatch means disconnect, before any gameplay
  traffic flows.

## Characters

One character per account, on purpose — accounts are free to make, so "I want
a second character" just means "make a second account." Each character is its
own `characters/{name}.json` file, name-keyed, so the directory listing *is*
the roster and two characters can't collide on a name by construction.

Creating a character happens over the wire, on the same TLS connection you
just authenticated on — you send a name, the server figures out which account
you are from the connection itself (it never trusts the packet for that), and
on success the connection closes so you re-authenticate and land the normal
token path, now that you actually own a character.

A startup reconciler in Core double-checks the account↔character link is
intact and will self-heal anything additive, but it will never delete a
character or sever a real link on its own — a character file is about the
least replaceable thing on disk here.

## Persistence

Flat files, no database. Atomic writes (write to temp, fsync, rename).
Everything hangs off one shared data root:

```
data/config/        runtime config, not committed
data/certs/          TLS cert
data/keys/           session signing key, shared by LoginServer + Sentinel
data/accounts/       {id}.json
data/characters/     {name}.json
data/logs/           per-process log files
```

## Status

**Working, end to end:**
- Full login flow — key auth, password auth, character creation over the
  wire, re-auth, UDP handoff, version check. All exercised by
  `Stratum.Probe`, a six-leg regression tool that's the gate before any
  auth/wire/`Shared` change gets committed.
- Keep-alive ping/pong between client and Sentinel.
- Core's account management, server launch/stop, and startup reconciler.
- The net10.0 retarget and the Godot migration — Probe-verified, not just
  "it compiles."

**Not built yet:**
- Any actual gameplay packets. There's a channel reserved (`0x03`) for it,
  and that's next up.
- ZoneManager, Zones, the ECS, voxels, AI — the whole simulation side is
  still just design notes.
- The Godot client itself. The server's being proven out first; the client
  comes after.

## License

See [License.txt](License.txt).
