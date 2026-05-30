# Stratum

A C# multiplayer RPG game server for a persistent, shared voxel world,
targeting ~25 concurrent players. Stratum is both a real build and a
learning exercise: the codebase is grown one deliberate brick at a time,
with architectural decisions made explicit before code is written.

`Stratum.Driver` is the .NET solution that drives every server process —
login, connection management, zone supervision, and the individual zones —
from a single source tree.

## Design philosophy

Stratum favors **emergent consequence over simulation depth**. The world is
not a deep civilization sim; it is a set of lightweight, interacting systems
whose state persists and whose consequences are permanent:

- **Permanent consequences.** Extinct colonies stay extinct, dead named NPCs
  stay dead, and an alienated faction trainer stays alienated. State is the
  story.
- **No cheating NPCs.** Perception is fully simulated — field of view and
  line of sight through the voxel grid, sound with wall dampening, smell,
  touch. Agents act on decaying beliefs, never on ground truth. Stealth is an
  emergent property of the perception system, not a stat check.
- **Flat files, no database.** JSON for plaintext, binary for voxel dirty
  deltas, append-only logs for events. Everything is hand-editable and
  version-controllable. Authoring files are canonical; editors are optional
  layers over them.
- **Freeze/thaw by algebra, not replay.** Empty zones freeze; on thaw they
  catch up with elapsed-time math per system rather than replaying history.
  Ephemerals are discarded and regenerated from spawners; persistent state is
  preserved.

## Stack

| Layer | Target |
| --- | --- |
| Server processes | .NET 10 |
| Shared library | .NET Standard 2.1 (compiles on both server and Unity Mono/IL2CPP) |
| Client | Unity 6 |

Networking is UDP + TLS/TCP via [LiteNetLib](https://github.com/RevenantX/LiteNetLib).
Serialization is hand-written over LiteNetLib's `NetDataWriter`/`NetDataReader`
for packets, and Newtonsoft.Json for persisted records.

## Architecture

The server runs as a set of cooperating processes in a **star topology**.
Zones never talk to each other directly; all routing flows through the
ZoneManager. The client only ever talks to the ConnectionManager.

```
                         +---------------+
        TLS/TCP auth  →   |  LoginServer  |   issues short-lived session tokens
                         +---------------+
                                 │
        client ─ TCP/UDP ─►  +-------------------------+
                             |  ConnectionManager      |  sole client-facing
                             |  (Sentinel)             |  in-game process
                             +-------------------------+
                                 │
                             +---------------+
                             |  ZoneManager  |  master clock, cross-zone
                             +---------------+  coordinator, lifecycle supervisor
                              ╱      │      ╲
                        +------+ +------+ +------+
                        | Zone | | Zone | | Zone |   one process per zone,
                        +------+ +------+ +------+   authoritative simulation
```

- **LoginServer** — TLS/TCP authentication only. Two auth paths: Ed25519 key
  (returning players, frictionless) and password fallback (first run, lost or
  expired key, new machine). Issues 30-second HMAC session tokens.
- **ConnectionManager / Sentinel** — the only client-facing in-game process.
  TCP for session lifetime and reliable commands, UDP for realtime state.
  Validates session tokens and translates between clients and zones.
- **ZoneManager** — the master clock and zone lifecycle supervisor. Routes all
  cross-zone messaging.
- **Zone** — one process per zone, running the authoritative simulation on a
  fixed timestep with per-system cadences.

## Solution layout

| Project | Target | Role |
| --- | --- | --- |
| `Stratum.Driver` | .NET 10 | Server driver / host entry point |
| `Stratum.SystemTools` | .NET 10 | Logging, disk I/O, clock, security, account store |
| `Stratum.Shared` | .NET Standard 2.1 | Client/server contract surface: packets, channels, IDs |
| `Stratum.Networking` | .NET 10 | Server-side networking: dispatcher, TCP host, UDP host |
| `Stratum.LoginServer` | .NET 10 | Auth-only executable (TCP + TLS, port 9997) |
| `Stratum.Connection` | .NET 10 | ConnectionManager executable (TCP 9997 / UDP 9998) |

**Project placement rule:** if the Unity client wouldn't call it, it doesn't
belong in `Shared`. `Shared` is the contract surface between client and
server, nothing more. Server-only infrastructure lives in `SystemTools` or its
own server-side library.

## Status

Foundational infrastructure is being built bottom-up; game systems come after
the network and persistence layers are solid.

**Built**

- **SystemTools** — `Scribe` async logging (bounded channel, backpressure);
  `DiskManager` write-back cache with atomic writes (tmp + fsync + rename);
  `Heartbeat` fixed-timestep tick driver; security stack (self-signed cert
  provider, Ed25519 verify/generate, Argon2id password hashing, HMAC session
  tokens); account store (per-account JSON, in-memory cache).
- **Shared** — channel enum, packet ID scheme, `IPacketWritable` send-path
  marker, disconnect reasons, auth packets (key / password / response), and
  lifecycle packets (ping / pong / disconnect) with hand-written serialization.

**In progress**

- **Networking** — `PacketDispatcher` (freeze-then-dispatch lifecycle,
  monomorphized handler closures), `DispatchResult`/`DispatchOutcome`,
  `PacketFramer` (`[length:4B BE][typeId:4B BE][payload]`). TCP and UDP hosts
  next.

**Planned**

- LoginServer wiring and auth handlers; per-account lockout; admin CLI for
  account management.
- ConnectionManager; ZoneManager and per-zone processes; cross-zone messaging.
- ECS core (sparse-set storage); AgentManager (perception, utility AI, GOAP
  planning, blackboards).
- Blueprint loader (JSON with `extends`/`overrides`, FNV-1a hashed IDs); voxel
  system (0.5m cubes, 32³ chunks, lazy seed resolution); client patcher.

## Building

Requires the .NET 10 SDK. Open `Stratum.Driver.slnx` in Visual Studio, or
build from the command line:

```
dotnet build Stratum.Driver.slnx
```

## License

See `License.txt` for licensing information.
