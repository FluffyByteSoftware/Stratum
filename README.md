# Stratum

A multiplayer RPG game server I'm writing in C#. Persistent shared voxel
world, somewhere around 25 players. It's a real project I intend to ship to
real people, but I'll be honest about the other half of it: this is also me
learning how all of this actually works — networking, crypto, process
architecture, simulation — by building it myself instead of grabbing a
framework that hides it from me.

That means some of the code in here is the second or third version of an
idea, because the first version taught me why it was wrong. I'm okay with
that. That's kind of the point. There's more on how I think about this in
[Philosophy.txt](Philosophy.txt).

## The stack

- **.NET 10** for the server processes
- **.NET Standard 2.1** for the shared library — it has to compile on .NET 10
  *and* under Unity's Mono/IL2CPP, and netstandard2.1 is the overlap
- **Unity 6** for the client (separate repo)
- **LiteNetLib** for UDP, and I also use its `NetDataWriter`/`NetDataReader`
  as general buffer tools
- **BouncyCastle** for Argon2id password hashing and Ed25519 keys
- **Newtonsoft.Json** for anything the client might ever read. System.Text.Json
  is allowed server-side only — IL2CPP strips STJ's reflection converters and
  I learned that the annoying way.

## How the server is shaped

It's a star. The client only ever talks to one process, zones never talk to
each other, and everything routes through the middle. Each box below is its
own executable:

- **LoginServer** — TLS/TCP, auth only. It does one job and then hangs up.
- **Core** — the operator console. Account management, launching the login
  server, admin tooling. This is *my* front door, not the player's.
- **ConnectionManager** — the only in-game process a client talks to. TCP +
  UDP per client; it translates between clients and zones. (Not built yet.)
- **ZoneManager** — master clock, cross-zone coordination, zone lifecycle.
  (Not built yet.)
- **Zones** — one process per zone, each one authoritative over its own
  simulation. (Not built yet.)

Why processes instead of threads in one big exe? Isolation, mostly. A zone
that crashes shouldn't take the login server with it, and forcing everything
through explicit message passing keeps me from cheating with shared state.

## Logging in

Auth has two paths that end in the same place: a session token.

The normal path is an **Ed25519 key**. The client signs a timestamp with its
private key, the server checks it, done — no password ever crosses the wire.
Keys expire after 3 days, hard.

The fallback is a **password**, verified against an Argon2id hash over TLS.
First login, new machine, lost key, expired key — same path. And here's the
part I like: a successful password login mints a *fresh* Ed25519 keypair on
the server and hands the private seed back to the client, once. So the
password path doesn't just log you in, it re-arms the key path. The system
heals itself back toward the good path every time you fall off it.

The key-path check order is deliberate: does the account exist → is the
timestamp fresh → is the signature valid → is the key expired. Expiry comes
*after* signature so a stranger can't probe which accounts exist by watching
which error they get. Failed password attempts lock the account for a minute
after three tries. Every auth failure is a disconnect.

There's no self-service registration. Accounts get created at the operator
console in Core, on purpose. At 25 players I'd rather know everyone who has
an account than build a signup flow.

## Saving things

Flat files. No database.

I went back and forth on this, but at this scale a database is solving a
problem I don't have. What I actually need is: writes that can't corrupt
(tmp file + fsync + rename, atomically), JSON I can open in a text editor
when something looks wrong, binary deltas for voxel edits, and append-only
logs for events. All of that is just files.

```
data/config/      runtime config — never committed
data/certs/       server certificate
data/keys/        session token key
data/accounts/    one JSON file per account
data/characters/  player characters
data/zones/       per-zone state
data/world/       world flags
data/logs/        server / admin logs, rolling at UTC midnight
```

Everything under `data/` is gitignored. Config is a runtime artifact that
ships itself from defaults on first run.

## Where it actually is right now

The full auth round trip is **passing**. There's a project in the solution
called `Probe` whose whole job is to be a paranoid fake client: it connects
over real TLS, logs in with a password, catches the freshly minted key seed,
disconnects, reconnects, and logs in again with the key — both paths, one
run, against real account files. Every time I touch the auth or wire code,
the Probe gets re-run. It's my regression test in exe form.

So as of now: logging, storage, the security primitives, accounts, the
operator console in Core (including launching the login server as a child
process), the LoginServer itself, and the Probe are all built and verified
end to end.

## What's next

1. The **UDP handshake** — the session token provably reaches the client
   now, so the next step is the packet that carries it over UDP and the
   session registry that validates it. I've deferred this one repeatedly.
   It's genuinely next this time.
2. **ConnectionManager**, then ZoneManager and the zone processes.
3. The simulation itself — hand-rolled ECS, 60 Hz fixed timestep, and an AI
   stack (utility AI out of combat, GOAP in combat) where NPCs only know
   what their senses actually tell them. No cheating with omniscient mobs.

## License

See [License.txt](License.txt).
