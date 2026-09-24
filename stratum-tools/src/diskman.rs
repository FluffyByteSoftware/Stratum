//! File:     stratum-tools/src/diskman.rs
//! Project:  Stratum Core
//! Author:   Jacob Chacko
//!
//! DiskMan, the Disk Manager.  The one place the server goes through to
//! replace a whole file, to read one back, to move one, to delete one, and
//! to make or delete a folder.  A file is a path and its contents as bytes
//! (a StratumFile).
//!
//! We never write a file in place.  We write `path.tmp`, force it onto the
//! disk, and then rename it over `path`.  If the server dies halfway through,
//! or the power goes out, the old file is still there.
//!
//! There are two ways to save a file:
//!
//!   - `write_file()` does it right now and doesn't come back until the file
//!     is safe on the disk.  About 40 ms on the dev machine's spinning drive.
//!     For the few callers that need to know it worked (the config file).
//!   - `write_later()` drops the file into a cache and comes back straight
//!     away.  A background thread (the "writer") takes everything in the
//!     cache and writes it, over and over, for as long as there is anything
//!     there.  This is for player saves and everything else the game can't
//!     afford to wait 40 ms on.
//!
//! The cache keeps one copy per path, the newest.  Save a player twice
//! before the writer gets to them and only the second save is written.  The
//! writer takes the whole cache at once and writes it with several threads,
//! then syncs each folder once for the lot -- on the dev machine that took 50
//! saves from 1.9 seconds down to about 0.5.
//!
//! Reads check the cache before the disk, so a file that is still waiting to
//! be written reads back as its newest version.
//!
//! The deal we took: a crash loses whatever was still in the cache, and
//! players get rolled back to their last save that made it to the disk.  It
//! never leaves half a file.  And a file the writer can't write is logged
//! and dropped, so a broken disk costs saves but never stalls the game.
//!
//! When the cache is full (MAX_CACHE_FILES or MAX_CACHE_BYTES), write_later()
//! waits until the writer makes room.  Player saves, one copy per path,
//! should almost never fill it.  The world's chunks can: tick-sim saved one
//! file per chunk on the spinning drive, filled the cache for its whole run,
//! and piled about 4 GiB of work up behind it.  So the game's tick never
//! calls write_later() for anything in bulk.  It hands its saves to a thread
//! of its own, and that thread calls write_later().
//!
//! The writer goes as fast as the drive lets it, and on a fast drive that
//! is its own problem.  A copy only gets replaced while it is waiting, and
//! on the NVMe almost nothing waits: tick-sim wrote 58 GiB there in one
//! minute, which would wear out an SSD in weeks if the server ever wrote
//! like that.  How often a file gets saved is up to whoever saves it, until
//! DiskMan has a minimum time between writes of the same path.
//!
//! Anything that replaces a whole file comes through here.  Scribe's log
//! files are the one exception -- Scribe appends, which is a different job,
//! and DiskMan logs through Scribe, so Scribe can't lean on DiskMan.
//!
//! A read of a file that isn't there is not logged, because for plenty of
//! callers that is normal (the first run, with no config file yet).
//!
//! A crash between writing `path.tmp` and the rename leaves the `.tmp`
//! behind.  It does no harm (the next save of that file wipes it), and
//! `clear_leftovers()` sweeps them up at launch, before anything saves.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::scribe::{self, Channel};

// ---------------------------------------------------------------------------
// The numbers
// ---------------------------------------------------------------------------

/// The most files the cache holds before write_later() starts waiting.
const MAX_CACHE_FILES: usize = 100;

/// The most bytes the cache holds before write_later() starts waiting.  2 GB.
const MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// How many threads the writer uses on a batch.  The benchmark on the dev
/// machine's spinning drive got better up to 8 and was a coin toss after
/// that.  A different drive may want a different number.
const WRITER_THREADS: usize = 8;

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

/// A file as DiskMan sees it: where it goes, and what is in it.
// Rust note: `Debug` is what lets a failing test print one.
#[derive(Debug)]
pub struct StratumFile {
    pub path: PathBuf,
    // Rust note: `Vec<u8>` is a growable array of bytes.  Text goes in with
    // `.into_bytes()` on a String, which hands over the bytes without
    // copying them.
    pub contents: Vec<u8>,
}

// ---------------------------------------------------------------------------
// The cache
// ---------------------------------------------------------------------------

/// One file waiting in the cache.
struct Pending {
    // Rust note: `Arc` is a pointer with a count of who is holding it.
    // Copying an Arc copies the pointer and not the bytes behind it, and the
    // bytes are freed when the last holder lets go.  That is what lets the
    // writer take the whole cache without copying 2 GB of it.
    contents: Arc<Vec<u8>>,
    /// Goes up by one every time anything is saved.  The writer uses it to
    /// tell whether the file it just wrote is still the newest copy.
    version: u64,
}

/// Where the writer thread is in its life.
#[derive(Clone, Copy, PartialEq)]
enum Writer {
    /// Nobody has called start() or write_later() yet.
    NotStarted,
    Running,
    /// stop() has run, or the thread couldn't be started.  From here on
    /// write_later() writes straight to the disk, the same as write_file().
    Stopped,
}

/// DiskMan's working state.  There is exactly one of these, in CACHE below.
struct Cache {
    /// The files waiting to be written, newest copy only, by path.
    files: HashMap<PathBuf, Pending>,
    /// All the bytes in `files`, added up, so the size limit doesn't have to
    /// count them every time.
    bytes: u64,
    next_version: u64,
    writer: Writer,
    /// Set by stop().  The writer finishes what is in the cache and quits.
    stopping: bool,
    /// Set when we have said the cache is full.  Cleared once it has emptied
    /// out completely, so a stall gets one Warn and not one per waiting save.
    said_full: bool,
}

fn new_cache() -> Cache {
    Cache {
        files: HashMap::new(),
        bytes: 0,
        next_version: 1,
        writer: Writer::NotStarted,
        stopping: false,
        said_full: false,
    }
}

// Same arrangement as Scribe and Constellations: a file-scope global behind a
// lock.  `None` means nothing has touched DiskMan yet.
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

// Rust note: a Condvar is how a thread sleeps until another thread says
// "something changed".  `wait()` lets go of the lock, sleeps, and takes the
// lock back before it returns.  We use one for everything: the writer
// sleeping on an empty cache, write_later() waiting on a full one, and
// flush() waiting on the cache to empty.  Whoever changes the cache wakes
// everybody up with `notify_all()`, and each one checks whether it was the
// change they were waiting for.
static CACHE_CHANGED: Condvar = Condvar::new();

/// The writer thread, so stop() can wait for it to finish.
// Rust note: a JoinHandle is the thread's receipt.  `join()` on it waits
// until the thread has finished.
static WRITER_THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Starting and stopping
// ---------------------------------------------------------------------------

