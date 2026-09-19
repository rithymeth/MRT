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

## The PNG encoder

Writing a PNG normally means a dependency, and this workspace's manifest says
in its first line that it has none.

PNG does not actually require compression. Its data stream is zlib, and
deflate has a *stored* block type: a length, then the bytes. So a valid PNG is
a header, the rows with a filter byte each, wrapped in stored blocks, with a
CRC-32 per chunk and an Adler-32 over the whole. Every decoder reads it,
because there is nothing unusual about it.

The cost is bounded and honest: a file about the size of the raw pixels. If
images ever need to be small, `png.rs` is the one file to replace.

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

## Roadmap

Done: the surface, the drawing primitives, PNG output, and the `game*`
built-ins that reach them.

Next, and in this order: a window and a frame loop — one problem, not two,
since a window that does not pump its event queue is a frozen window. Then
input, sprites and text; then collision; then the CLI that packages a game.
