//! File:     stratum-game/src/chunk.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! A chunk: 32 x 32 x 32 blocks, the unit the world is held, generated
//! and saved in.  A block is a 2-byte id, and what the id means (its name,
//! its color) is blocks.rs's business.  0 is always air.
//!
//! Most chunks are all one thing.  In a flat world the ground is one row of
//! chunks and everything above it is sky, so a chunk that is all air or all
//! stone is kept as a single id and not 64 KiB of the same number.  That's
//! the two kinds below: Uniform and Full.  A Uniform chunk turns into a Full
//! one the first time a block in it is changed, and `pack()` turns a Full
//! one back if every block in it turns out to be the same.  tick-sim's rule
//! 13.
//!
//! Positions come in two kinds and this file keeps them apart: a block
//! position is a whole-number place in the world (x, y, z, one block each,
//! Y up), and a ChunkPos is which chunk, which is the block position divided
//! by 32, rounded down.  Inside a chunk a block is 0 to 31 on each axis.
//!
//! Nothing in here touches a file.

/// Blocks along one side of a chunk.
pub const CHUNK_SIDE: usize = 32;

/// Blocks in a chunk.  32 x 32 x 32.
pub const BLOCKS_PER_CHUNK: usize = CHUNK_SIDE * CHUNK_SIDE * CHUNK_SIDE;

/// A block id.  Two bytes, so 65,536 kinds of block, which is more than
/// anybody will draw.
pub type BlockId = u16;

/// The one id that means nothing is there.
pub const AIR: BlockId = 0;

// ---------------------------------------------------------------------------
// Where a chunk is
// ---------------------------------------------------------------------------

/// Which chunk, counted in chunks.  The chunk holding block (40, 15, 3) is
/// (1, 0, 0).  Signed, so the world can grow past the origin later, even
/// though today's world starts at 0 on every axis.
// Rust note: `Hash` and `Eq` are what let a ChunkPos be the key of a
// HashMap, which is how the world finds a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl ChunkPos {
    pub fn new(x: i32, y: i32, z: i32) -> ChunkPos {
        ChunkPos { x, y, z }
    }

    /// The chunk a block position is in.
    // Rust note: `div_euclid` rounds toward minus infinity, so -1 / 32 is
    // -1, the chunk just below zero.  C's `/` rounds toward zero and would
    // say 0, which is the wrong chunk.  Nothing is negative in today's
    // world, but the maths is right for when something is.
    pub fn of_block(x: i32, y: i32, z: i32) -> ChunkPos {
        let side = CHUNK_SIDE as i32;
        ChunkPos {
            x: x.div_euclid(side),
            y: y.div_euclid(side),
            z: z.div_euclid(side),
        }
    }

    /// The block position of this chunk's corner: the lowest x, y and z in
    /// it.  Chunk (1, 0, 2) starts at block (32, 0, 64).
    pub fn origin(&self) -> (i32, i32, i32) {
        let side = CHUNK_SIDE as i32;
        (self.x * side, self.y * side, self.z * side)
    }
}

/// Where a block position lands inside its chunk: 0 to 31 on each axis.
// Rust note: `rem_euclid` is the remainder that goes with `div_euclid`, so
// -1 comes out as 31, the last block of the chunk below.
pub fn block_in_chunk(x: i32, y: i32, z: i32) -> (usize, usize, usize) {
    let side = CHUNK_SIDE as i32;
    (x.rem_euclid(side) as usize, y.rem_euclid(side) as usize, z.rem_euclid(side) as usize)
}

/// Where a block inside a chunk sits in the chunk's array, and in a region
/// file.  y slowest, then z, then x, so one horizontal layer of the chunk
/// is 1,024 ids in a row.  The client reads the file in the same order.
pub fn index_in_chunk(x: usize, y: usize, z: usize) -> usize {
    (y * CHUNK_SIDE + z) * CHUNK_SIDE + x
}

// ---------------------------------------------------------------------------
// The chunk itself
// ---------------------------------------------------------------------------

/// A chunk's blocks, one of two ways.
// Rust note: `Box<[BlockId; BLOCKS_PER_CHUNK]>` is a 64 KiB array kept on
// the heap, with the Box being the pointer to it.  Putting the array in the
// enum directly would make every chunk 64 KiB, Uniform ones included, and
// the whole point of Uniform is that it isn't.
#[derive(Debug, Clone, PartialEq)]
pub enum Chunk {
    /// Every block in the chunk is this one.
    Uniform(BlockId),
    /// One id per block, in `index_in_chunk()` order.
    Full(Box<[BlockId; BLOCKS_PER_CHUNK]>),
}