/// Starts the writer thread.  main() calls this right after Scribe is up.
///
/// The first write_later() would start it anyway.  Calling this at launch
/// means a thread that won't start gets reported at launch and not with the
/// first player save.
pub fn start() {
    let mut guard = CACHE.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache = guard.get_or_insert_with(new_cache);

    if cache.writer != Writer::NotStarted {
        return;
    }
    cache.writer = Writer::Running;
    drop(guard);

    let spawned = thread::Builder::new()
        .name("diskman".to_string())
        .spawn(writer_loop);

    match spawned {
        Ok(handle) => {
            let mut writer_thread = WRITER_THREAD.lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *writer_thread = Some(handle);
        }
        Err(error) => {
            // Without the writer nothing would ever leave the cache.  So
            // from here on every save gets written on the spot.  Slow, but
            // nothing is lost.
            let mut guard = CACHE.lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(cache) = guard.as_mut() {
                cache.writer = Writer::Stopped;
            }
            drop(guard);
            scribe::error(Channel::Core,
                          &format!("DiskMan can't start its writer thread ({}).  \
                          Every save gets written on the spot instead.", error));
        }
    }
}

/// Writes everything still in the cache, waits for it to finish, and stops
/// the writer.  main() calls this as the very last thing on the way out.
///
/// Any write_later() after this writes straight to the disk, so a straggler
/// still gets saved.
pub fn stop() {
    let mut guard = CACHE.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache = guard.get_or_insert_with(new_cache);

    if cache.writer != Writer::Running {
        cache.writer = Writer::Stopped;
        return;
    }
    cache.stopping = true;
    let waiting = cache.files.len();
    drop(guard);
    CACHE_CHANGED.notify_all();

    if waiting > 0 {
        scribe::info(Channel::Core,
                     &format!("DiskMan is writing the {} file(s) it is still \
                     holding.", waiting));
    }

    let handle = WRITER_THREAD.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    if let Some(handle) = handle {
        if handle.join().is_err() {
            scribe::error(Channel::Core,
                          "DiskMan's writer thread crashed.  Anything it was \
                          still holding is lost.");
        }
    }

    let mut guard = CACHE.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(cache) = guard.as_mut() {
        cache.writer = Writer::Stopped;
    }
}

