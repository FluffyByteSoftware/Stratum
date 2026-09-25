//! File:     stratum-game/src/region_file.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! The region file: 4 x 4 x 4 chunks, 64 in all, as bytes on the disk.
//! One file per chunk was tick-sim's mistake (about 90 synced files a second
//! was the spinning drive's ceiling, and it fell a minute behind), and 64
//! to a file kept up on both drives.  So chunks are saved in these.
//!
//! The client reads the same file, so the bytes are written down here, and
//! nowhere else.  Little-endian everywhere, like the protocol.
//!
//! ```text
//! Header, 16 bytes
//!   0   "STRG"            4 bytes    so a wrong file is refused early
//!   4   format            u16        1
//!   6   region x, y, z    i16 x 3    in regions (chunk coordinate / 4)
//!   12  region_side       u8         4
//!   13  chunk_side        u8         32
//!   14  reserved          u16        0
//!
//! Then 64 chunks, in order: y slowest, then z, then x, so the chunk at
//! (cx, cy, cz) inside the region is number (cy * 4 + cz) * 4 + cx.  Each:
//!
//!   kind   u8    0 = the whole chunk is one block, 1 = full
//!   kind 0:  block id                        u16       3 bytes in all
//!   kind 1:  32 x 32 x 32 block ids          u16 each  65,537 bytes in all
//!            in chunk.rs's order: (y * 32 + z) * 32 + x
//! ```
//!
//! No table of offsets.  A reader walks the 64 chunks, and a 3-byte one is
//! skipped as fast as it's read.  The file is exactly as long as its chunks
//! add up to; a byte more or less and it's refused.
//!
//! Nothing in here touches a file.  The world hands the bytes to DiskMan
//! and gets them back from it.

use crate::chunk::{BLOCKS_PER_CHUNK, BlockId, CHUNK_SIDE, Chunk, ChunkPos};

/// Chunks along one side of a region.
pub const REGION_SIDE: usize = 4;

/// Chunks in a region.  4 x 4 x 4.
pub const CHUNKS_PER_REGION: usize = REGION_SIDE * REGION_SIDE * REGION_SIDE;

/// The file's extension, without the dot.
pub const REGION_EXTENSION: &str = "rgn";

/// The version of the bytes above.  It goes in every file and in
/// world.json, and a reader that doesn't know it stops there.
pub const FORMAT: u16 = 1;

/// The first four bytes of every region file.
const MAGIC: &[u8; 4] = b"STRG";

const HEADER_LEN: usize = 16;

const KIND_UNIFORM: u8 = 0;
const KIND_FULL: u8 = 1;

// ---------------------------------------------------------------------------
// Where a region is
// ---------------------------------------------------------------------------

/// Which region, counted in regions.  The region holding chunk (5, 0, 3)
/// is (1, 0, 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl RegionPos {
    pub fn new(x: i32, y: i32, z: i32) -> RegionPos {
        RegionPos { x, y, z }
    }

    /// The region a chunk is in.  Rounds down, like ChunkPos::of_block().
    pub fn of_chunk(chunk: ChunkPos) -> RegionPos {
        let side = REGION_SIDE as i32;
        RegionPos {
            x: chunk.x.div_euclid(side),
            y: chunk.y.div_euclid(side),
            z: chunk.z.div_euclid(side),
        }
    }

    /// The file's name: `r.<x>.<y>.<z>.rgn`, with a minus sign where a
    /// coordinate has one.
    pub fn file_name(&self) -> String {
        format!("r.{}.{}.{}.{}", self.x, self.y, self.z, REGION_EXTENSION)
    }

    /// The chunk at a place in the file, counting from 0 in file order.
    /// The other half of `index_in_region()`.
    pub fn chunk_at(&self, index: usize) -> ChunkPos {
        let side = REGION_SIDE;
        let cx = index % side;
        let cz = (index / side) % side;
        let cy = index / (side * side);
        ChunkPos::new(self.x * side as i32 + cx as i32,
                      self.y * side as i32 + cy as i32,
                      self.z * side as i32 + cz as i32)
    }

    /// Every chunk in the region, in file order.
    pub fn chunks(&self) -> Vec<ChunkPos> {
        (0..CHUNKS_PER_REGION).map(|index| self.chunk_at(index)).collect()
    }
}

/// Where a chunk sits in its region's file, counting from 0: y slowest,
/// then z, then x.
pub fn index_in_region(chunk: ChunkPos) -> usize {
    let side = REGION_SIDE as i32;
    let cx = chunk.x.rem_euclid(side) as usize;
    let cy = chunk.y.rem_euclid(side) as usize;
    let cz = chunk.z.rem_euclid(side) as usize;
    (cy * REGION_SIDE + cz) * REGION_SIDE + cx
}

// ---------------------------------------------------------------------------
// Bytes out
// ---------------------------------------------------------------------------

