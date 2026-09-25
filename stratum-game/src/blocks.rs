//! File:     stratum-game/src/blocks.rs
//! Project:  Stratum Game
//! Author:   Jacob Chacko
//!
//! The block types.  A chunk holds 2-byte ids, and this file says what each
//! id is: a name, and a color.  That's all a block is for now.  The client
//! draws every face of a block in its one diffuse color, with no texture,
//! which is enough to see the ground and the bumps in it.
//!
//! The list lives in `blocks.json`, next to the region files in the world
//! folder, so it ships with the world and the client reads the same file
//! the server does:
//!
//! ```text
//! {
//!   "blocks": [
//!     { "id": 0, "name": "air",   "color": null },
//!     { "id": 1, "name": "stone", "color": "7A7A7A" },
//!     { "id": 2, "name": "dirt",  "color": "6B4A2B" },
//!     { "id": 3, "name": "grass", "color": "4F9A3A" }
//!   ]
//! }
//! ```
//!
//! A color is `RRGGBB` in hex, the way it's written in a web page, and
//! `null` for air, which has no color because it isn't drawn.  Id 0 is
//! always air.  A block will grow more fields than this (solid or not, how
//! hard it is to dig, what it drops), and a new field goes on the end with
//! a default, the way the player file does it.
//!
//! Nothing in here touches a file.  The world (world.rs) writes the text
//! out and reads it back through DiskMan.

use serde::{Deserialize, Serialize};

use crate::chunk::{AIR, BlockId};

/// The file's name inside the world folder.
pub const BLOCKS_FILE_NAME: &str = "blocks.json";

/// The ids a new world starts with, so the generator can name them.  Air
/// is chunk.rs's, since a chunk has to know it without knowing any of
/// these.
pub const STONE: BlockId = 1;
pub const DIRT: BlockId = 2;
pub const GRASS: BlockId = 3;

/// One kind of block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: BlockId,
    pub name: String,
    /// `RRGGBB` in hex, or `None` for a block that isn't drawn (air).
    pub color: Option<String>,
}

/// The whole list, as `blocks.json` holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Blocks {
    pub blocks: Vec<Block>,
}

impl Blocks {
    /// The four blocks every new world is generated with.
    pub fn starting_set() -> Blocks {
        Blocks {
            blocks: vec![
                Block { id: AIR, name: "air".to_string(), color: None },
                Block { id: STONE, name: "stone".to_string(), color: Some("7A7A7A".to_string()) },
                Block { id: DIRT, name: "dirt".to_string(), color: Some("6B4A2B".to_string()) },
                Block { id: GRASS, name: "grass".to_string(), color: Some("4F9A3A".to_string()) },
            ],
        }
    }

    /// The name of a block, or `None` for an id the list doesn't have.
    pub fn name_of(&self, id: BlockId) -> Option<&str> {
        self.blocks.iter()
            .find(|block| block.id == id)
            .map(|block| block.name.as_str())
    }

    /// The list as the text that goes on the disk: JSON, indented, like
    /// every JSON file we write.
    pub fn to_text(&self) -> Result<String, String> {
        match serde_json::to_string_pretty(self) {
            Ok(text) => Ok(text),
            Err(error) => Err(format!("Couldn't turn the block list into JSON: {}", error)),
        }
    }

    /// The text of a `blocks.json`, checked and turned back into the list.
    /// An `Err` says what's wrong with it, in words.  A list the client
    /// couldn't draw from (a color that isn't one, two blocks with one id,
    /// a 0 that isn't air) is refused here rather than found in the client.
    pub fn from_text(text: &str) -> Result<Blocks, String> {
        let list: Blocks = match serde_json::from_str(text) {
            Ok(list) => list,
            Err(error) => return Err(format!("it isn't a block list ({})", error)),
        };
        list.check()?;
        Ok(list)
    }

    /// The rules a list has to keep.
    fn check(&self) -> Result<(), String> {
        match self.blocks.iter().find(|block| block.id == AIR) {
            None => return Err("there is no block 0, and 0 has to be air".to_string()),
            Some(air) if air.name != "air" || air.color.is_some() => {
                return Err(format!("block 0 has to be air with no color, and it is \"{}\"", air.name));
            }
            Some(_) => {}
        }

        for (index, block) in self.blocks.iter().enumerate() {
            if block.name.is_empty() {
                return Err(format!("block {} has no name", block.id));
            }
            if self.blocks[..index].iter().any(|earlier| earlier.id == block.id) {
                return Err(format!("block {} is in the list twice", block.id));
            }
            if let Some(color) = &block.color {
                if !is_hex_color(color) {
                    return Err(format!("{}'s color is \"{}\", and it needs to be six hex digits like 4F9A3A",
                                       block.name, color));
                }
            }
        }
        Ok(())
    }
}

/// True for `RRGGBB`: six hex digits, either case, and nothing else.
fn is_hex_color(text: &str) -> bool {
    text.len() == 6 && text.chars().all(|c| c.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_we_write_we_can_read() {
        let list = Blocks::starting_set();
        let text = list.to_text().unwrap();
        assert_eq!(Blocks::from_text(&text), Ok(list));
    }

    #[test]
    fn the_starting_set_names_its_blocks() {
        let list = Blocks::starting_set();
        assert_eq!(list.name_of(AIR), Some("air"));
        assert_eq!(list.name_of(STONE), Some("stone"));
        assert_eq!(list.name_of(GRASS), Some("grass"));
        assert_eq!(list.name_of(99), None);
    }

    #[test]
    fn zero_has_to_be_air() {
        let no_zero = "{ \"blocks\": [ { \"id\": 1, \"name\": \"stone\", \"color\": \"7A7A7A\" } ] }";
        assert!(Blocks::from_text(no_zero).is_err());

        let painted_air = "{ \"blocks\": [ { \"id\": 0, \"name\": \"air\", \"color\": \"FFFFFF\" } ] }";
        assert!(Blocks::from_text(painted_air).is_err());

        let named_wrong = "{ \"blocks\": [ { \"id\": 0, \"name\": \"void\", \"color\": null } ] }";
        assert!(Blocks::from_text(named_wrong).is_err());
    }

    #[test]
    fn a_bad_list_is_refused() {
        let twice = "{ \"blocks\": [ { \"id\": 0, \"name\": \"air\", \"color\": null }, \
                                     { \"id\": 1, \"name\": \"stone\", \"color\": \"7A7A7A\" }, \
                                     { \"id\": 1, \"name\": \"dirt\", \"color\": \"6B4A2B\" } ] }";
        assert!(Blocks::from_text(twice).is_err());

        let bad_color = "{ \"blocks\": [ { \"id\": 0, \"name\": \"air\", \"color\": null }, \
                                         { \"id\": 1, \"name\": \"stone\", \"color\": \"grey\" } ] }";
        assert!(Blocks::from_text(bad_color).is_err());

        let no_name = "{ \"blocks\": [ { \"id\": 0, \"name\": \"air\", \"color\": null }, \
                                       { \"id\": 1, \"name\": \"\", \"color\": \"7A7A7A\" } ] }";
        assert!(Blocks::from_text(no_name).is_err());

        assert!(Blocks::from_text("this is not json").is_err());
    }

    #[test]
    fn colors_are_six_hex_digits() {
        assert!(is_hex_color("4F9A3A"));
        assert!(is_hex_color("4f9a3a"));
        assert!(!is_hex_color("#4F9A3A"));
        assert!(!is_hex_color("4F9A3"));
        assert!(!is_hex_color("GGGGGG"));
    }
}