/// Waits until everything in the cache has been written (or dropped).  The
/// server keeps running and the writer keeps going.  stratum-game calls it
/// before it looks for a new player file on the disk, and before it deletes
/// one.  An admin "save everything now" would call it too.
pub fn flush() {
    let mut guard = CACHE.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    loop {
        let cache = guard.get_or_insert_with(new_cache);
        if cache.files.is_empty() || cache.writer != Writer::Running {
            return;
        }
        guard = CACHE_CHANGED.wait(guard)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Hands a file to the cache and comes straight back.  The writer thread
/// writes it as soon as it can.  If the same path is already waiting, this
/// copy replaces it.
/// If the cache is full this waits until the writer has made room.  If the
/// writer is stopped (or never started) the file gets written on the spot.
/// So the game's tick never calls this for anything in bulk (see the top of
/// the file).
///
/// Don't use this and write_file() on the same path.  A write_file() could
/// land first and then get written over by an older copy from the cache, so
/// write_file() refuses a path that is still waiting in here.
// Rust note: this takes the StratumFile itself and not a `&` to it, so the
// bytes move into the cache without being copied.  The caller can't use the
// file after handing it over, and the compiler will say so if they try.
// TODO(save-rate): a minimum time between writes of the same path, so a
// fast drive doesn't write the same file many times a second.  Jacob's
// call, from tick-sim: DiskMan decides, not the caller.  Session 18.
#[track_caller]
pub fn write_later(file: StratumFile) {
    let mut guard = CACHE.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if guard.get_or_insert_with(new_cache).writer == Writer::NotStarted {
        drop(guard);
        start();
        guard = CACHE.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }

    loop {
        let cache = guard.get_or_insert_with(new_cache);

        if cache.writer != Writer::Running {
            drop(guard);
            let _ = write_file(&file);
            return;
        }

        if cache_has_room(cache, &file.path, file.contents.len() as u64) {
            cache_insert(cache, file);
            drop(guard);
            CACHE_CHANGED.notify_all();
            return;
        }

        // Full.  Say so once per stall (without holding our lock while we
        // talk to Scribe), then wait for the writer to make room.
        if !cache.said_full {
            cache.said_full = true;
            drop(guard);
            scribe::warn(Channel::Core,
                         "DiskMan's cache is full.  Waiting on the disk.");
            guard = CACHE.lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            continue;
        }
        guard = CACHE_CHANGED.wait(guard)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

/// Replaces `file.path` with `file.contents` right now, and doesn't come back
/// until it is safe on the disk.  Makes the folder if it isn't there.
///
/// The steps, in order: write the whole thing to `path.tmp`, force it onto
/// the disk, rename it over `path`, then force the folder onto the disk.  The
/// old file isn't touched until the new one is complete and on the disk, so
/// there is no moment where a crash leaves us with half a file.
///
/// A `.tmp` left behind by an earlier crash doesn't matter.  Creating the
/// temp file wipes whatever was in it.
///
/// A path still waiting in the cache from a write_later() is refused, and
/// logged.  The cached copy is older than this one, and the writer would put
/// it on top of this one when it got to it.  That is the one way mixing the
/// two kinds of write goes wrong, so it's the one that gets caught.
// Rust note: `#[track_caller]` here passes the caller's location on to
// Scribe, so an error line says `(stratum-tools/src/constellations.rs:253)` and not
// somewhere in this file.  Every function between the caller and Scribe has
// to carry it, which is why complain() does too.
#[track_caller]
pub fn write_file(file: &StratumFile) -> io::Result<()> {
    let path = &file.path;

    // A path with no file name on the end (like `/`) has nothing to rename
    // over.  Better to say so than to let the rename fail in a stranger way.
    if path.file_name().is_none() {
        let error = io::Error::new(io::ErrorKind::InvalidInput,
                                   "that path has no file name on the end");
        return Err(complain(&format!("DiskMan can't write {}", path.display()),
                            error));
    }

    if cached_copy(path).is_some() {
        let error = io::Error::new(io::ErrorKind::InvalidInput,
                                   "an older copy from write_later() is still waiting in the cache");
        return Err(complain(&format!("DiskMan won't write {}", path.display()), error));
    }

    let folder = folder_of(path);
    let temp_path = temp_path_for(path);

    // Rust note: `if let Err(error) = ...` runs the block only when the call
    // failed, and hands us the error.  A match with an empty Ok arm, shorter.
    if let Err(error) = fs::create_dir_all(&folder) {
        return Err(complain(&format!("DiskMan can't make the folder {}",
                                     folder.display()),
                            error));
    }

    if let Err(error) = write_and_sync(&temp_path, &file.contents) {
        // Whatever made it into the temp file is no use to anybody, and if
        // the disk is full it is taking up room.  If this fails too there is
        // nothing more to do about it.
        let _ = fs::remove_file(&temp_path);
        return Err(complain(&format!("DiskMan can't write {}",
                                     temp_path.display()),
                            error));
    }

    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(complain(&format!("DiskMan can't rename {} over {}",
                                     temp_path.display(), path.display()),
                            error));
    }

    // The rename only really happens on the disk once the folder is written
    // out, because a folder is just a list of names.  By now the new file
    // is in place and the old one is gone either way, so if this step fails
    // it is a Warn and the save still counts.
    if let Err(error) = sync_folder(&folder) {
        scribe::warn(Channel::Core,
                     &format!("DiskMan wrote {}, but couldn't force the \
                     folder onto the disk ({}).  A power cut in the next few \
                     seconds could lose the save.", path.display(), error));
    }

    Ok(())
}

/// The same as write_file(), except only our own user can read or write the
/// file afterwards (0600).  For the TLS key, and anything else that has no
/// business being read by another user on the machine.
///
/// The trick is to make the temp file ourselves first, private from the
/// moment it exists, and then let write_file() fill it.  Filling a file
/// that is already there keeps its permissions, and so does the rename.
/// So there is no moment where the key sits on the disk readable by
/// anybody else.
// Rust note: `.mode()` comes from OpenOptionsExt, which is Unix only.  It
// sets the permissions a brand new file is made with.
#[track_caller]
pub fn write_private_file(file: &StratumFile) -> io::Result<()> {
    let temp_path = temp_path_for(&file.path);

    // A temp file left over from a crash would keep whatever permissions it
    // was made with, so it goes first.
    let _ = fs::remove_file(&temp_path);
    let _ = fs::create_dir_all(folder_of(&file.path));

    let made = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp_path);
    if let Err(error) = made {
        return Err(complain(&format!("DiskMan can't make the private file {}",
                                     temp_path.display()),
                            error));
    }

    write_file(file)
}

/// Deletes a file for good, and doesn't come back until the deletion is safe
/// on the disk.  A leftover `path.tmp` from an old crash goes too.
///
/// Same rule as write_file(): this is for paths that never go through
/// write_later().  A file still waiting in the cache would get written back
/// a moment after we deleted it, so if the path is in the cache we refuse,
/// and say so.
///
/// A file that isn't there comes back as `NotFound` and isn't logged, the
/// same as a read.  The caller knows whether that is a problem.
#[track_caller]
pub fn delete_file(path: &Path) -> io::Result<()> {
    if cached_copy(path).is_some() {
        let error = io::Error::new(io::ErrorKind::InvalidInput,
                                   "it is still waiting in the cache to be written");
        return Err(complain(&format!("DiskMan won't delete {}", path.display()), error));
    }

    if let Err(error) = fs::remove_file(path) {
        if error.kind() == io::ErrorKind::NotFound {
            return Err(error);
        }
        return Err(complain(&format!("DiskMan can't delete {}", path.display()), error));
    }
    let _ = fs::remove_file(temp_path_for(path));

    // Same as after a rename.  The file is gone from the folder's list of
    // names, but only in memory until the folder is forced onto the disk.
    // A power cut before that and the file comes back.  The file is gone
    // either way by now, so this is a Warn and the delete still counts.
    let folder = folder_of(path);
    if let Err(error) = sync_folder(&folder) {
        scribe::warn(Channel::Core,
                     &format!("DiskMan deleted {}, but couldn't force the \
                     folder onto the disk ({}).  A power cut in the next few \
                     seconds could bring it back.", path.display(), error));
    }

    Ok(())
}

/// Makes a folder, and any folder above it that is missing.  It says so in
/// the log when it really made one, so the first launch shows what got
/// built.  A folder that is already there is fine, and quiet.
#[track_caller]
pub fn make_folder(folder: &Path) -> io::Result<()> {
    if folder.is_dir() {
        return Ok(());
    }
    if let Err(error) = fs::create_dir_all(folder) {
        return Err(complain(&format!("DiskMan can't make the folder {}", folder.display()), error));
    }
    scribe::info(Channel::Core, &format!("DiskMan made the folder {}", folder.display()));
    Ok(())
}

/// Makes a folder only our own user can open (0700).  Any folder above it
/// that is missing gets made too, with the ordinary permissions.  For
/// saved/ssl/, where the TLS key lives.
///
/// If the folder is already there and anybody else can get into it, it gets
/// tightened to 0700, and the log says so.  A folder that is already private
/// is fine, and quiet.
#[track_caller]
pub fn make_private_folder(folder: &Path) -> io::Result<()> {
    match make_folder_private(folder) {
        Ok(PrivateFolder::AlreadyPrivate) => Ok(()),
        Ok(PrivateFolder::Made) => {
            scribe::info(Channel::Core,
                         &format!("DiskMan made the private folder {}", folder.display()));
            Ok(())
        }
        Ok(PrivateFolder::Tightened) => {
            scribe::info(Channel::Core,
                         &format!("DiskMan made {} private.  Only our own user can open it now.",
                                  folder.display()));
            Ok(())
        }
        Err(error) => Err(complain(&format!("DiskMan can't make the private folder {}",
                                            folder.display()), error)),
    }
}

/// What make_folder_private() found, or did.
#[derive(Debug, PartialEq)]
enum PrivateFolder {
    AlreadyPrivate,
    Made,
    Tightened,
}

/// The part of make_private_folder() that doesn't log, so the tests can run
/// it.  "Private" means nobody but us can do anything with it: the group and
/// everybody-else bits are all off.  A folder that is even tighter than 0700
/// is left alone.
// Rust note: `.mode()` on the permissions carries the file type in its high
// bits, so `& 0o077` keeps only the group and everybody-else bits.
fn make_folder_private(folder: &Path) -> io::Result<PrivateFolder> {
    if folder.is_dir() {
        let mode = fs::metadata(folder)?.permissions().mode();
        if mode & 0o077 == 0 {
            return Ok(PrivateFolder::AlreadyPrivate);
        }
        fs::set_permissions(folder, fs::Permissions::from_mode(0o700))?;
        return Ok(PrivateFolder::Tightened);
    }

    // The folders above it get the ordinary permissions.  Only the last one
    // is made private, from the moment it exists.
    fs::create_dir_all(folder_of(folder))?;
    DirBuilder::new().mode(0o700).create(folder)?;
    Ok(PrivateFolder::Made)
}

/// Deletes an empty folder for good, and doesn't come back until the
/// deletion is safe on the disk.  Only an empty one: a folder with anything
/// in it is refused, and whatever is in it has to go first, one file at a
/// time, through delete_file() or move_file().  Jacob's call, so a wrong
/// path can't take a whole tree with it.  For an account's folder in
/// saved/players/, once its player files have moved to saved/orphaned/.
///
/// A file still waiting in the cache anywhere under the folder would put the
/// folder back when the writer got to it, so that is refused too.
///
/// A folder that isn't there comes back as `NotFound` and isn't logged, the
/// same as delete_file().
#[track_caller]
pub fn delete_folder(folder: &Path) -> io::Result<()> {
    let waiting = {
        let guard = CACHE.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.as_ref() {
            Some(cache) => cache_has_files_under(cache, folder),
            None => false,
        }
    };
    if waiting {
        let error = io::Error::new(io::ErrorKind::InvalidInput,
                                   "something in it is still waiting in the cache to be written");
        return Err(complain(&format!("DiskMan won't delete the folder {}", folder.display()),
                            error));
    }

    if let Err(error) = fs::remove_dir(folder) {
        if error.kind() == io::ErrorKind::NotFound {
            return Err(error);
        }
        return Err(complain(&format!("DiskMan can't delete the folder {}", folder.display()),
                            error));
    }

    // A folder is a name in the folder above it, so that is the one to force
    // onto the disk.
    let above = folder_of(folder);
    if let Err(error) = sync_folder(&above) {
        scribe::warn(Channel::Core,
                     &format!("DiskMan deleted the folder {}, but couldn't force {} onto the \
                     disk ({}).  A power cut in the next few seconds could bring it back.",
                              folder.display(), above.display(), error));
    }

    Ok(())
}

/// Moves a file to a new path, and doesn't come back until the move is safe
/// on the disk.  It is a rename, so it happens all at once: the file is at
/// one path or the other, never half at each.  Makes the new folder if it
/// isn't there.  For a deleted account's player files, on their way into
/// saved/orphaned/players/ under their UUIDs.
///
/// It won't write over a file that is already at `to`.  It won't touch
/// either path while it is waiting in the cache, so whoever moves a file
/// that goes through write_later() calls flush() first.  A leftover
/// `from.tmp` goes too.
///
/// A rename only works within one disk.  If `to` is on another one (a
/// folder linked to the NVMe, say), the move fails, says so, and nothing
/// has changed.
///
/// A file that isn't at `from` comes back as `NotFound` and isn't logged,
/// the same as delete_file().
#[track_caller]
pub fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    let what = format!("DiskMan won't move {} to {}", from.display(), to.display());

    if cached_copy(from).is_some() || cached_copy(to).is_some() {
        let error = io::Error::new(io::ErrorKind::InvalidInput,
                                   "one of them is still waiting in the cache to be written");
        return Err(complain(&what, error));
    }
    if to.exists() {
        let error = io::Error::new(io::ErrorKind::AlreadyExists,
                                   "there is already a file at the new path");
        return Err(complain(&what, error));
    }

    let to_folder = folder_of(to);
    if let Err(error) = fs::create_dir_all(&to_folder) {
        return Err(complain(&format!("DiskMan can't make the folder {}", to_folder.display()),
                            error));
    }

    if let Err(error) = fs::rename(from, to) {
        if error.kind() == io::ErrorKind::NotFound {
            return Err(error);
        }
        return Err(complain(&format!("DiskMan can't move {} to {}", from.display(), to.display()),
                            error));
    }
    let _ = fs::remove_file(temp_path_for(from));

    // Both folders' lists of names changed.  The new one goes onto the disk
    // first, so a power cut in between can't lose the file from both.
    for folder in [to_folder, folder_of(from)] {
        if let Err(error) = sync_folder(&folder) {
            scribe::warn(Channel::Core,
                         &format!("DiskMan moved {} to {}, but couldn't force {} onto the \
                         disk ({}).  A power cut in the next few seconds could undo it.",
                                  from.display(), to.display(), folder.display(), error));
        }
    }

    Ok(())
}

/// Deletes every leftover `.tmp` file anywhere under `folder`.  Each one is
/// a save that a crash cut short between the write and the rename, so the
/// real file next to it is the last good save, and the `.tmp` is no use to
/// anybody.  They were never a danger, since the next save of that file
/// wipes its `.tmp`, but until then they lie around.
///
/// constellations::make_folders() calls this at launch, before anything
/// saves.  That is the only time a `.tmp` can't be a save in progress, so
/// nothing else should call it.
///
/// Each one gets an Info line, since each one means a save was lost.  It
/// never goes through a link, so it can't wander out of the folder.  A
/// folder it can't read is skipped, quietly: a leftover is only clutter.
#[track_caller]
pub fn clear_leftovers(folder: &Path) {
    for path in find_leftovers(folder) {
        match fs::remove_file(&path) {
            Ok(()) => {
                scribe::info(Channel::Core,
                             &format!("DiskMan cleared {}, left over from a save a crash cut short.",
                                      path.display()));
            }
            Err(error) => {
                scribe::warn(Channel::Core,
                             &format!("DiskMan can't clear the leftover {} ({}).",
                                      path.display(), error));
            }
        }
    }
}

/// Every `.tmp` file under `folder`, going down into the folders inside it
/// but never through a link.  It doesn't delete or log anything, so the
/// tests can run it.
// Rust note: `.flatten()` on the folder's entries quietly skips any entry
// that couldn't be read, and `file_type()` on an entry describes the entry
// itself -- a link is a link, not whatever it points at.
fn find_leftovers(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(_) => return found,
    };

    for entry in entries.flatten() {
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(_) => continue,
        };
        let path = entry.path();
        if kind.is_dir() {
            found.extend(find_leftovers(&path));
        } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "tmp") {
            found.push(path);
        }
    }
    found
}


