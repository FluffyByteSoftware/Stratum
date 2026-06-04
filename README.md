# Stratum

A custom C# multiplayer RPG game server for a persistent, shared voxel world.
Targets ~25 concurrent players. Built as both a real project and a deliberate
learning exercise.

## Stack

- **.NET 10** — server processes
- **.NET Standard 2.1** — shared library (compiles on .NET 10 *and* Unity
  Mono/IL2CPP)
- **Unity 6** — client
- **LiteNetLib** — UDP networking
- **BouncyCastle** — Argon2id password hashing, Ed25519 primitives

## Architecture

A star topology. The client only ever talks to the ConnectionManager; zones never
talk to each other directly — all routing flows through the ZoneManager. Each
server process is its own executable.

- **LoginServer** — TLS/TCP auth only. Two auth paths (Ed25519 key, password
  fallback); issues short-lived session tokens.
- **ConnectionManager / Sentinel** — TCP + UDP; the sole client-facing in-game
  process; translates between clients and zones.
- **ZoneManager** — master clock, cross-zone coordinator, zone lifecycle supervisor.
- **Zones** — one process per zone, authoritative simulation.
- **Core** — operator console. The local control surface for running the server
  and managing accounts; not client-facing.

## Solution layout

```
Stratum.slnx
├── SystemTools/   .NET 10     — Scribe, AdminToolLog, DiskManager, ConfigStore,
│                                Heartbeat, CertificateProvider,
│                                Ed25519Verifier/KeyGenerator, PasswordHasher,
│                                SessionKeyProvider, SessionTokenIssuer,
│                                AccountRecord/Store, AccountManager, ConsoleInput
├── Shared/        .NETStd2.1  — packet defs, channels, IDs, disconnect reasons,
│                                IPacketWritable
├── Networking/    .NET 10     — dispatcher, TcpHost, UdpHost
├── LoginServer/   .NET 10     — auth-only exe (TCP+TLS, no UDP)
├── Connection/    .NET 10     — ConnectionManager exe (TCP / UDP; not yet built)
└── Core/          .NET 10     — operator console (server control + account mgmt)
```

**Placement rule:** if the Unity client wouldn't call it, it doesn't belong in
`Shared`. Shared is the contract surface between client and server, nothing more.
Server-only infrastructure lives in `SystemTools` or its own server-side library.

## Authentication

Two paths, same outcome (session token + UDP endpoint).

- **Key path (returning player):** client signs a Unix-ms timestamp with its
  Ed25519 private key. Check order is fixed: exists → timestamp within 30s →
  signature → key age within 3 days. A stale key is rejected and the client falls
  back to password.
- **Password path (first run / lost / expired / new machine):** verified against a
  stored Argon2id hash over TLS. On success the server rotates in a fresh Ed25519
  keypair, persists it, then returns the new private seed once.

Account creation is **admin-only** — there is no self-service registration.
Accounts are managed from the **Core** operator console (create / reset / delete /
list); every action is written to a dedicated audit log. Session tokens are
stateless HMAC-SHA256 with a 30s lifetime.

## Logging

A single `DiskManager` owns all disk I/O and fans log output to multiple
daily-rolling files (rolling at UTC midnight), keyed by a `LogFile` enum. Three
independent facades write through it:

- **Scribe** → `server_{date}.log` — severity-tagged runtime + exceptions, with
  caller context and color-coded console output.
- **AdminToolLog** → `admin_{date}.log` — account-management audit trail; records
  every action, success and failure.
- **SimulationLog** → `simulation_{date}.log` — reserved for non-network game
  logic (not yet active).

All log timestamps are UTC, so filenames and line stamps agree.

## Persistence

Flat files, no database. Atomic writes (tmp + fsync + rename). Accounts are global;
characters are per-shard.

```
data/config/        runtime config (never committed)
data/certs/         server.pfx, server.cer
data/keys/          session_token.key
data/accounts/      {id}_account.json
data/shards/        per-shard characters, zones, items, voxels, events
data/logs/          server_{date}.log, admin_{date}.log
```

`data/` is gitignored; config ships as first-run artifacts from record defaults.

## Status

**Built:** logging (Scribe + multi-sink DiskManager + admin audit log), storage
(DiskManager, ConfigStore), config records, Heartbeat, security primitives (certs,
Ed25519, Argon2id, session tokens), account management (`AccountManager` + the Core
operator console), shared packet/channel definitions, the networking layer
(dispatcher, TCP/UDP hosts), and the full LoginServer near-term slice — it boots,
binds, runs both auth paths, and tears down cleanly.

**Next:** a full auth round-trip test — create an account in Core, then
authenticate against LoginServer across both key and password paths.

**Later:** ConnectionManager, ZoneManager and zone processes, Core server-control
and resource-monitoring features, ECS core, AI (perception, utility AI, GOAP), the
blueprint loader, and the voxel system.

## License

See [License.txt](License.txt).
