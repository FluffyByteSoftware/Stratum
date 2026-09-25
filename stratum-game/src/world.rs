//! File:     stratum-game/src/world.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! The world: chunks of blocks, the whole set of them, in memory and on the
//! disk.  This is the first one, and it's for checking that positions work
//! once the client is up: 256 m square, 128 m tall, flat ground with the
//! odd bump in it, in the four block types blocks.rs starts with.
//!
//! ```text
//! y = 16       air        where a player's feet are
//! y = 15       grass      the top solid block, GROUND_LEVEL
//! y = 12..14   dirt
//! y = 0..11    stone
//! ```
//!
//! One column in 16 is a block higher than that and one in 16 a block
//! lower, picked by a hash of the column and the seed, so the same seed
//! always gives the same world and the ground isn't one flat sheet.  The
//! world starts at 0 on every axis, so the origin is the corner of the map
//! and the middle of it is (128, 16, 128).
//!
//! On the disk it's a folder, `saved/world/` in the content folder, with
//! `world.json` (this header), `blocks.json` (the block types) and one
//! region file per 4 x 4 x 4 chunks (region_file.rs): four of them for this
//! world.  The folder is what the patcher ships to a client and what the
//! world check compares, later.
//!
//! Two moments touch the disk.  At launch, `start()` generates the world
//! and writes the folder if `world.json` isn't there, and otherwise says
//! it found it.  At S), `load()` reads the folder back into a VoxelWorld,
//! which the game loop puts on its `World` as a resource.  Nothing writes
//! a chunk while the world runs yet; that's the saver's job, and it doesn't
//! exist.
//!
//! In memory the chunks are a HashMap from ChunkPos to Chunk.  A chunk
//! nobody has changed and that is all one block is 2 bytes, and a chunk
//! with the ground in it is 64 KiB, so this world is about 4 MiB.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::Path;
use std::time::Instant;

use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use stratum_tools::{constellations, diskman};
use stratum_tools::diskman::StratumFile;
use stratum_tools::scribe::{self, Channel};

use crate::blocks::{BLOCKS_FILE_NAME, Blocks, DIRT, GRASS, STONE};
use crate::chunk::{AIR, BlockId, CHUNK_SIDE, Chunk, ChunkPos, block_in_chunk};
use crate::player_file::now_seconds;
use crate::region_file::{self, FORMAT, REGION_SIDE, RegionPos};

/// The header's name inside the world folder.
pub const WORLD_FILE_NAME: &str = "world.json";

/// The top solid block of a column that isn't bumped.  A player standing on
/// it has their feet at 16.  Jacob's number.
pub const GROUND_LEVEL: i32 = 15;

/// Dirt goes this many blocks under the grass, and stone from there down.
const DIRT_DEPTH: i32 = 3;

/// One column in this many is a block higher than the ground, and one in
/// this many a block lower.
const BUMP_ONE_IN: u64 = 16;

/// The first world's size, in chunks, both ends included: 8 x 4 x 8, so
/// 256 blocks square and 128 tall.
const FIRST_CHUNK: [i32; 3] = [0, 0, 0];
const LAST_CHUNK: [i32; 3] = [7, 3, 7];

// ---------------------------------------------------------------------------
// The header, world.json
// ---------------------------------------------------------------------------

/// What `world.json` holds: the shape of the world and where it came from.
/// A client reads it to know what the region files are, and refuses a
/// format it doesn't know.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldFile {
    /// The region file's format, region_file::FORMAT.
    pub format: u16,
    /// What the generator was seeded with.
    pub seed: u64,
    pub chunk_side: u8,
    pub region_side: u8,
    /// The lowest chunk on each axis, included.
    pub first_chunk: [i32; 3],
    /// The highest chunk on each axis, included.
    pub last_chunk: [i32; 3],
    /// Seconds since 1970, when it was generated.
    pub generated_at: u64,
}

impl WorldFile {
    /// The header of the first world, as this server generates it.
    pub fn new(seed: u64, generated_at: u64) -> WorldFile {
        WorldFile {
            format: FORMAT,
            seed,
            chunk_side: CHUNK_SIDE as u8,
            region_side: REGION_SIDE as u8,
            first_chunk: FIRST_CHUNK,
            last_chunk: LAST_CHUNK,
            generated_at,
        }
    }

