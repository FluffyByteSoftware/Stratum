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
process is its own executable.

- **LoginServer** — TLS/TCP auth only. Two auth paths (Ed25519 key, password
  fallback); issues short-lived session tokens.
- **ConnectionManager / Sentinel** — TCP + UDP; the sole client-facing in-game
  process; translates between clients and zones.
- **ZoneManager** — master clock, cross-zone coordinator, zone lifecycle supervisor.
- **Zones** — one process per zone, authoritative simulation.

## Solution layout

```
Stratum.sln
├── Stratum.SystemTools/   .NET 10     — Scribe, DiskManager, ConfigStore, Heartbeat,
│                                         CertificateProvider, Ed25519Verifier/KeyGenerator,
│                                         PasswordHasher, SessionKeyProvider,
│                                         SessionTokenIssuer, AccountRecord/Store
├── Stratum.Shared/        .NETStd2.1  — packet defs, channels, IDs, disconnect reasons,
│                                         IPacketWritable
├── Stratum.Networking/    .NET 10     — dispatcher, TcpHost, UdpHost
├── Stratum.LoginServer/   .NET 10     — auth-only exe (TCP+TLS, no UDP)
├── Stratum.Connection/    .NET 10     — ConnectionManager exe (TCP / UDP)
└── Stratum.AdminTools/    .NET 10     — CLI account management (not yet built)
```

**Placement rule:** if the Unity client wouldn't call it, it doesn't belong in
`Shared`. Shared is the contract surface between client and server, nothing more.
Server-only infrastructure lives in `SystemTools` or its own server-side library.

## Authentication

Two paths, same outcome (session token + UDP endpoint).

- **Key path (returning player):** client signs a Unix-ms timestamp with its Ed25519
  private key. Check order is fixed: exists → timestamp within 30s → signature →
  key age within 3 days. A stale key is rejected and the client falls back to
  password.
- **Password path (first run / lost / expired / new machine):** verified against a
  stored Argon2id hash over TLS. On success the server rotates in a fresh Ed25519
  keypair, persists it, then returns the new private seed once.

Account creation is **admin-only** (`Stratum.AdminTools`) — no self-service
registration. Session tokens are stateless HMAC-SHA256 with a 30s lifetime.

## Persistence

Flat files, no database. Atomic writes (tmp + fsync + rename). Accounts are global;
characters are per-shard.

```
data/config/        runtime config (never committed)
data/certs/         server.pfx, server.cer
data/keys/          session_token.key
data/accounts/      {id}_account.json
data/shards/        per-shard characters, zones, items, voxels, events
data/logs/          server_{date}.log
```

`data/` is gitignored; config ships as first-run artifacts from record defaults.

## Status

**Built:** logging (Scribe), storage (DiskManager, ConfigStore), config records,
Heartbeat, security primitives (certs, Ed25519, Argon2id, session tokens), accounts,
shared packet/channel definitions, the networking layer (dispatcher, TCP/UDP hosts),
and the full LoginServer near-term slice — it boots, binds, runs both auth paths,
and tears down cleanly.

**Next:** `Stratum.AdminTools` (account creation CLI), then a full auth round-trip
test.

**Later:** ConnectionManager, ZoneManager and zone processes, ECS core, AI
(perception, utility AI, GOAP), the blueprint loader, and the voxel system.

## License

See [License.txt](License.txt).