impl Chunk {
    /// A chunk that is all one block.  What every chunk starts as.
    pub fn filled_with(block: BlockId) -> Chunk {
        Chunk::Uniform(block)
    }

    /// The block at (x, y, z) inside the chunk, each 0 to 31.
    pub fn block(&self, x: usize, y: usize, z: usize) -> BlockId {
        match self {
            Chunk::Uniform(block) => *block,
            Chunk::Full(blocks) => blocks[index_in_chunk(x, y, z)],
        }
    }

    /// Sets the block at (x, y, z) inside the chunk, each 0 to 31.  A
    /// Uniform chunk being set to the block it already is stays Uniform.
    /// Otherwise it becomes Full first, which is the 64 KiB moment.
    pub fn set_block(&mut self, x: usize, y: usize, z: usize, block: BlockId) {
        if let Chunk::Uniform(current) = self {
            if *current == block {
                return;
            }
            *self = Chunk::Full(Box::new([*current; BLOCKS_PER_CHUNK]));
        }
        if let Chunk::Full(blocks) = self {
            blocks[index_in_chunk(x, y, z)] = block;
        }
    }

    /// Turns a Full chunk whose blocks are all the same back into a Uniform
    /// one.  A Uniform chunk is left alone.  The generator calls this on
    /// every chunk it finishes, and a chunk that was dug into and filled
    /// back can be packed the same way later.
    pub fn pack(&mut self) {
        if let Chunk::Full(blocks) = self {
            let first = blocks[0];
            if blocks.iter().all(|block| *block == first) {
                *self = Chunk::Uniform(first);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const STONE: BlockId = 1;
    const GRASS: BlockId = 3;

    #[test]
    fn the_index_runs_x_then_z_then_y() {
        assert_eq!(index_in_chunk(0, 0, 0), 0);
        assert_eq!(index_in_chunk(1, 0, 0), 1);
        assert_eq!(index_in_chunk(0, 0, 1), 32);
        assert_eq!(index_in_chunk(0, 1, 0), 1024);
        assert_eq!(index_in_chunk(31, 31, 31), BLOCKS_PER_CHUNK - 1);
    }

    #[test]
    fn a_block_position_finds_its_chunk_and_its_place_in_it() {
        assert_eq!(ChunkPos::of_block(0, 0, 0), ChunkPos::new(0, 0, 0));
        assert_eq!(ChunkPos::of_block(31, 15, 31), ChunkPos::new(0, 0, 0));
        assert_eq!(ChunkPos::of_block(32, 15, 64), ChunkPos::new(1, 0, 2));
        assert_eq!(ChunkPos::of_block(255, 127, 255), ChunkPos::new(7, 3, 7));
        assert_eq!(block_in_chunk(40, 15, 3), (8, 15, 3));
        assert_eq!(block_in_chunk(255, 127, 255), (31, 31, 31));
    }

    #[test]
    fn negative_positions_round_down_not_toward_zero() {
        assert_eq!(ChunkPos::of_block(-1, 0, -33), ChunkPos::new(-1, 0, -2));
        assert_eq!(block_in_chunk(-1, 0, -33), (31, 0, 31));
    }

    #[test]
    fn a_chunk_knows_its_corner() {
        assert_eq!(ChunkPos::new(0, 0, 0).origin(), (0, 0, 0));
        assert_eq!(ChunkPos::new(1, 0, 2).origin(), (32, 0, 64));
        assert_eq!(ChunkPos::new(-1, 0, 0).origin(), (-32, 0, 0));
    }

    #[test]
    fn a_uniform_chunk_stays_uniform_until_a_block_differs() {
        let mut chunk = Chunk::filled_with(STONE);
        assert_eq!(chunk.block(5, 5, 5), STONE);

        chunk.set_block(5, 5, 5, STONE);
        assert_eq!(chunk, Chunk::Uniform(STONE));

        chunk.set_block(5, 5, 5, GRASS);
        assert!(matches!(chunk, Chunk::Full(_)));
        assert_eq!(chunk.block(5, 5, 5), GRASS);
        assert_eq!(chunk.block(5, 6, 5), STONE);
        assert_eq!(chunk.block(0, 0, 0), STONE);
    }

    #[test]
    fn packing_folds_a_full_chunk_that_is_all_one_block() {
        let mut chunk = Chunk::filled_with(AIR);
        chunk.set_block(0, 0, 0, STONE);
        chunk.pack();
        assert!(matches!(chunk, Chunk::Full(_)));

        chunk.set_block(0, 0, 0, AIR);
        chunk.pack();
        assert_eq!(chunk, Chunk::Uniform(AIR));

        // Packing a Uniform chunk is nothing.
        chunk.pack();
        assert_eq!(chunk, Chunk::Uniform(AIR));
    }
}