/// Writes the bytes to a brand new file (or wipes an old one) and doesn't
/// come back until the operating system says they are on the disk.
///
/// Without `sync_all()` the bytes can sit in memory for a while before the
/// operating system gets round to them.  The rename after this would then be
/// renaming a file that is empty on the disk, and a power cut at the wrong
/// moment leaves us with an empty file where the old one used to be.
fn write_and_sync(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

/// Forces a folder's list of names onto the disk.  This is how a rename gets
/// made permanent on Linux.
fn sync_folder(folder: &Path) -> io::Result<()> {
    File::open(folder)?.sync_all()
}

// ---------------------------------------------------------------------------
// The writer thread
// ---------------------------------------------------------------------------

/// One file the writer has taken out of the cache to write.  It stays in the
/// cache while it is being written, so a read in the meantime still finds it.
struct BatchItem {
    path: PathBuf,
    contents: Arc<Vec<u8>>,
    version: u64,
}

/// The writer thread's whole life.  Sleep until the cache has something in
/// it, take all of it, write it, take out whatever was written, repeat.
/// Returns once stop() has been called and the cache is empty.
///
/// When things are quiet a batch is one file, written a moment after it was
/// saved.  When things are busy, everything that piled up while the last
/// batch was writing goes out together as the next one.  That is where the
/// several threads and the one folder sync pay off.
fn writer_loop() {
    loop {
        let mut guard = CACHE.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let batch = loop {
            let cache = guard.get_or_insert_with(new_cache);
            if !cache.files.is_empty() {
                break cache_take_batch(cache);
            }
            if cache.stopping {
                return;
            }
            guard = CACHE_CHANGED.wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        };
        drop(guard);

        let started = Instant::now();
        let written = write_batch(&batch);
        let elapsed = started.elapsed();

        let bytes: usize = batch.iter().map(|item| item.contents.len()).sum();
        scribe::debug(Channel::Core,
                      &format!("DiskMan wrote {} of {} file(s), {} bytes, in {} ms.",
                               written, batch.len(), bytes, elapsed.as_millis()));

        let mut guard = CACHE.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cache) = guard.as_mut() {
            cache_finish_batch(cache, &batch);
        }
        drop(guard);
        CACHE_CHANGED.notify_all();
    }
}

