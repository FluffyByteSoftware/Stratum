# Stratum

A custom C# multiplayer RPG game server for a persistent, shared voxel world.
Targets ~25 concurrent players. Built as both a real project and a deliberate
learning exercise.

## Stack

- **.NET 10** — server processes
- **.NET Standard 2.1** — shared library (compiles on .NET 10 *and* Unity Mono/IL2CPP)
- **Unity 6** — client
- **LiteNetLib 2.1.4** — UDP transport and general-purpose buffer utilities
- **BouncyCastle** — Argon2id password hashing, Ed25519 primitives
- **Newtonsoft.Json** — all shared/client-read JSON
- **System.Text.Json** — server-only types Unity never deserializes

## Architecture

A star topology. The client only ever talks to Sentinel; zones never talk to each
other directly — all routing flows through the ZoneManager. Each process is its own
executable.

- **LoginServer** — TLS/TCP auth only. Two auth paths (Ed25519 key, Argon2id password
  fallback); issues short-lived session tokens and advertises Sentinel's UDP endpoint.
- **Sentinel** — UDP front door. Validates TCP-issued session tokens on connection,
  tracks authenticated sessions, enforces one session per account. Eventually routes
  between clients and zones via ZoneManager.
- **Core** — operator console. Account management, server launch (starts and stops
  LoginServer and Sentinel together).
- **ZoneManager** — master clock, cross-zone coordinator, zone lifecycle supervisor
  *(not yet built)*.
- **Zones** — one process per zone, authoritative simulation *(not yet built)*.

## Solution layout

```
Stratum.slnx
├── SystemTools/    .NET 10      — Scribe, DiskManager, ConfigStore,
│                                  CertificateProvider, Ed25519Verifier/KeyGenerator,
│                                  PasswordHasher, SessionKeyProvider,
│                                  SessionTokenIssuer, AccountRecord/Store
├── Shared/         .NETStd2.1  — packet structs, packet IDs, IPacketWritable,
│                                  SecureDisconnectReason, GameProtocolVersion
├── Networking/     .NET 10      — PacketDispatcher, TcpHost/TcpConnection,
│                                  UdpHost/UdpConnection
├── LoginServer/    .NET 10      — auth-only exe (TLS/TCP)
├── Sentinel/       .NET 10      — UDP front door exe
├── Core/           .NET 10      — operator console exe
└── Probe/          .NET 10      — auth round-trip regression tool (all four legs)
```

**Placement rule:** if the Unity client wouldn't call it, it doesn't belong in
`Shared`. Shared is the wire contract between client and server. Server-only
infrastructure lives in `SystemTools` or its own server-side library.

## Authentication

Fully verified end to end across all four legs of `Stratum.Probe`.

### TCP auth (LoginServer)

Two paths, same outcome — a session token and Sentinel's UDP endpoint.

- **Key path (returning player):** client signs an 8-byte big-endian Unix-ms
  timestamp with its Ed25519 private key. Check order: exists → timestamp within
  ±30s → signature → key age within 3 days.
- **Password path (first run / expired key / new machine):** verified against a
  stored Argon2id hash over TLS. On success the server rotates in a fresh Ed25519
  keypair, persists it, then returns the new private seed to the client. Subsequent
  logins use the key path.

Session tokens are stateless HMAC-SHA256 with a 30-second lifetime. Account
creation is admin-only via Core — no self-service registration.

### UDP auth (Sentinel)

After TCP auth the client connects to Sentinel over UDP, presenting the session
token as LiteNetLib connection-request data. Sentinel validates the token, enforces
one session per account, and acknowledges admission. No typed packet is exchanged
during the handshake itself — the connection request *is* the auth request.

### Protocol version check

Immediately after admission, Sentinel sends a version challenge carrying the
server's current protocol version string. The client responds with its own version;
Sentinel replies with `Ok` (session proceeds) or `Mismatch` (disconnect). Stale or
out-of-date clients are rejected here before any gameplay traffic flows.

## Logging

Each server process writes to its own log files — `server_{process}_{date}.log`,
`admin_{process}_{date}.log` — resolved at startup from the process name. This
avoids cross-process file contention: `File.AppendAllText` opens with
`FileShare.Read` and does not tolerate concurrent writers. Single-writer files make
the append path correct with no locking required.

## Persistence

Flat files, no database. Atomic writes (tmp + fsync + rename). All processes share
a single data root (`E:\Stratum\data` in the dev environment).

```
data/config/        runtime config (never committed)
data/certs/         server.pfx
data/keys/          session_token.key  (shared by LoginServer and Sentinel)
data/accounts/      {id}.json
data/logs/          server_{proc}_{date}.log, admin_{proc}_{date}.log
```

`data/` is gitignored; config files are generated as first-run defaults.

## Status

### Verified and working

- **Full auth round trip** — all four legs of `Stratum.Probe` pass: password auth →
  key auth → UDP session auth → protocol version check.
- **LoginServer** — boots, binds TLS/TCP, runs both auth paths, tears down cleanly.
  Advertises Sentinel's UDP endpoint from config.
- **Sentinel** — UDP front door: validates tokens, tracks sessions, sends auth ack,
  runs the version-check exchange.
- **Core** — account management (create, list, reset, delete); item 1 launches and
  stops both LoginServer and Sentinel together.
- **Networking layer** — `TcpHost`, `TcpConnection`, `UdpHost`, `UdpConnection`,
  `PacketDispatcher<TConnection>`.
- **Logging** — per-process log filenames; concurrent cross-process append issue
  resolved.
- **Probe** — standing regression tool; re-run whenever auth, wire code, or `Shared`
  packets change.

### Not yet built

ConnectionManager translation layer (Sentinel routing client↔zone), ZoneManager,
Zone processes, ECS core, AI (perception, utility AI, GOAP), blueprint loader,
voxel system, client-side simulation library.

## License

See [License.txt](License.txt).
