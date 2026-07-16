# Stratum

I'm building a multiplayer RPG server from scratch in C#. Persistent, shared
voxel world, aiming for around 25 people online at once. It's a real project I
actually want to finish, but it's also very deliberately a learning exercise:
I'm hand-writing the low-level stuff (auth, wire protocol, the eventual ECS)
instead of reaching for an existing framework, because the point is as much
understanding *how* this works as it is shipping a game.

Nothing about the design is final. Things get ripped out and redone when I
learn something better mid-build. That's kind of the whole deal.

## Stack

- **.NET 10** everywhere, server processes *and* the shared library. (Used to
  have `Shared` pinned to netstandard2.1 for Unity's IL2CPP AOT compiler. Not
  anymore, see below.)
- **Godot 4.7 (Mono)** for the client. Was Unity 6 until recently.
- **LiteNetLib 2.1.4** for UDP transport.
- **BouncyCastle** for Ed25519 keys and Argon2id password hashing.
- **System.Text.Json + Newtonsoft.Json**, both in play. STJ is the default
  lean going forward; Newtonsoft survives where it was already working, and
  gets collapsed opportunistically rather than ripped out on principle.

## Unity to Godot

Worth explaining, since it ended up touching the whole `Shared` library.

The client has hundreds of animated FBX assets, and Unity kept mangling the
import: bone names and hierarchies breaking, tracks silently dropping. Got
tired of fighting it, tried the same content in Godot 4.7 via direct glTF
(`.glb`) import instead of going through FBX, and it just worked. No lossy
conversion hop in the middle, animations came in clean.

That was the whole trigger. The server never cared what engine renders the
client, it just speaks a wire protocol, so the switch touched zero server
design. What it *did* unlock: `Shared` no longer needs to compile under
Unity's IL2CPP AOT floor, so it's net10.0 now like everything else, and
System.Text.Json is usable client-side too (Godot's Mono export is JIT, not
AOT, so the reflection-based STJ that IL2CPP used to strip out just works).

And the client is no longer a someday item. ForgeClient (Godot, private repo
beside this one for asset protection) now walks the entire verified login
chain live against a running server, polling LiteNetLib on a dedicated
background thread so the render loop can't starve the connection. It even
proved a path the regression tool never touches: rejecting an invalid
character name and retrying on the same held-open connection.

## Architecture

Star topology. The client only ever talks to Sentinel. Zones never talk to
each other; everything routes through ZoneManager. Every box below is its
own process.

- **LoginServer**: TLS/TCP. Ed25519 key auth for the normal case, Argon2id
  password fallback for first run or an expired key. Also hosts character
  creation. An account with no character gets held open on the same
  connection to finish creating one before a token is ever minted.
- **Sentinel**: UDP front door. Validates the session token LoginServer
  issued, tracks live sessions, runs the protocol version check, echoes a
  keep-alive ping, and echoes a diagnostic ping on the gameplay channel to
  prove the firehose route works before real gameplay traffic exists. The
  client-to-zone bridge itself isn't built yet, and now there's a real
  client holding live sessions on the other side of that seam.
- **Core**: the operator console. Account management, starts and stops
  LoginServer and Sentinel together, and runs a startup reconciler that heals
  the account-to-character link if it ever drifts out of sync.
- **ZoneManager**: master clock, cross-zone coordination, zone lifecycle.
  The registration tier is built and proven on the wire: it verifies a
  dialing zone's signed marker, admits it into a first-wins registry, and
  evicts on disconnect. It also boot-scans `data/zones/` for zone manifests,
  so it knows which zones *exist* separately from which are *running*. The
  master clock is still to come.
- **Zone**: one process per zone, authoritative simulation for that zone
  only. Dials outward to ZoneManager, registers itself, and runs a 60 Hz
  heartbeat gated on successful registration. The simulation side hasn't
  started.

## Authentication

- **TCP (LoginServer):** key auth for returning players. Sign a timestamp
  with your Ed25519 key. Password auth is the fallback, and it also rotates
  in a fresh keypair on success. Neither path mints a token unless the
  account already owns a character; accounts without one get routed into
  character creation on the same connection instead.
- **UDP (Sentinel):** the session token rides inside the LiteNetLib
  connection request itself. There's no separate typed auth packet, the
  connection request *is* the auth request.
- **Version check:** right after UDP auth, Sentinel and the client trade
  protocol version strings. Mismatch means disconnect, before any gameplay
  traffic flows.

## Zone registration

Same idea as client key auth, turned inward for the server mesh. A zone proves
who it is to ZoneManager the same way a returning player proves who they are to
LoginServer: it signs, ZoneManager verifies. This whole round trip has now
been run for real on the wire, two processes talking, not just compiled.

- The zone signs a marker (its own zone id plus a timestamp) with an Ed25519
  registration seed, and that signed marker rides inside the LiteNetLib
  connection request, mirroring the Sentinel pattern. The zone id is inside
  the signed bytes, so a valid signature authenticates the identity, not just
  possession of the seed.
