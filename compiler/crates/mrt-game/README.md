# MRT-Game

The 2D foundation: a framebuffer and the drawing it understands.

It is an **engine extension**, not part of the language: only the Rust engines
have it, a program using it is not portable, and it is deliberately outside the
conformance corpus. See "Engine extensions" in `docs/LANGUAGE_SPEC.md`.

## What is here, and what is deliberately not

`lib.rs` is a `Surface`: a rectangle of packed `0xAARRGGBB` pixels, with
clipped, integer drawing — pixels, lines, rectangles and their outlines,
circles filled and hollow, alpha blending, and blitting one surface onto
another. `png.rs` writes a frame out as a real PNG.

**There is no window.** Nothing here opens a display or reads a key, and that
is the split the whole design rests on: everything a 2D game draws is
arithmetic on an array, and arithmetic on an array can be tested on a machine
with no screen at all — which is exactly what continuous integration is. The
test suite draws thousands of shapes and checks the pixels; a person runs the
same program and looks at a PNG. When a window arrives it will present a
surface this crate already got right.

## PNG, both ways

Writing a PNG normally means a dependency, and this workspace's manifest says
in its first line that it has none.

PNG does not actually require compression. Its data stream is zlib, and
deflate has a *stored* block type: a length, then the bytes. So a valid PNG is
a header, the rows with a filter byte each, wrapped in stored blocks, with a
CRC-32 per chunk and an Adler-32 over the whole. Every decoder reads it,
because there is nothing unusual about it.

The cost is bounded and honest: a file about the size of the raw pixels. If
images ever need to be small, that half of `png.rs` is the one thing to
replace.

*Reading* is not symmetrical. A PNG from anywhere else is compressed with
Huffman coding, so a decoder that only understood stored blocks would reject
every real image in the world — which is why `inflate.rs` implements the whole
of DEFLATE: stored blocks, the fixed tables, and dynamic Huffman with its code
lengths encoded in a Huffman code of their own. It is checked against vectors
from an independent compressor, because nothing this crate writes would
exercise any of it.

## From MRT

```mrt
func main() {
    gameInit(400, 240, "MRT Shapes");
    gameClear(gameColor(18, 22, 38));
    gameRect(40, 60, 120, 140, gameColor(72, 168, 224));
    gameCircle(300, 90, 40, gameColorAlpha(255, 214, 92, 140));
    gameSave("frame.png");
}
```

Run it with:

    cargo run --manifest-path compiler/Cargo.toml --bin mrt-run -- examples/game_shapes.mrt

`docs/LANGUAGE_SPEC.md` lists every built-in. The bridge is
`compiler/crates/mrt-interp/src/game.rs`, and the decision worth reading there
is why the screen is *not* an MRT value when a model is.

## The window

`mrt-window` is a separate crate, and the only one here with dependencies. A
program reaches it through `gameOpen`, `gamePresent`, `gameKeyDown` and
friends, with the loop written in MRT rather than owned by an engine:

```mrt
while (gameOpen()) {
    gameClear(sky);
    gameCircle(ball.x, ball.y, 16, gold);
    gamePresent();
}
```

The window opens on the first `gameOpen`, not at `gameInit`, so a program
that only draws and saves a PNG needs no display at all — and asking for a
window where there is none is a catchable error rather than a crash, which is
what lets `examples/game_bounce.mrt` run both on a desktop and on a build
server and print the same numbers either way.

## The font

`font.rs` is a 5x7 bitmap font for printable ASCII, and each glyph is written
as the shape it draws:

```rust
('A',  [" ### ", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"]),
```

A bitmap font is normally a wall of hex, which is compact, unreadable, and
impossible to check by eye — one wrong nibble is a letter with a hole in it
and you find out by rendering. Five bytes a row instead of one, in a constant,
in a program about to allocate a megabyte for a framebuffer, is no cost at
all. What it buys is that a reader can check the font without running it, and
can add a glyph by drawing one.

## Roadmap

Done: the surface, the drawing primitives, PNG output, the `game*` built-ins,
a window, input, a frame loop, text, sprites, collision, and loading art
from PNG files.

Sprites too: `gameSurface` makes an offscreen surface, `gameTarget` points
drawing at it, and `gameDraw` stamps it. Ids are plain numbers, so the
language still learns no new type.

`collide.rs` holds the box maths: overlap, resolution, and a swept test.
Only the last of those earns its place in an engine — overlap is four lines a
program can write, but a fast mover crossing a thin wall between two frames
overlaps it on no frame at all, and no better overlap test fixes that.

## The command

    mrt-game new pong     start a game in a new directory
    mrt-game run          play it
    mrt-game check        parse and resolve it without running

`new` scaffolds a complete, playable breakout — a loop, real elapsed time,
input, swept collision and text — rather than a stub that prints "hello". The
first edit anyone makes is then to a program that already works.

There is no `build`: MRT is interpreted, so it would be a lie or a synonym for
`check`.

`mrt-game package` flattens a game and its imports into one file. Each module
becomes a function returning its exports, so two modules may both define
`helper` and nothing needs renaming. The delicate part is hoisting — MRT
hoists declarations at the top level and nowhere else, so a module moved into
a function body would lose every forward reference in it, and the packager
emits declarations first to reproduce that by construction.

Next: audio. A game with no sound is the last obvious gap.
