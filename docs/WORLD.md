# The world on disk

What a client needs to draw the world Stratum serves: the folder it comes in, the three kinds of file in it, and the bytes of a region file.  The server writes this folder once, when it first launches, and reads it back every time it starts.  The client reads the same folder.  Nothing in it changes while the server runs, yet.

The server is the authority.  What a client reads here is a copy for showing, and the server decides where the walls are.

## The folder

`saved/world/` inside the server's content folder (`/opt/stratum/content/saved/world/` by default).

```
saved/world/
├── world.json          The header: the shape of the world and its seed.
├── blocks.json         The block types: id, name, color.
└── r.0.0.0.rgn         Region files, one per 4 x 4 x 4 chunks, named by
    r.1.0.0.rgn         the region's coordinates.  Four for the first world.
    r.0.0.1.rgn
    r.1.0.1.rgn
```

## The layout

- A **block** is one cubic metre.  Y is up.  A block position is three whole numbers.
- A **chunk** is 32 x 32 x 32 blocks.  The chunk holding block (x, y, z) is (x / 32, y / 32, z / 32), rounded down, and the block sits at (x mod 32, y mod 32, z mod 32) inside it.  Today's world starts at 0 on every axis, so plain integer division does it.  If the world ever grows past the origin, "rounded down" matters: `floori(x / 32.0)` and `posmod(x, 32)` in GDScript, since a plain `/` and `%` round toward zero.
- A **region** is 4 x 4 x 4 chunks, 64 in all, and one file.  Chunk (cx, cy, cz) is in region (cx / 4, cy / 4, cz / 4), rounded down the same way.
- The **first world** is chunks (0, 0, 0) to (7, 3, 7), included: 256 blocks square and 128 tall.  The ground is at y = 15 (the top solid block), so a player standing on it has their feet at y = 16.  One column in 16 is a block higher and one in 16 a block lower, so the ground isn't one flat sheet.  Grass on top, three blocks of dirt, stone to the bottom, air above.  The middle of the map is (128, 16, 128).

## world.json

```json
{
  "format": 1,
  "seed": 1790298578,
  "chunk_side": 32,
  "region_side": 4,
  "first_chunk": [0, 0, 0],
  "last_chunk": [7, 3, 7],
  "generated_at": 1790298578
}
```

`format` is the region file's version below; a reader that doesn't know it should stop there.  `first_chunk` and `last_chunk` are the world's bounds in chunks, both included.  `seed` is what the generator was seeded with, and `generated_at` is seconds since 1970, UTC.  The two are the same number today because the seed is the time.

## blocks.json

```json
{
  "blocks": [
    { "id": 0, "name": "air",   "color": null },
    { "id": 1, "name": "stone", "color": "7A7A7A" },
    { "id": 2, "name": "dirt",  "color": "6B4A2B" },
    { "id": 3, "name": "grass", "color": "4F9A3A" }
  ]
}
```

A block is drawn in its one diffuse color, every face the same, no texture.  The color is `RRGGBB` in hex, the way a web page writes it (`Color.html()` in Godot takes it as it is).  Id 0 is always air, has no color, and isn't drawn.  A block will grow more fields than this later (solid or not, how hard it is to dig), added on the end; a reader should ignore fields it doesn't know.

## The region file

Little-endian everywhere.

```
Header, 16 bytes
  0   "STRG"            4 bytes    magic, so a wrong file is refused early
  4   format            u16        1
  6   region x, y, z    i16 x 3    in regions (chunk coordinate / 4)
  12  region_side       u8         4
  13  chunk_side        u8         32
  14  reserved          u16        0

Then 64 chunks, in order: y slowest, then z, then x, so the chunk at
(cx, cy, cz) inside the region is number (cy * 4 + cz) * 4 + cx.  Each:

  kind   u8    0 = the whole chunk is one block, 1 = full
  kind 0:  block id                        u16       3 bytes in all
  kind 1:  32 x 32 x 32 block ids          u16 each  65,537 bytes in all
           in the same order: block (x, y, z) inside the chunk is
           number (y * 32 + z) * 32 + x
```

There is no table of offsets.  A reader walks the 64 chunks in order, and a 3-byte one is skipped as fast as it's read.  The file is exactly as long as its chunks add up to.

A chunk that is all one block (all air, all stone) is kind 0, so it costs 3 bytes instead of 64 KiB.  In the first world every chunk with the ground in it is kind 1 and everything above is kind 0, so each region file is 1,048,752 bytes: the header, 16 full chunks, 48 of one block.

The header of `r.1.0.0.rgn`, byte by byte:

```
53 54 52 47   "STRG"
01 00         format 1
01 00         region x = 1
00 00         region y = 0
00 00         region z = 0
04            4 chunks a side
20            32 blocks a side
00 00         reserved
```

Then the first chunk, at offset 16: `00 01 00` is kind 0, block 1, a chunk of solid stone; `01` followed by 65,536 bytes is a full chunk.