    pub fn to_text(&self) -> Result<String, String> {
        match serde_json::to_string_pretty(self) {
            Ok(text) => Ok(text),
            Err(error) => Err(format!("Couldn't turn the world's header into JSON: {}", error)),
        }
    }

    /// The text of a `world.json`, checked against what this server can
    /// read.  An `Err` says what's wrong with it, in words.
    pub fn from_text(text: &str) -> Result<WorldFile, String> {
        let header: WorldFile = match serde_json::from_str(text) {
            Ok(header) => header,
            Err(error) => return Err(format!("it isn't a world header ({})", error)),
        };
        if header.format != FORMAT {
            return Err(format!("it is format {}, and this server reads format {}", header.format, FORMAT));
        }
        if header.chunk_side as usize != CHUNK_SIDE || header.region_side as usize != REGION_SIDE {
            return Err(format!("it says {} chunks a side of {} blocks, and this server uses {} and {}",
                               header.region_side, header.chunk_side, REGION_SIDE, CHUNK_SIDE));
        }
        for axis in 0..3 {
            if header.first_chunk[axis] > header.last_chunk[axis] {
                return Err(format!("its first chunk {:?} is past its last chunk {:?}",
                                   header.first_chunk, header.last_chunk));
            }
        }
        Ok(header)
    }

    /// Every chunk the world has, in a fixed order: x fastest, then z,
    /// then y.
    pub fn chunk_positions(&self) -> Vec<ChunkPos> {
        let mut positions = Vec::with_capacity(self.chunk_count());
        for y in self.first_chunk[1]..=self.last_chunk[1] {
            for z in self.first_chunk[2]..=self.last_chunk[2] {
                for x in self.first_chunk[0]..=self.last_chunk[0] {
                    positions.push(ChunkPos::new(x, y, z));
                }
            }
        }
        positions
    }

    /// Every region the world's chunks fall in, each once, in the same
    /// order as the chunks.
    pub fn region_positions(&self) -> Vec<RegionPos> {
        let mut regions = Vec::new();
        for chunk in self.chunk_positions() {
            let region = RegionPos::of_chunk(chunk);
            if !regions.contains(&region) {
                regions.push(region);
            }
        }
        regions
    }

    pub fn chunk_count(&self) -> usize {
        let mut count = 1;
        for axis in 0..3 {
            count *= (self.last_chunk[axis] - self.first_chunk[axis] + 1) as usize;
        }
        count
    }

    /// Whether a chunk is inside the world.
    pub fn contains(&self, chunk: ChunkPos) -> bool {
        let inside = |value: i32, axis: usize| value >= self.first_chunk[axis] && value <= self.last_chunk[axis];
        inside(chunk.x, 0) && inside(chunk.y, 1) && inside(chunk.z, 2)
    }
}

// ---------------------------------------------------------------------------
// The world in memory
// ---------------------------------------------------------------------------

/// The world, as the game loop holds it: the header, the block types, and
/// every chunk.  A resource on the loop's `World`, so a system can ask it
/// what block is where.
// Rust note: `Resource` is bevy_ecs's word for one thing the World holds
// once, as opposed to a Component, which every entity can have.  The
// derive is what lets `world.insert_resource()` take it.
#[derive(Resource, Debug)]
pub struct VoxelWorld {
    pub header: WorldFile,
    pub blocks: Blocks,
    chunks: HashMap<ChunkPos, Chunk>,
}

impl VoxelWorld {
    /// The block at a position in the world.  Outside the world is air.
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> BlockId {
        match self.chunks.get(&ChunkPos::of_block(x, y, z)) {
            Some(chunk) => {
                let (cx, cy, cz) = block_in_chunk(x, y, z);
                chunk.block(cx, cy, cz)
            }
            None => AIR,
        }
    }

    /// How many chunks are held.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// The chunk at a position, if the world has it.
    pub fn chunk(&self, position: ChunkPos) -> Option<&Chunk> {
        self.chunks.get(&position)
    }

