# Stratum Core -- Project Instructions

(Paste this into the project's custom instructions.  Claude drafts a revised copy at the end of every session.)

## What we are building

Stratum Core is a game server written in Rust (RustRover).  It is the authority for a small-scale RPG set in a persistent voxel world.  "Core" is the driver: it starts up and launches everything else needed to get the game ready for play.  The game content itself will eventually be written in our own LPC-like scripting language, which Core will run.

Stratum is its own project.  It is not a port of anything.

## Where it lives

- The project is at `/opt/storage/stratum/dev/rustup/server`, on the dev machine's 4 TB WD mechanical drive.  The crate is named `server`, edition 2024.
- Source is in `src/`.  The project documents are in `ai/`: `STATUS.md`, `TODO.md`, `README.md`, `PROJECT_CORE.md`, `PROJECT_CORE_INSTRUCTIONS.md` (this file) and `WRITINGSTYLE.md`.  Use those exact names.
- Runtime files go under `/opt/stratum/content/`: logs in `logs/`, config files in `config/`.  That is on the mechanical drive too, which is why a synced save costs about 40 ms there.

## Named pieces

- **Scribe** -- logging.  Built and working (`src/scribe.rs`).  Callable from anywhere as `scribe::info(Channel::Core, "...")`, prints to the terminal in color, appends to a log file that starts fresh at 00:00Z or on a size limit.  Messages carry a priority (Debug, Info, Warn, Error) and a channel (World, Core, Tools, NetTcp, NetUdp, Security), and every line ends with the caller's file and line, like `(src/main.rs:31)`.  Any new public function that logs on someone else's behalf needs `#[track_caller]`, and so does every function between it and Scribe, or the location will point at the wrong place.  Scribe starts on built-in defaults and gets its real settings through `scribe::initialize(ScribeConfig { ... })`.  Scribe does not know Constellations exists -- the glue is `initialize_scribe()` in main.rs.  Keep it that way.  Standard library only.
- **Constellations** -- configuration.  Built and working (`src/constellations.rs`).  `constellations::load()` reads `/opt/stratum/content/config/stratum.conf` (our own `KEY=VALUE` format, keys in capitals with underscores) or generates it, `constellations::get()` hands back a copy of every setting, and `constellations::save()` writes them back at shutdown.  It reads and writes through DiskMan (`read_text()` and the direct `write_file()`) and never touches the disk itself.  A bad value warns through Scribe and keeps its default.  Adding a setting means touching four places in the file: `Settings`, `default_settings()`, `apply_setting()` and `file_text()`.  It never holds its lock while it talks to Scribe or the disk.  Standard library only.
- **DiskMan** -- the Disk Manager.  Built and working (`src/diskman.rs`).  The one place the server goes through to replace a whole file and to read one back.  A file is a `StratumFile` (a `path` and its `contents` as bytes).  Every write is temp file, `sync_all()`, rename, sync the folder.  `write_file()` does that right now and returns an `io::Result`.  `write_later()` puts the file in a cache and returns at once; a writer thread drains the cache continuously in batches (8 threads, one folder sync per folder).  The cache keeps the newest copy per path, holds 100 files or 2 GB, makes saves wait when full, and logs and drops a file it can't write.  Reads check the cache first.  `diskman::start()` comes right after Scribe and `diskman::stop()` is the last thing main() does.  Never use `write_file()` and `write_later()` on the same path.  Scribe stays out of DiskMan, because Scribe appends, and because DiskMan logs through Scribe.  Standard library only.

## Decisions made

- Language and IDE: Rust, RustRover.  Development on Linux (Nobara).
- Client: Godot.
- Network: UDP for game traffic, TCP for authentication.
- Scale: 50 players at peak.
- World: chunked into zones, procedurally generated.
- Persistence: flat files in the LPC tradition.  No database.
- Atomic saves: write to a temp file, then rename over the old one.  Anything that replaces a whole file goes through DiskMan.
- Crashes: players get rolled back to their last save that reached the disk.  Nothing is ever left half-written.
- Time: all time is UTC.  Any time we display ends in Z.
- Dependencies: as few as possible.  Zero so far.
- Startup order: Scribe, DiskMan, Constellations, initialize Scribe, everything else.  Shutdown: `constellations::save()`, then `diskman::stop()` last.
- Line width: source lines stay inside 120 columns (RustRover's hard wrap).  Comments still wrap at 78, and code gets wrapped wherever that reads better.  The rule is in WRITINGSTYLE.md.

## Proposed, not yet confirmed

- Fixed-tick simulation loop with networking running alongside it.

## Open decisions

- Concurrency model (async runtime vs. plain threads).  DiskMan's writer is a plain thread, and that holds either way.
- How the server gets stopped once there is a main loop.  `constellations::save()` and `diskman::stop()` both depend on it.
- On-disk file format for saves and zones.
- Where hot save files live (mechanical drive or NVMe).
- Script language design, the VM, and whether scripts hot-reload the way LPC objects do.

## Who Claude is working with

Jacob is the sole developer.  He is a hobbyist, knows C and C#, and is new to Rust -- he can read some of it but gets lost in the syntax.  He often describes what he wants as the C# he would have written, or the C# he already built (the DiskMan cache came straight from his C# DiskManager).  So:

- Explain Rust in chat plainly and keep it short.  Do not translate things into C terms unless Jacob asks for that.
- Prefer the simple, readable version of the code over the clever one.  No macro tricks, no generics gymnastics, no lifetimes unless there is no way around them -- and when there isn't, explain why.  Plain functions that take the struct as the first argument are fine; we don't need `impl` blocks yet.
- Don't add a crate without saying what it is for and what the alternative would be.  Keep dependencies few.  When Jacob asks whether something already exists, look properly -- he asked this session, and the honest answer ("no crate does this; an embedded database does, and it costs us flat files") shaped the decision.
- When a performance question comes up, measure before designing.  The DiskMan benchmark turned a guess ("4 seconds") into numbers (1.9 seconds, then 0.5), and the design came out of them.
- When Claude makes a judgment call Jacob didn't ask for, list it so he can overrule it.  He answers the things he disagrees with and stays quiet on the rest.  If a question matters and he skipped it, ask it again -- staying quiet on a question is not an answer.
- Jacob runs delivered files past a local LLM and takes some of its syntax suggestions.  His copy is the master copy.  Match what he settled on when writing new code: the lock is taken with `.lock().unwrap_or_else(|poisoned| poisoned.into_inner())`, tests use `assert_eq!` where the values can be printed, and long message strings are split with a `\` at the end of the line.

## How a session runs

**Start.**  Jacob uploads the current STATUS.md, README.md, PROJECT_CORE.md, TODO.md and WRITINGSTYLE.md, and the source files the session is going to touch or lean on.  Read them before doing anything else.  If one is missing, ask for it.  (In Session 2 Claude built against a stand-in for a file it didn't have.  It worked, but it is the wrong way round.)

**Middle.**  We go one file at a time.  Each file is delivered complete and compilable.  Claude compiles and runs a file before handing it over whenever it has a compiler to do that with, and says what it tested -- and what it didn't.  The sandbox's default image has no Rust, but Ubuntu's own packages have new enough versions: `apt-get install rustc-1.91 cargo-1.91` and link them into `/usr/local/bin`.  That gives a real edition 2024 build.  Test anything to do with permissions as an ordinary user, not root, because root ignores them.  Anything a file needs from a file that doesn't exist yet gets stubbed, marked `// TODO(topic): ...`, and logged in TODO.md.

Tests must not log and must not touch the global state (Scribe's, Constellations' or DiskMan's).  A test that logs writes into Jacob's real log folder, and tests run at the same time as each other.  Design code so its logic can be tested with a struct handed in, the way the config parser and DiskMan's cache functions are.

Jacob edits delivered files by hand.  So before a delivered file replaces one that already exists, ask him to paste his current copy and remind him to commit first -- whole-file replacement is exactly how code gets written over.  When a change is only a few lines, give him the lines to add by hand and leave the file alone.  Hand edits get missed (Session 3's `main.rs` came back without them), so check for them at the end.

Before delivering a source file, check it for lines wider than the limit and for characters that aren't plain ASCII.

**End.**  Jacob sends the master copies of the source files.  Build them, run the tests, and compare them with what was delivered, so the documents describe the code that is actually on his disk.  Read the compiler warnings -- twice now they have caught a shutdown call that was never made.  Don't change code at hand-off unless there is a glaring bug.  Then generate updated copies of whichever of these changed, as complete files.  Skip the ones that didn't:

1. STATUS.md -- improved from the last version, with this session's progress.
2. TODO.md -- deferred work, plus a running tab of ideas that came up.  Every `// TODO(topic):` in the code has a line here.
3. README.md -- the Git repo readme.
4. PROJECT_CORE.md -- the skeletal layout of the whole Core project.
5. PROJECT_CORE_INSTRUCTIONS.md -- this file, revised.
6. WRITINGSTYLE.md -- only if something was learned about Jacob's voice this session, or something in it has gone stale.

## Writing

Everything publicly visible -- the project documents and every comment in every source file -- follows WRITINGSTYLE.md, so that it keeps Jacob's cadence, tone and voice.  That includes the text the server writes for people to read: log messages, and the comments inside a generated config file.  Chat is exempt.  In chat, explain as much as the thing needs and no more.