/// Writes a batch: every temp file written and synced, WRITER_THREADS at a
/// time, then all the renames, then each folder synced once.  A file that
/// fails at any step gets an Error line and is dropped.  Hands back how many
/// made it.
fn write_batch(batch: &[BatchItem]) -> usize {
    let per_thread = batch.len().div_ceil(WRITER_THREADS);

    // Step 1: the temp files, several threads at once.  Each thread hands
    // back which of its files made it.
    // Rust note: `thread::scope` starts threads that are guaranteed to finish
    // before the scope ends, which is what lets them borrow `batch` straight
    // out of this function.  `join()` gets each thread's answer back.
    let mut temp_ok: Vec<bool> = Vec::new();
    thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in batch.chunks(per_thread) {
            handles.push(scope.spawn(move || {
                chunk.iter().map(write_temp).collect::<Vec<bool>>()
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(results) => temp_ok.extend(results),
                // A thread that panicked wrote nothing we can trust.  Count
                // its files as failed.  The panic itself has already printed.
                Err(_) => temp_ok.extend(vec![false; per_thread]),
            }
        }
    });

    // Step 2: the renames, one at a time.  They are quick -- nothing gets
    // synced here.
    let mut written = 0;
    let mut folders: Vec<PathBuf> = Vec::new();
    for (item, ok) in batch.iter().zip(temp_ok.iter()) {
        if !ok {
            continue;
        }
        let temp_path = temp_path_for(&item.path);
        match fs::rename(&temp_path, &item.path) {
            Ok(()) => {
                written += 1;
                folders.push(folder_of(&item.path));
            }
            Err(error) => {
                let _ = fs::remove_file(&temp_path);
                dropped(&format!("can't rename {} over {}", temp_path.display(),
                                 item.path.display()), &error);
            }
        }
    }

    // Step 3: each folder once, however many files went into it.
    folders.sort();
    folders.dedup();
    for folder in &folders {
        if let Err(error) = sync_folder(folder) {
            scribe::warn(Channel::Core,
                         &format!("DiskMan wrote into {}, but couldn't force the \
                         folder onto the disk ({}).  A power cut in the next few \
                         seconds could lose those saves.", folder.display(), error));
        }
    }

    written
}

/// Step 1 for one file: make its folder, write its temp file and sync it.
/// True if all of that worked.
fn write_temp(item: &BatchItem) -> bool {
    let folder = folder_of(&item.path);
    if let Err(error) = fs::create_dir_all(&folder) {
        dropped(&format!("can't make the folder {}", folder.display()), &error);
        return false;
    }

    let temp_path = temp_path_for(&item.path);
    if let Err(error) = write_and_sync(&temp_path, &item.contents) {
        let _ = fs::remove_file(&temp_path);
        dropped(&format!("can't write {}", temp_path.display()), &error);
        return false;
    }
    true
}

/// The writer couldn't write a file.  It is gone from the cache after this
/// batch, so this is the last anybody hears of it.  Whatever is on the disk
/// is the last good save.
fn dropped(what: &str, error: &io::Error) {
    scribe::error(Channel::Core,
                  &format!("DiskMan {} ({}).  Dropped it -- whatever is on the \
                  disk is the last good save.", what, error));
}

// ---------------------------------------------------------------------------
// The cache itself
// ---------------------------------------------------------------------------

// These take the Cache as an argument and don't touch the global, the lock,
// the disk or Scribe.  That is what lets the tests run them.

/// Whether a file of `size` bytes for `path` fits right now.  A file for a
/// path that is already waiting replaces the old copy, so it doesn't add to
/// the count.  An empty cache always has room, even for one file bigger than
/// the whole limit -- otherwise that file would wait forever.
fn cache_has_room(cache: &Cache, path: &Path, size: u64) -> bool {
    if cache.files.is_empty() {
        return true;
    }

    let (files_after, bytes_after) = match cache.files.get(path) {
        Some(old) => (cache.files.len(),
                      cache.bytes - old.contents.len() as u64 + size),
        None => (cache.files.len() + 1, cache.bytes + size),
    };
    files_after <= MAX_CACHE_FILES && bytes_after <= MAX_CACHE_BYTES
}

/// Puts a file in the cache, over the top of any older copy for the same
/// path.
fn cache_insert(cache: &mut Cache, file: StratumFile) {
    let size = file.contents.len() as u64;
    let pending = Pending {
        contents: Arc::new(file.contents),
        version: cache.next_version,
    };
    cache.next_version += 1;

    // Rust note: `insert` hands back the old entry if there was one.
    if let Some(old) = cache.files.insert(file.path, pending) {
        cache.bytes -= old.contents.len() as u64;
    }
    cache.bytes += size;
}

/// A copy of the newest waiting bytes for `path`, if there are any.
fn cache_lookup(cache: &Cache, path: &Path) -> Option<Vec<u8>> {
    cache.files.get(path).map(|pending| pending.contents.to_vec())
}

/// Whether anything waiting in the cache lives somewhere under `folder`.
// Rust note: `starts_with` on a path goes a whole folder name at a time, so
// `/saved/players/jac` doesn't count as holding `/saved/players/jacob/x`.
fn cache_has_files_under(cache: &Cache, folder: &Path) -> bool {
    cache.files.keys().any(|path| path.starts_with(folder))
}

/// Everything in the cache, ready to write.  The cache keeps all of it until
/// cache_finish_batch() says otherwise.
fn cache_take_batch(cache: &Cache) -> Vec<BatchItem> {
    let mut batch = Vec::new();
    for (path, pending) in &cache.files {
        batch.push(BatchItem {
            path: path.clone(),
            contents: Arc::clone(&pending.contents),
            version: pending.version,
        });
    }
    batch
}