- The seed is asymmetric by generation authority. ZoneManager is the *only*
  side that can generate the keypair; a zone is load-only and will refuse to
  start rather than mint its own identity. The seed gets cloned out to each
  zone host out of band, after ZoneManager has generated and flushed it.
- ZoneManager verifies the signature before admitting the connection, then
  sends back a confirmation packet. Registration is first-wins per zone id,
  and disconnect evicts the entry. It's server-only control-plane traffic on
  its own channel, so Sentinel's client bridge can't forward it by accident.

Signature verification only for now; a timestamp-freshness (replay) window is
a deferred hardening pass.

## The gameplay channel

There's a channel reserved for the eventual 60 Hz gameplay firehose, and
rather than wait for ZoneManager's bridge to exist before finding out whether
routing on that channel even works, I proved it early with a diagnostic
ping/pong: the client sends an opaque random nonce, Sentinel echoes it back
verbatim. A nonce, not a timestamp, on purpose. The keep-alive already owns
liveness; this only proves routing, and a random nonce makes echo-by-luck
impossible.

The echo handler itself is temporary and gets replaced wholesale by the real
client-to-zone forwarding. But the routing, the packet pair, and the second
dispatcher registration it exercises are permanent, and they're proven.

## Characters

One character per account, on purpose. Accounts are free to make, so "I want
a second character" just means "make a second account." Each character is its
own `characters/{name}.json` file, name-keyed, so the directory listing *is*
the roster and two characters can't collide on a name by construction.

Creating a character happens over the wire, on the same TLS connection you
just authenticated on. You send a name, the server figures out which account
you are from the connection itself (it never trusts the packet for that), and
on success the connection closes so you re-authenticate and land the normal
token path, now that you actually own a character. A rejected name (invalid
or taken) keeps the connection open so you can just try again in place, and
the Godot client has now exercised that retry loop for real.

A startup reconciler in Core double-checks the account-to-character link is
intact and will self-heal anything additive, but it will never delete a
character or sever a real link on its own. A character file is about the
least replaceable thing on disk here.

## Persistence

Flat files, no database. Atomic writes (write to temp, fsync, rename), with a
write-back cache in front. And because a write-back cache can fail *after*
telling its caller "success," it owes everyone a failure story: flushes
return a report of exactly what failed, failed entries retry on the normal
cadence without clobbering newer writes, and the first failure of any entry
gets dumped to a `recovery/` folder in both binary and human-readable form so
nothing is silently at risk.

Everything hangs off one shared data root:

```
data/config/         runtime config, not committed
data/certs/          TLS cert
data/keys/           session signing key (LoginServer + Sentinel);
                     zone-registration keypair (ZoneManager generates, zones clone the seed)
data/accounts/       {id}.json
data/characters/     {name}.json
data/zones/{id}/     per-zone manifest + persistent state
data/logs/           per-process log files
data/recovery/       emergency dumps from failed flushes
```

## Solution layout

```
Stratum.slnx
├── SystemTools/    net10.0  — disk/log infra, accounts, characters, crypto, config
├── Shared/         net10.0  — wire packets, packet IDs, shared enums (Species, etc.)
├── Networking/     net10.0  — TCP + UDP transport, packet dispatcher
├── LoginServer/    net10.0  — auth + character-create exe
├── Sentinel/       net10.0  — UDP front door exe
├── Core/           net10.0  — operator console exe
├── ZoneManager/    net10.0  — zone registration hub exe (registry live; master clock in progress)
├── Zone/           net10.0  — per-zone simulation exe (registers + heartbeats; simulation in progress)
└── Probe/          net10.0  — standing regression tool (seven legs)
```

**Rule of thumb:** if the client would never need it, it doesn't belong in
`Shared`. `Shared` is the contract between client and server, nothing else.
Server-only contracts live server-side.

## Status

**Working, end to end (Probe-verified):**
- The full login flow: key auth, password auth, character creation over the
  wire, re-auth, UDP handoff, version check, keep-alive echo, and the
  gameplay-channel diagnostic echo. `Stratum.Probe` is now seven legs and
  it's the gate before any auth, wire, or `Shared` change gets committed.
- The Zone-to-ZoneManager registration round trip, previously listed here as
  "built, wire-verification pending." It's been run for real: signed marker,
  verification, admission, confirmation, eviction on disconnect. Green build
  wasn't verified; now the bytes are.
- The Godot client walking the whole chain live, including the in-place
  character-name retry the Probe never exercises.
- Core's account management, server launch/stop, and startup reconciler.
- Disk failure surfacing: flush reports, retry-without-clobber, recovery
  dumps.

**Not built yet:**
- Real gameplay packets. The channel is proven; the payloads aren't designed.
- ZoneManager's master clock, and Sentinel's client-to-zone bridge that
  replaces the diagnostic echo handler.
- The ECS, voxels, AI. The whole simulation side is still design notes.

## License

See [License.txt](License.txt).