    /// A brand new world from a seed.  Pure: no files, no clock.
    pub fn generate(seed: u64, generated_at: u64) -> VoxelWorld {
        let header = WorldFile::new(seed, generated_at);
        let mut chunks = HashMap::with_capacity(header.chunk_count());
        for position in header.chunk_positions() {
            chunks.insert(position, generate_chunk(position, seed));
        }
        VoxelWorld { header, blocks: Blocks::starting_set(), chunks }
    }

    /// The whole world as the files that go in the world folder: the
    /// header, the block list, and one region file per region.  The
    /// caller writes them.  A region the world doesn't fill is padded with
    /// air chunks.
    pub fn files(&self, folder: &Path) -> Result<Vec<StratumFile>, String> {
        let mut files = Vec::new();
        files.push(StratumFile {
            path: folder.join(WORLD_FILE_NAME),
            contents: self.header.to_text()?.into_bytes(),
        });
        files.push(StratumFile {
            path: folder.join(BLOCKS_FILE_NAME),
            contents: self.blocks.to_text()?.into_bytes(),
        });

        let air = Chunk::filled_with(AIR);
        for region in self.header.region_positions() {
            let chunks: Vec<&Chunk> = region.chunks().iter()
                .map(|position| self.chunks.get(position).unwrap_or(&air))
                .collect();
            files.push(StratumFile {
                path: folder.join(region.file_name()),
                contents: region_file::to_bytes(region, &chunks)?,
            });
        }
        Ok(files)
    }
}

// ---------------------------------------------------------------------------
// The generator
// ---------------------------------------------------------------------------

/// One chunk of the first world, from its position and the seed.
fn generate_chunk(position: ChunkPos, seed: u64) -> Chunk {
    let (ox, oy, oz) = position.origin();
    let top = oy + CHUNK_SIDE as i32 - 1;

    // A chunk that starts above the highest bump is all sky, and one that
    // ends below the deepest dirt is all stone.  Three chunks in four of
    // this world are one or the other, and they cost nothing.
    if oy > GROUND_LEVEL + 1 {
        return Chunk::filled_with(AIR);
    }
    if top < GROUND_LEVEL - 1 - DIRT_DEPTH {
        return Chunk::filled_with(STONE);
    }

    let mut chunk = Chunk::filled_with(AIR);
    for z in 0..CHUNK_SIDE {
        for x in 0..CHUNK_SIDE {
            let surface = surface_height(ox + x as i32, oz + z as i32, seed);
            for y in 0..CHUNK_SIDE {
                chunk.set_block(x, y, z, column_block(oy + y as i32, surface));
            }
        }
    }

    // An all-air or all-stone chunk goes back to being 2 bytes.
    chunk.pack();
    chunk
}

/// The top solid block of the column at (x, z): the ground, or a block up
/// or down from it.
pub fn surface_height(x: i32, z: i32, seed: u64) -> i32 {
    match hash(x, z, seed) % BUMP_ONE_IN {
        0 => GROUND_LEVEL + 1,
        1 => GROUND_LEVEL - 1,
        _ => GROUND_LEVEL,
    }
}

/// What block is at height `y` in a column whose top solid block is at
/// `surface`.
fn column_block(y: i32, surface: i32) -> BlockId {
    if y > surface {
        AIR
    } else if y == surface {
        GRASS
    } else if y >= surface - DIRT_DEPTH {
        DIRT
    } else {
        STONE
    }
}