/// The batch is done.  Every file in it leaves the cache, whether it was
/// written or dropped -- unless somebody saved a newer copy while we were
/// writing.  Then the newer copy stays, and goes out with the next batch.
fn cache_finish_batch(cache: &mut Cache, batch: &[BatchItem]) {
    for item in batch {
        let still_newest = match cache.files.get(&item.path) {
            Some(pending) => pending.version == item.version,
            None => false,
        };
        if still_newest {
            if let Some(old) = cache.files.remove(&item.path) {
                cache.bytes -= old.contents.len() as u64;
            }
        }
    }

    if cache.files.is_empty() {
        cache.said_full = false;
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Reads a whole file and hands it back as a StratumFile.  If the file is
/// still waiting in the cache, this is the cached copy -- the newest one.
///
/// If the file isn't there, the error comes back with the kind `NotFound`
/// and nothing is logged.  Any other failure is logged and handed back.
// Rust note: `&Path` is a borrowed path.  A `&PathBuf` can be passed in
// here as it is -- the compiler turns one into the other for us.
#[track_caller]
pub fn read_file(path: &Path) -> io::Result<StratumFile> {
    let contents = match cached_copy(path) {
        Some(contents) => contents,
        None => match fs::read(path) {
            Ok(contents) => contents,
            Err(error) => return Err(complain_unless_missing(path, error)),
        },
    };

    Ok(StratumFile {
        path: path.to_path_buf(),
        contents,
    })
}

/// Reads a whole text file and hands it back as a String.  Checks the cache
/// first, the same as read_file().  A file that isn't valid UTF-8 (so it
/// isn't really text) is an error, with the kind `InvalidData`.
///
/// A missing file is handled the same way as in read_file(): the error comes
/// back with the kind `NotFound` and nothing is logged.
#[track_caller]
pub fn read_text(path: &Path) -> io::Result<String> {
    let contents = match cached_copy(path) {
        Some(contents) => contents,
        None => match fs::read(path) {
            Ok(contents) => contents,
            Err(error) => return Err(complain_unless_missing(path, error)),
        },
    };

    match String::from_utf8(contents) {
        Ok(text) => Ok(text),
        Err(_) => {
            let error = io::Error::new(io::ErrorKind::InvalidData,
                                       "it isn't valid UTF-8 text");
            Err(complain(&format!("DiskMan can't read {}", path.display()),
                         error))
        }
    }
}

/// The cached copy of a file, if it is waiting to be written.  Takes the lock
/// just long enough to copy the bytes.
fn cached_copy(path: &Path) -> Option<Vec<u8>> {
    let guard = CACHE.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match guard.as_ref() {
        Some(cache) => cache_lookup(cache, path),
        None => None,
    }
}

// ---------------------------------------------------------------------------
// The small stuff
// ---------------------------------------------------------------------------

/// Says what went wrong, in the Error color, and hands the error back so the
/// caller can return it.
#[track_caller]
fn complain(what: &str, error: io::Error) -> io::Error {
    scribe::error(Channel::Core, &format!("{} ({}).", what, error));
    error
}

/// A read failed.  Logs it unless the file just isn't there, and hands the
/// error back either way.
#[track_caller]
fn complain_unless_missing(path: &Path, error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::NotFound {
        return error;
    }
    complain(&format!("DiskMan can't read {}", path.display()), error)
}

/// The folder a file lives in.  A bare file name like `stratum.conf` lives
/// in the folder the server was started from, which Rust calls `.`.
fn folder_of(path: &Path) -> PathBuf {
    match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// `stratum.conf` becomes `stratum.conf.tmp`, in the same folder.  It has to
/// be the same folder, because a rename only works within one disk.
// Rust note: a path isn't always valid text on Linux, so we can't just glue
// ".tmp" onto a String.  OsString is the operating system's own kind of
// string, and it will take ".tmp" on the end without asking questions.
fn temp_path_for(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".tmp");
    PathBuf::from(name)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// None of these go down a path that logs, and none of them touch the global
// cache or the writer thread.  Scribe would write into the real log folder,
// and a test run shouldn't leave anything there.  The cache is tested by
// handing its functions a Cache of their own.

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of our own under the system temp folder, empty to start.
    /// The process ID is in the name so two test runs can't collide, and the
    /// test name is in it because the tests run at the same time.
    fn test_folder(test_name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("stratum_diskman_{}_{}",
                                                       std::process::id(),
                                                       test_name));
        let _ = fs::remove_dir_all(&folder);
        folder
    }

    fn a_file(path: &str, contents: &str) -> StratumFile {
        StratumFile {
            path: PathBuf::from(path),
            contents: contents.as_bytes().to_vec(),
        }
    }

    #[test]
    fn what_we_write_we_can_read() {
        let folder = test_folder("round_trip");
        let path = folder.join("player.sav");

        let written = StratumFile {
            path: path.clone(),
            contents: vec![0, 1, 2, 255, b'\n', 42],
        };
        write_file(&written).unwrap();

        let read_back = read_file(&path).unwrap();
        assert_eq!(read_back.path, path);
        assert_eq!(read_back.contents, written.contents);

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_second_write_replaces_the_first_and_leaves_no_tmp() {
        let folder = test_folder("replace");
        let path = folder.join("stratum.conf");

        write_file(&StratumFile {
            path: path.clone(),
            contents: b"a much longer first version".to_vec(),
        }).unwrap();
        write_file(&StratumFile {
            path: path.clone(),
            contents: b"short".to_vec(),
        }).unwrap();

        assert_eq!(read_text(&path).unwrap(), "short");
        assert!(!temp_path_for(&path).exists());

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_leftover_tmp_gets_written_over() {
        let folder = test_folder("leftover");
        let path = folder.join("stratum.conf");

        // What a crash between the write and the rename would leave behind.
        fs::create_dir_all(&folder).unwrap();
        fs::write(temp_path_for(&path), "half a file from last ti").unwrap();

        write_file(&StratumFile {
            path: path.clone(),
            contents: b"the real thing".to_vec(),
        }).unwrap();

        assert_eq!(read_text(&path).unwrap(), "the real thing");
        assert!(!temp_path_for(&path).exists());

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn missing_folders_get_made() {
        let folder = test_folder("folders");
        let path = folder.join("zones").join("0_0").join("zone.dat");

        write_file(&StratumFile {
            path: path.clone(),
            contents: b"x".to_vec(),
        }).unwrap();

        assert!(path.exists());

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_missing_file_reads_as_not_found() {
        let folder = test_folder("missing");
        let path = folder.join("nothing_here.conf");

        let error = read_text(&path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);

        let error = read_file(&path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_deleted_file_is_gone() {
        let folder = test_folder("delete");
        let path = folder.join("jacob.act");

        write_file(&StratumFile {
            path: path.clone(),
            contents: b"{}".to_vec(),
        }).unwrap();
        // What a crash in the middle of an old save would leave behind.
        fs::write(temp_path_for(&path), "half").unwrap();

        delete_file(&path).unwrap();
        assert!(!path.exists());
        assert!(!temp_path_for(&path).exists());

        // A second time, there is nothing there to delete.
        assert_eq!(delete_file(&path).unwrap_err().kind(), io::ErrorKind::NotFound);

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn temp_names_and_folders() {
        assert_eq!(temp_path_for(Path::new("/opt/stratum/content/config/stratum.conf")),
                   PathBuf::from("/opt/stratum/content/config/stratum.conf.tmp"));
        assert_eq!(temp_path_for(Path::new("zone")), PathBuf::from("zone.tmp"));
        assert_eq!(folder_of(Path::new("/opt/stratum/x.conf")),
                   PathBuf::from("/opt/stratum"));
        assert_eq!(folder_of(Path::new("x.conf")), PathBuf::from("."));
    }

    #[test]
    fn the_newest_copy_wins() {
        let mut cache = new_cache();
        cache_insert(&mut cache, a_file("/p/alice.sav", "first save"));
        cache_insert(&mut cache, a_file("/p/alice.sav", "second"));

        assert_eq!(cache.files.len(), 1);
        assert_eq!(cache.bytes, 6);
        assert_eq!(cache_lookup(&cache, Path::new("/p/alice.sav")).unwrap(),
                   b"second".to_vec());
        assert_eq!(cache_lookup(&cache, Path::new("/p/bob.sav")), None);
    }

    #[test]
    fn a_finished_batch_leaves_the_cache() {
        let mut cache = new_cache();
        cache_insert(&mut cache, a_file("/p/alice.sav", "alice"));
        cache_insert(&mut cache, a_file("/p/bob.sav", "bob"));

        let batch = cache_take_batch(&cache);
        assert_eq!(batch.len(), 2);
        // Still there while it is being written, so a read can find it.
        assert_eq!(cache.files.len(), 2);

        cache_finish_batch(&mut cache, &batch);
        assert!(cache.files.is_empty());
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn a_save_during_a_batch_waits_for_the_next_one() {
        let mut cache = new_cache();
        cache_insert(&mut cache, a_file("/p/alice.sav", "old"));

        let batch = cache_take_batch(&cache);
        // Alice saves again while the writer is busy with the old copy.
        cache_insert(&mut cache, a_file("/p/alice.sav", "newer"));
        cache_finish_batch(&mut cache, &batch);

        assert_eq!(cache_lookup(&cache, Path::new("/p/alice.sav")).unwrap(),
                   b"newer".to_vec());
        assert_eq!(cache.bytes, 5);
    }

    #[test]
    fn room_in_the_cache() {
        let mut cache = new_cache();

        // An empty cache takes anything, even a file bigger than the limit.
        assert!(cache_has_room(&cache, Path::new("/huge"), MAX_CACHE_BYTES + 1));

        for number in 0..MAX_CACHE_FILES {
            cache_insert(&mut cache, a_file(&format!("/p/{}.sav", number), "x"));
        }
        // Full on count.  A new path has to wait.  A path that is already
        // waiting just replaces its copy, so it doesn't.
        assert!(!cache_has_room(&cache, Path::new("/p/new.sav"), 1));
        assert!(cache_has_room(&cache, Path::new("/p/7.sav"), 1));
    }

    #[test]
    fn a_batch_gets_written() {
        let folder = test_folder("batch");
        let mut cache = new_cache();
        for number in 0..20 {
            let path = folder.join(format!("p{}", number % 3))
                .join(format!("{}.sav", number));
            cache_insert(&mut cache, StratumFile {
                path,
                contents: format!("save {}", number).into_bytes(),
            });
        }

        let batch = cache_take_batch(&cache);
        assert_eq!(write_batch(&batch), 20);

        for number in 0..20 {
            let path = folder.join(format!("p{}", number % 3))
                .join(format!("{}.sav", number));
            assert_eq!(fs::read_to_string(&path).unwrap(),
                       format!("save {}", number));
            assert!(!temp_path_for(&path).exists());
        }

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_private_file_is_private_from_the_start() {
        let folder = test_folder("private");
        let path = folder.join("key.pem");
        let temp_path = temp_path_for(&path);

        // What a crash would leave behind, with the ordinary permissions a
        // plain `fs::write()` hands out.  The private write has to throw it
        // away rather than fill it, or the key inherits 0644.
        fs::create_dir_all(&folder).unwrap();
        fs::write(&temp_path, "an old half-written key").unwrap();
        fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o644)).unwrap();

        write_private_file(&StratumFile {
            path: path.clone(),
            contents: b"the key".to_vec(),
        }).unwrap();

        // `.mode()` carries the file type in its high bits, so mask down to
        // the permission bits before comparing.
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(read_text(&path).unwrap(), "the key");
        assert!(!temp_path.exists());

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_private_folder_is_private_and_an_open_one_gets_tightened() {
        let folder = test_folder("private_folder");
        let ssl = folder.join("saved").join("ssl");

        assert_eq!(make_folder_private(&ssl).unwrap(), PrivateFolder::Made);
        assert_eq!(fs::metadata(&ssl).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(make_folder_private(&ssl).unwrap(), PrivateFolder::AlreadyPrivate);

        // One made before this existed, with the ordinary permissions.
        fs::set_permissions(&ssl, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(make_folder_private(&ssl).unwrap(), PrivateFolder::Tightened);
        assert_eq!(fs::metadata(&ssl).unwrap().permissions().mode() & 0o777, 0o700);

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_moved_file_is_only_at_its_new_path() {
        let folder = test_folder("move");
        let from = folder.join("players").join("jacob").join("aldric.plyr");
        let to = folder.join("orphaned").join("players")
            .join("3f2a91c0-e4b7-4d1a-9c0e-2b7f5a6d8e10.plyr");

        write_file(&StratumFile {
            path: from.clone(),
            contents: b"{}".to_vec(),
        }).unwrap();
        // What a crash in the middle of an old save would leave behind.
        fs::write(temp_path_for(&from), "half").unwrap();

        move_file(&from, &to).unwrap();
        assert!(!from.exists());
        assert!(!temp_path_for(&from).exists());
        assert_eq!(read_text(&to).unwrap(), "{}");

        // A second time, there is nothing there to move.  (A new `to`, or
        // the one already there would be the complaint, and it would log.)
        let again = folder.join("orphaned").join("players").join("again.plyr");
        assert_eq!(move_file(&from, &again).unwrap_err().kind(), io::ErrorKind::NotFound);

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_empty_folder_can_be_deleted() {
        let folder = test_folder("delete_folder");
        let account = folder.join("players").join("jacob");
        fs::create_dir_all(&account).unwrap();

        delete_folder(&account).unwrap();
        assert!(!account.exists());
        assert!(folder.join("players").exists());

        // A second time, there is nothing there to delete.
        assert_eq!(delete_folder(&account).unwrap_err().kind(), io::ErrorKind::NotFound);

        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn files_in_the_cache_under_a_folder() {
        let mut cache = new_cache();
        cache_insert(&mut cache, a_file("/content/saved/players/jacob/aldric.plyr", "{}"));

        assert!(cache_has_files_under(&cache, Path::new("/content/saved/players/jacob")));
        assert!(cache_has_files_under(&cache, Path::new("/content/saved/players")));
        assert!(!cache_has_files_under(&cache, Path::new("/content/saved/players/jac")));
        assert!(!cache_has_files_under(&cache, Path::new("/content/saved/orphaned")));
    }

    #[test]
    fn leftovers_are_found_but_never_through_a_link() {
        let folder = test_folder("leftovers");
        let players = folder.join("players");
        let elsewhere = folder.join("elsewhere");
        fs::create_dir_all(players.join("jacob")).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();

        fs::write(players.join("stray.tmp"), "half").unwrap();
        fs::write(players.join("jacob").join("aldric.plyr.tmp"), "half").unwrap();
        fs::write(players.join("jacob").join("aldric.plyr"), "{}").unwrap();
        fs::write(elsewhere.join("not_ours.tmp"), "half").unwrap();
        std::os::unix::fs::symlink(&elsewhere, players.join("link")).unwrap();

        let mut found = find_leftovers(&players);
        found.sort();
        assert_eq!(found, vec![players.join("jacob").join("aldric.plyr.tmp"),
                               players.join("stray.tmp")]);

        let _ = fs::remove_dir_all(&folder);
    }
    
    // -----------------------------------------------------------------------
    // The benchmark
    // -----------------------------------------------------------------------

    /// Where the benchmark writes.  It has to be the real content folder and
    /// not the system temp folder, because the whole point is to time the
    /// drive the saves will actually live on.
    const BENCH_FOLDER: &str = "/opt/stratum/content/bench";

    /// How many saves, and how big each one is.  8 KB was a guess at a
    /// player file, made before there were any.
    const BENCH_SAVES: usize = 50;
    const BENCH_BYTES: usize = 8 * 1024;
    
    /// Times 50 player-sized saves, several ways, on the real drive.  Not part
    /// of a normal `cargo test` -- run it by hand:
    ///
    ///     cargo test fifty_saves -- --ignored --nocapture
    ///
    /// The "no sync" line is only there for comparison.  It is what a save
    /// costs when we don't wait for the disk, and it is not safe.
    #[test]
    #[ignore]
    fn fifty_saves_benchmark() {
        let folder = PathBuf::from(BENCH_FOLDER);
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();

        println!();
        println!("{} saves of {} bytes each, in {}", BENCH_SAVES, BENCH_BYTES,
                 BENCH_FOLDER);
        println!();

        // One save first and not timed, so the folder and the drive are awake
        // before the clock starts.
        write_file(&bench_files(&folder, "warmup", 1).remove(0)).unwrap();

        // Twice through, because a spinning disk is moody and one run on its
        // own doesn't mean much.
        for pass in 1..=2 {
            println!("Pass {}:", pass);
            bench_no_sync(&folder, pass);
            for threads in [1, 2, 4, 8, 16] {
                bench_threads(&folder, pass, threads);
            }
            for threads in [4, 8, 16] {
                bench_batch(&folder, pass, threads);
            }
            println!();
        }

        let _ = fs::remove_dir_all(&folder);
    }

    /// A batch of player-sized files ready to save.  Every file gets different
    /// bytes, so nothing clever further down can get away with skipping one.
    fn bench_files(folder: &Path, label: &str, count: usize) -> Vec<StratumFile> {
        let mut files = Vec::new();
        for number in 0..count {
            let mut contents = vec![b'.'; BENCH_BYTES];
            let stamp = format!("{} {}", label, number);
            contents[..stamp.len()].copy_from_slice(stamp.as_bytes());
            files.push(StratumFile {
                path: folder.join(format!("player_{:02}.sav", number)),
                contents,
            });
        }
        files
    }

    /// The same temp-and-rename, minus both syncs.  The floor we can't beat.
    fn bench_no_sync(folder: &Path, pass: u32) {
        let files =
            bench_files(folder, &format!("nosync{}", pass), BENCH_SAVES);

        let started = Instant::now();
        for file in &files {
            let temp_path = temp_path_for(&file.path);
            fs::write(&temp_path, &file.contents).unwrap();
            fs::rename(&temp_path, &file.path).unwrap();
        }
        bench_report("no sync (unsafe)", started.elapsed());
    }

    /// The real write_file(), with the saves split evenly across some number
    /// of threads.  One thread is what DiskMan does today.
    // Rust note: `thread::scope` starts threads that are guaranteed to finish
    // before the scope ends.  That is what lets them borrow `files` straight
    // out of this function, with no locks or copies.  A panic in any of them
    // (a failed write) fails the test.
    fn bench_threads(folder: &Path, pass: u32, threads: usize) {
        let files = bench_files(folder, &format!("t{}p{}", threads, pass),
                                BENCH_SAVES);
        let per_thread = files.len().div_ceil(threads);

        let started = Instant::now();
        std::thread::scope(|scope| {
            for chunk in files.chunks(per_thread) {
                scope.spawn(move || {
                    for file in chunk {
                        write_file(file).unwrap();
                    }
                });
            }
        });

        let label = if threads == 1 {
            "1 thread (today)".to_string()
        } else {
            format!("{} threads", threads)
        };
        bench_report(&label, started.elapsed());
    }

    /// What the writer thread will do with a batch: every temp file written
    /// and synced, several at a time, then all the renames, then the folder
    /// synced once for the lot.  The one folder sync is the difference from
    /// bench_threads(), which syncs the folder after every single file.
    fn bench_batch(folder: &Path, pass: u32, threads: usize) {
        let files = bench_files(folder, &format!("b{}p{}", threads, pass),
                                BENCH_SAVES);
        let per_thread = files.len().div_ceil(threads);

        let started = Instant::now();
        std::thread::scope(|scope| {
            for chunk in files.chunks(per_thread) {
                scope.spawn(move || {
                    for file in chunk {
                        write_and_sync(&temp_path_for(&file.path), &file.contents)
                            .unwrap();
                    }
                });
            }
        });
        for file in &files {
            fs::rename(temp_path_for(&file.path), &file.path).unwrap();
        }
        sync_folder(folder).unwrap();

        bench_report(&format!("batch, {} threads", threads), started.elapsed());
    }

    fn bench_report(label: &str, elapsed: std::time::Duration) {
        let total_ms = elapsed.as_secs_f64() * 1000.0;
        println!("  {:<18} {:>8.1} ms total  {:>6.1} ms a save", label, total_ms,
                 total_ms / BENCH_SAVES as f64);
    }
}