/// A region's 64 chunks as the bytes of its file.  `chunks` is in file
/// order (`RegionPos::chunks()` gives the positions in that order), and
/// there have to be exactly 64 of them.
pub fn to_bytes(region: RegionPos, chunks: &[&Chunk]) -> Result<Vec<u8>, String> {
    if chunks.len() != CHUNKS_PER_REGION {
        return Err(format!("A region file holds {} chunks, and {} were handed in for {}.",
                           CHUNKS_PER_REGION, chunks.len(), region.file_name()));
    }
    let (x, y, z) = match (i16::try_from(region.x), i16::try_from(region.y), i16::try_from(region.z)) {
        (Ok(x), Ok(y), Ok(z)) => (x, y, z),
        _ => return Err(format!("Region ({}, {}, {}) is too far out for the file's header.",
                                region.x, region.y, region.z)),
    };

    let mut bytes = Vec::with_capacity(HEADER_LEN + CHUNKS_PER_REGION * 3);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&FORMAT.to_le_bytes());
    bytes.extend_from_slice(&x.to_le_bytes());
    bytes.extend_from_slice(&y.to_le_bytes());
    bytes.extend_from_slice(&z.to_le_bytes());
    bytes.push(REGION_SIDE as u8);
    bytes.push(CHUNK_SIDE as u8);
    bytes.extend_from_slice(&0u16.to_le_bytes());

    for chunk in chunks {
        match chunk {
            Chunk::Uniform(block) => {
                bytes.push(KIND_UNIFORM);
                bytes.extend_from_slice(&block.to_le_bytes());
            }
            Chunk::Full(blocks) => {
                bytes.push(KIND_FULL);
                bytes.reserve(BLOCKS_PER_CHUNK * 2);
                for block in blocks.iter() {
                    bytes.extend_from_slice(&block.to_le_bytes());
                }
            }
        }
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Bytes in
// ---------------------------------------------------------------------------

/// The bytes of a region file, checked and turned back into which region
/// it is and its 64 chunks, in file order.  An `Err` says what's wrong
/// with the bytes, in words, and doesn't say which file: the caller knows
/// that and adds it.
pub fn from_bytes(bytes: &[u8]) -> Result<(RegionPos, Vec<Chunk>), String> {
    if bytes.len() < HEADER_LEN {
        return Err(format!("it is {} bytes long, shorter than the header", bytes.len()));
    }
    if &bytes[0..4] != MAGIC {
        return Err("it doesn't start with STRG, so it isn't a region file".to_string());
    }
    let format = u16::from_le_bytes([bytes[4], bytes[5]]);
    if format != FORMAT {
        return Err(format!("it is format {}, and this server reads format {}", format, FORMAT));
    }
    let region = RegionPos::new(i16::from_le_bytes([bytes[6], bytes[7]]) as i32,
                                i16::from_le_bytes([bytes[8], bytes[9]]) as i32,
                                i16::from_le_bytes([bytes[10], bytes[11]]) as i32);
    if bytes[12] as usize != REGION_SIDE || bytes[13] as usize != CHUNK_SIDE {
        return Err(format!("it says {} chunks a side of {} blocks, and this server uses {} and {}",
                           bytes[12], bytes[13], REGION_SIDE, CHUNK_SIDE));
    }

    let mut chunks = Vec::with_capacity(CHUNKS_PER_REGION);
    let mut at = HEADER_LEN;
    for number in 0..CHUNKS_PER_REGION {
        let kind = match bytes.get(at) {
            Some(kind) => *kind,
            None => return Err(format!("it ends after {} of {} chunks", number, CHUNKS_PER_REGION)),
        };
        at += 1;

        match kind {
            KIND_UNIFORM => {
                let block = match read_block(bytes, at) {
                    Some(block) => block,
                    None => return Err(format!("it ends inside chunk {}", number)),
                };
                at += 2;
                chunks.push(Chunk::Uniform(block));
            }
            KIND_FULL => {
                let mut blocks = Box::new([0 as BlockId; BLOCKS_PER_CHUNK]);
                for slot in blocks.iter_mut() {
                    match read_block(bytes, at) {
                        Some(block) => *slot = block,
                        None => return Err(format!("it ends inside chunk {}", number)),
                    }
                    at += 2;
                }
                chunks.push(Chunk::Full(blocks));
            }
            other => return Err(format!("chunk {} is of kind {}, which isn't one", number, other)),
        }
    }

    if at != bytes.len() {
        return Err(format!("it has {} bytes left over after the last chunk", bytes.len() - at));
    }
    Ok((region, chunks))
}

/// The block id at `at`, or `None` if the bytes run out there.
fn read_block(bytes: &[u8], at: usize) -> Option<BlockId> {
    match (bytes.get(at), bytes.get(at + 1)) {
        (Some(low), Some(high)) => Some(u16::from_le_bytes([*low, *high])),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::AIR;

    const STONE: BlockId = 1;
    const GRASS: BlockId = 3;

    /// 64 air chunks, with one of them Full and one of them stone.
    fn a_mixed_region() -> Vec<Chunk> {
        let mut chunks = vec![Chunk::filled_with(AIR); CHUNKS_PER_REGION];
        chunks[5] = Chunk::filled_with(STONE);
        chunks[9].set_block(3, 15, 7, GRASS);
        chunks
    }

    #[test]
    fn a_chunk_finds_its_region_and_its_place_in_the_file() {
        assert_eq!(RegionPos::of_chunk(ChunkPos::new(0, 0, 0)), RegionPos::new(0, 0, 0));
        assert_eq!(RegionPos::of_chunk(ChunkPos::new(5, 0, 3)), RegionPos::new(1, 0, 0));
        assert_eq!(RegionPos::of_chunk(ChunkPos::new(7, 3, 7)), RegionPos::new(1, 0, 1));
        assert_eq!(RegionPos::of_chunk(ChunkPos::new(-1, 0, 0)), RegionPos::new(-1, 0, 0));

        assert_eq!(index_in_region(ChunkPos::new(0, 0, 0)), 0);
        assert_eq!(index_in_region(ChunkPos::new(1, 0, 0)), 1);
        assert_eq!(index_in_region(ChunkPos::new(0, 0, 1)), 4);
        assert_eq!(index_in_region(ChunkPos::new(0, 1, 0)), 16);
        assert_eq!(index_in_region(ChunkPos::new(7, 3, 7)), 63);
        assert_eq!(index_in_region(ChunkPos::new(-1, 0, 0)), 3);
    }

    #[test]
    fn the_file_order_goes_there_and_back() {
        let region = RegionPos::new(1, 0, -1);
        let chunks = region.chunks();
        assert_eq!(chunks.len(), CHUNKS_PER_REGION);
        assert_eq!(chunks[0], ChunkPos::new(4, 0, -4));
        assert_eq!(chunks[63], ChunkPos::new(7, 3, -1));
        for (index, chunk) in chunks.iter().enumerate() {
            assert_eq!(index_in_region(*chunk), index);
            assert_eq!(RegionPos::of_chunk(*chunk), region);
        }
    }

    #[test]
    fn file_names() {
        assert_eq!(RegionPos::new(0, 0, 0).file_name(), "r.0.0.0.rgn");
        assert_eq!(RegionPos::new(-1, 0, 1).file_name(), "r.-1.0.1.rgn");
    }

    #[test]
    fn what_we_write_we_can_read() {
        let region = RegionPos::new(1, 0, 1);
        let chunks = a_mixed_region();
        let refs: Vec<&Chunk> = chunks.iter().collect();

        let bytes = to_bytes(region, &refs).unwrap();
        // 16 for the header, 63 chunks of 3 bytes, and one full one.
        assert_eq!(bytes.len(), HEADER_LEN + 63 * 3 + 1 + BLOCKS_PER_CHUNK * 2);

        let (back_region, back) = from_bytes(&bytes).unwrap();
        assert_eq!(back_region, region);
        assert_eq!(back, chunks);
        assert_eq!(back[9].block(3, 15, 7), GRASS);
    }

    #[test]
    fn the_header_in_bytes() {
        let chunks = vec![Chunk::filled_with(AIR); CHUNKS_PER_REGION];
        let refs: Vec<&Chunk> = chunks.iter().collect();
        let bytes = to_bytes(RegionPos::new(1, -1, 2), &refs).unwrap();

        assert_eq!(&bytes[0..HEADER_LEN],
                   &[b'S', b'T', b'R', b'G', 0x01, 0x00, 0x01, 0x00, 0xFF, 0xFF, 0x02, 0x00, 4, 32, 0, 0]);
        // The first chunk: kind 0, then air.
        assert_eq!(&bytes[HEADER_LEN..HEADER_LEN + 3], &[0, 0, 0]);
    }

    #[test]
    fn the_wrong_number_of_chunks_is_refused() {
        let chunks = vec![Chunk::filled_with(AIR); CHUNKS_PER_REGION - 1];
        let refs: Vec<&Chunk> = chunks.iter().collect();
        assert!(to_bytes(RegionPos::new(0, 0, 0), &refs).is_err());
    }

    #[test]
    fn bad_bytes_are_refused() {
        let chunks = a_mixed_region();
        let refs: Vec<&Chunk> = chunks.iter().collect();
        let good = to_bytes(RegionPos::new(0, 0, 0), &refs).unwrap();

        assert!(from_bytes(&good[..10]).is_err());

        let mut wrong_magic = good.clone();
        wrong_magic[0] = b'X';
        assert!(from_bytes(&wrong_magic).is_err());

        let mut wrong_format = good.clone();
        wrong_format[4] = 2;
        assert!(from_bytes(&wrong_format).is_err());

        let mut wrong_side = good.clone();
        wrong_side[13] = 16;
        assert!(from_bytes(&wrong_side).is_err());

        let mut wrong_kind = good.clone();
        wrong_kind[HEADER_LEN] = 7;
        assert!(from_bytes(&wrong_kind).is_err());

        // Cut off inside the last chunk.
        assert!(from_bytes(&good[..good.len() - 1]).is_err());

        let mut too_long = good.clone();
        too_long.push(0);
        assert!(from_bytes(&too_long).is_err());
    }
}