/// A well-mixed number from a column and the seed.  Not random: the same
/// three numbers always give the same answer, which is the point.
// Rust note: `wrapping_mul` lets the multiply overflow on purpose, which
// is what a hash wants.  A plain `*` would panic in a debug build.  The
// `as i64 as u64` keeps a negative coordinate's bits instead of turning
// it into a small number.  The constants are the ones splitmix64 uses.
fn hash(x: i32, z: i32, seed: u64) -> u64 {
    let mut mixed = seed
        ^ (x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (z as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    mixed ^= mixed >> 30;
    mixed = mixed.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed ^= mixed >> 27;
    mixed = mixed.wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^ (mixed >> 31)
}

// ---------------------------------------------------------------------------
// The disk
// ---------------------------------------------------------------------------

/// Makes the world if there isn't one.  main() calls this once at launch,
/// after the folders are made.  If `world.json` is there, the world is
/// left alone; if it isn't, a new one is generated, written, and waited
/// for.  A header that is there but can't be read is an Error, and the
/// world stays as it is: S) will refuse to start until it's fixed or the
/// folder is cleared.
///
/// Nothing in here stops the server.  A world that couldn't be written is
/// an Error in the log, and S) finds out.
pub fn start() {
    let folder = constellations::world_folder();
    let header_path = folder.join(WORLD_FILE_NAME);

    match diskman::read_text(&header_path) {
        Ok(text) => match WorldFile::from_text(&text) {
            Ok(header) => {
                scribe::info(Channel::World,
                             &format!("Found the world on the disk: {} chunk(s), seed {}.",
                                      header.chunk_count(), header.seed));
            }
            Err(problem) => {
                scribe::error(Channel::World,
                              &format!("{} can't be read: {}.  The server won't start until it's fixed \
                                        or the folder is cleared.",
                                       header_path.display(), problem));
            }
        },
        Err(error) if error.kind() == ErrorKind::NotFound => {
            make_a_new_world(&folder);
        }
        // DiskMan has already logged what went wrong.
        Err(_) => {}
    }
}

/// Generates the first world and writes its folder.
fn make_a_new_world(folder: &Path) {
    let seed = now_seconds();
    let began = Instant::now();
    let world = VoxelWorld::generate(seed, seed);
    let generated_in = began.elapsed();

    let files = match world.files(folder) {
        Ok(files) => files,
        Err(problem) => {
            scribe::error(Channel::World, &format!("The new world couldn't be turned into files: {}", problem));
            return;
        }
    };
    let file_count = files.len();
    let bytes: usize = files.iter().map(|file| file.contents.len()).sum();

    // A handful of files, well under the cache's limit, so this never
    // waits on the way in.  The flush waits for them on the way out, so
    // the world is on the disk before the menu comes up.
    let began = Instant::now();
    for file in files {
        diskman::write_later(file);
    }
    diskman::flush();
    let written_in = began.elapsed();

    // write_later() drops a file it can't write without telling us, so
    // the header is checked for.  A missing region file shows up at S).
    if diskman::read_file(&folder.join(WORLD_FILE_NAME)).is_err() {
        scribe::error(Channel::World,
                      &format!("The new world wasn't written to {}.  DiskMan has said why above.",
                               folder.display()));
        return;
    }

    scribe::info(Channel::World,
                 &format!("Generated a new world: {} chunk(s), seed {}, in {} ms.  Written to {} as {} \
                           file(s), {:.1} MiB, in {} ms.",
                          world.chunk_count(),
                          seed,
                          generated_in.as_millis(),
                          folder.display(),
                          file_count,
                          bytes as f64 / (1024.0 * 1024.0),
                          written_in.as_millis()));
}

/// Reads the world folder into a VoxelWorld.  The game loop calls this at
/// S), before its thread starts, and an `Err` stops the server from
/// starting: it says which file and what's wrong with it.
pub fn load() -> Result<VoxelWorld, String> {
    let began = Instant::now();
    let folder = constellations::world_folder();

    let header = read_header(&folder.join(WORLD_FILE_NAME))?;
    let blocks = read_blocks(&folder.join(BLOCKS_FILE_NAME))?;

    let mut chunks = HashMap::with_capacity(header.chunk_count());
    let regions = header.region_positions();
    for region in &regions {
        let path = folder.join(region.file_name());
        let file = match diskman::read_file(&path) {
            Ok(file) => file,
            Err(error) => return Err(format!("{} couldn't be read: {}", path.display(), error)),
        };
        let (found, region_chunks) = match region_file::from_bytes(&file.contents) {
            Ok(read) => read,
            Err(problem) => return Err(format!("{} can't be read: {}", path.display(), problem)),
        };
        if found != *region {
            return Err(format!("{} says it is region ({}, {}, {}), which isn't its name",
                               path.display(), found.x, found.y, found.z));
        }
        for (position, chunk) in region.chunks().into_iter().zip(region_chunks) {
            if header.contains(position) {
                chunks.insert(position, chunk);
            }
        }
    }

    scribe::info(Channel::World,
                 &format!("Loaded the world: {} chunk(s) from {} region file(s), seed {}, in {} ms.",
                          chunks.len(), regions.len(), header.seed, began.elapsed().as_millis()));
    Ok(VoxelWorld { header, blocks, chunks })
}

fn read_header(path: &Path) -> Result<WorldFile, String> {
    match diskman::read_text(path) {
        Ok(text) => WorldFile::from_text(&text)
            .map_err(|problem| format!("{} can't be read: {}", path.display(), problem)),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Err(format!("There is no world: {} isn't there.  It gets generated at launch.", path.display()))
        }
        Err(error) => Err(format!("{} couldn't be read: {}", path.display(), error)),
    }
}

fn read_blocks(path: &Path) -> Result<Blocks, String> {
    match diskman::read_text(path) {
        Ok(text) => Blocks::from_text(&text)
            .map_err(|problem| format!("{} can't be read: {}", path.display(), problem)),
        Err(error) => Err(format!("{} couldn't be read: {}", path.display(), error)),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// start() and load() aren't tested here: they go through DiskMan and
// Constellations.  The Launcher is their test.  Everything else is pure.
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use super::*;

    const SEED: u64 = 1_790_000_000;

    #[test]
    fn the_column_has_grass_on_dirt_on_stone() {
        assert_eq!(column_block(16, 15), AIR);
        assert_eq!(column_block(15, 15), GRASS);
        assert_eq!(column_block(14, 15), DIRT);
        assert_eq!(column_block(12, 15), DIRT);
        assert_eq!(column_block(11, 15), STONE);
        assert_eq!(column_block(0, 15), STONE);
    }

    #[test]
    fn most_columns_are_flat_and_the_rest_are_a_block_off() {
        let (mut up, mut down, mut flat) = (0, 0, 0);
        for z in 0..256 {
            for x in 0..256 {
                match surface_height(x, z, SEED) - GROUND_LEVEL {
                    1 => up += 1,
                    -1 => down += 1,
                    0 => flat += 1,
                    other => panic!("a column is {} blocks off the ground", other),
                }
            }
        }
        // One in 16 each way, give or take.
        assert!(up > 2000 && up < 6000, "{} columns up", up);
        assert!(down > 2000 && down < 6000, "{} columns down", down);
        assert!(flat > 50_000, "{} columns flat", flat);
    }

    #[test]
    fn the_same_seed_gives_the_same_world_and_the_hash_spreads() {
        assert_eq!(surface_height(100, 100, SEED), surface_height(100, 100, SEED));
        assert_eq!(hash(1, 2, SEED), hash(1, 2, SEED));
        assert_ne!(hash(1, 2, SEED), hash(2, 1, SEED));
        assert_ne!(hash(1, 2, SEED), hash(1, 2, SEED + 1));
        assert_ne!(hash(-1, 0, SEED), hash(1, 0, SEED));
    }

    #[test]
    fn a_generated_world_has_its_ground_at_fifteen() {
        let world = VoxelWorld::generate(SEED, SEED);
        assert_eq!(world.chunk_count(), 256);
        assert_eq!(world.header, WorldFile::new(SEED, SEED));

        // Whatever the bump, the layers hold together under it.
        for (x, z) in [(0, 0), (128, 128), (255, 255), (37, 200)] {
            let surface = surface_height(x, z, SEED);
            assert!((GROUND_LEVEL - 1..=GROUND_LEVEL + 1).contains(&surface));
            assert_eq!(world.block_at(x, surface + 1, z), AIR);
            assert_eq!(world.block_at(x, surface, z), GRASS);
            assert_eq!(world.block_at(x, surface - 1, z), DIRT);
            assert_eq!(world.block_at(x, surface - DIRT_DEPTH, z), DIRT);
            assert_eq!(world.block_at(x, surface - DIRT_DEPTH - 1, z), STONE);
            assert_eq!(world.block_at(x, 0, z), STONE);
            assert_eq!(world.block_at(x, 127, z), AIR);
        }

        // Outside the world is air, on every side.
        assert_eq!(world.block_at(-1, 5, 5), AIR);
        assert_eq!(world.block_at(5, -1, 5), AIR);
        assert_eq!(world.block_at(256, 5, 5), AIR);
        assert_eq!(world.block_at(5, 128, 5), AIR);
    }

    #[test]
    fn the_ground_chunks_are_full_and_the_sky_is_packed() {
        let world = VoxelWorld::generate(SEED, SEED);
        assert!(matches!(world.chunk(ChunkPos::new(3, 0, 3)), Some(Chunk::Full(_))));
        assert_eq!(world.chunk(ChunkPos::new(3, 1, 3)), Some(&Chunk::Uniform(AIR)));
        assert_eq!(world.chunk(ChunkPos::new(3, 3, 3)), Some(&Chunk::Uniform(AIR)));
        assert_eq!(world.chunk(ChunkPos::new(8, 0, 0)), None);
    }

    #[test]
    fn the_header_goes_there_and_back_and_is_checked() {
        let header = WorldFile::new(SEED, SEED);
        assert_eq!(WorldFile::from_text(&header.to_text().unwrap()), Ok(header.clone()));

        let mut wrong_format = header.clone();
        wrong_format.format = 99;
        assert!(WorldFile::from_text(&wrong_format.to_text().unwrap()).is_err());

        let mut wrong_side = header.clone();
        wrong_side.chunk_side = 16;
        assert!(WorldFile::from_text(&wrong_side.to_text().unwrap()).is_err());

        let mut inside_out = header;
        inside_out.last_chunk = [-1, 0, 0];
        assert!(WorldFile::from_text(&inside_out.to_text().unwrap()).is_err());

        assert!(WorldFile::from_text("not a header").is_err());
    }

    #[test]
    fn the_header_counts_its_chunks_and_regions() {
        let header = WorldFile::new(SEED, SEED);
        assert_eq!(header.chunk_count(), 256);
        assert_eq!(header.chunk_positions().len(), 256);
        assert_eq!(header.chunk_positions()[0], ChunkPos::new(0, 0, 0));
        assert_eq!(header.chunk_positions()[255], ChunkPos::new(7, 3, 7));
        assert_eq!(header.region_positions(),
                   vec![RegionPos::new(0, 0, 0), RegionPos::new(1, 0, 0),
                        RegionPos::new(0, 0, 1), RegionPos::new(1, 0, 1)]);
        assert!(header.contains(ChunkPos::new(7, 3, 7)));
        assert!(!header.contains(ChunkPos::new(8, 0, 0)));
        assert!(!header.contains(ChunkPos::new(0, -1, 0)));
    }

    #[test]
    fn the_files_are_the_header_the_blocks_and_four_regions() {
        let world = VoxelWorld::generate(SEED, SEED);
        let files = world.files(Path::new("/tmp/world")).unwrap();

        let names: Vec<String> = files.iter()
            .map(|file| file.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["world.json", "blocks.json", "r.0.0.0.rgn", "r.1.0.0.rgn",
                               "r.0.0.1.rgn", "r.1.0.1.rgn"]);
        assert_eq!(files[0].path, PathBuf::from("/tmp/world/world.json"));

        // Each region: 16 ground chunks of 64 KiB and 48 sky chunks of 3
        // bytes, plus the header.
        for file in &files[2..] {
            assert_eq!(file.contents.len(), 16 + 16 * 65_537 + 48 * 3);
            let (_, chunks) = region_file::from_bytes(&file.contents).unwrap();
            assert_eq!(chunks.len(), 64);
        }
    }

    #[test]
    fn what_the_files_hold_is_the_world() {
        let world = VoxelWorld::generate(SEED, SEED);
        let files = world.files(Path::new("/tmp/world")).unwrap();

        let (region, chunks) = region_file::from_bytes(&files[2].contents).unwrap();
        assert_eq!(region, RegionPos::new(0, 0, 0));
        for (position, chunk) in region.chunks().into_iter().zip(chunks) {
            assert_eq!(world.chunk(position), Some(&chunk));
        }
    }
}