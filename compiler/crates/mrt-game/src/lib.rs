//! The 2D foundation: a framebuffer and the drawing it understands.
//!
//! -- What this crate is not --
//!
//! It is not a window. Nothing here talks to an operating system, opens a
//! display or reads a key. That is deliberate, and it is the split the whole
//! design rests on: **everything a 2D game draws is arithmetic on an array**,
//! and arithmetic on an array can be tested on a machine with no screen at
//! all -- which is exactly what continuous integration is.
//!
//! So a `Surface` is the picture, and showing it is somebody else's problem.
//! The test suite draws thousands of shapes and checks the pixels; a person
//! runs the same program and looks at a PNG. Neither needs a window, and when
//! a window arrives it renders a surface this crate already got right.
//!
//! -- Conventions, chosen once --
//!
//! Y points **down** and the origin is the top-left corner, which is what
//! every framebuffer, image format and mouse position already does. A colour
//! is packed `0xAARRGGBB` in a `u32`, the layout a window expects to be
//! handed, so presenting a frame later is a copy rather than a conversion.
//!
//! Coordinates are **signed**, and everything clips. A game draws off the
//! edge of the screen constantly -- that is what it means for something to
//! walk out of shot -- so an off-screen rectangle is ordinary, not an error,
//! and must no more panic than it should wrap around to the other side.

pub mod collide;
pub mod font;
pub mod png;

use std::io;
use std::path::Path;

/// A colour, packed `0xAARRGGBB`.
///
/// A plain number rather than four fields, because it crosses into MRT as one
/// value and sits in a variable there: `var RED = gameColor(255, 0, 0);`
/// reads better than carrying an object around a draw loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Color(pub u32);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::rgba(r, g, b, 255)
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    pub const fn alpha(self) -> u8 {
        (self.0 >> 24) as u8
    }
    pub const fn red(self) -> u8 {
        (self.0 >> 16) as u8
    }
    pub const fn green(self) -> u8 {
        (self.0 >> 8) as u8
    }
    pub const fn blue(self) -> u8 {
        self.0 as u8
    }

    /// Source-over compositing: `self` drawn on top of `under`.
    ///
    /// The arithmetic is integer throughout. `(x * a + y * (255 - a) + 127) /
    /// 255` is the rounded average; the `+ 127` is what stops a long run of
    /// blends drifting darker, which is visible as banding rather than as a
    /// wrong number anywhere in particular.
    pub fn over(self, under: Color) -> Color {
        let a = self.alpha() as u32;
        match a {
            255 => self,
            0 => under,
            _ => {
                let mix = |top: u8, bottom: u8| {
                    let sum = top as u32 * a + bottom as u32 * (255 - a) + 127;
                    (sum / 255) as u8
                };
                // The result is as opaque as the two inputs together make it.
                let out_a = a + (under.alpha() as u32 * (255 - a) + 127) / 255;
                Color::rgba(
                    mix(self.red(), under.red()),
                    mix(self.green(), under.green()),
                    mix(self.blue(), under.blue()),
                    out_a.min(255) as u8,
                )
            }
        }
    }
}

pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
pub const BLACK: Color = Color::rgb(0, 0, 0);
pub const WHITE: Color = Color::rgb(255, 255, 255);

/// A rectangle of pixels.
pub struct Surface {
    pub width: u32,
    pub height: u32,
    pixels: Vec<Color>,
}

impl Surface {
    /// A new surface, filled with `fill`.
    ///
    /// Zero in either dimension is allowed and gives a surface that holds no
    /// pixels. Every draw clips, so one behaves like a screen nothing is
    /// visible on rather than like a trap -- which is what a caller computing
    /// a size from data wants when the data turns out to be empty.
    pub fn new(width: u32, height: u32, fill: Color) -> Surface {
        Surface {
            width,
            height,
            pixels: vec![fill; width as usize * height as usize],
        }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height
    }

    /// The colour at a point, or `None` off the edge.
    pub fn get(&self, x: i32, y: i32) -> Option<Color> {
        self.contains(x, y)
            .then(|| self.pixels[y as usize * self.width as usize + x as usize])
    }

    /// Replace a pixel, ignoring alpha. Off-surface points are dropped.
    pub fn set(&mut self, x: i32, y: i32, color: Color) {
        if self.contains(x, y) {
            let at = y as usize * self.width as usize + x as usize;
            self.pixels[at] = color;
        }
    }

    /// Draw a pixel, blending it over what is there.
    pub fn draw(&mut self, x: i32, y: i32, color: Color) {
        if self.contains(x, y) {
            let at = y as usize * self.width as usize + x as usize;
            self.pixels[at] = color.over(self.pixels[at]);
        }
    }

    /// Fill the whole surface, replacing rather than blending -- a frame
    /// starts from a known state, and blending a translucent clear over the
    /// last frame is a trail effect, not a clear.
    pub fn clear(&mut self, color: Color) {
        self.pixels.fill(color);
    }

    /// A filled rectangle. Negative widths draw nothing rather than
    /// reflecting: a size that came out backwards is a bug in the caller, and
    /// silently drawing somewhere else would hide it.
    pub fn fill_rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: Color) {
        if width <= 0 || height <= 0 || color.alpha() == 0 {
            return;
        }
        // Clip once, here, rather than testing every pixel: the inner loop is
        // the hot one in any program that draws for a living.
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x.saturating_add(width)).min(self.width as i32);
        let y1 = (y.saturating_add(height)).min(self.height as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for row in y0..y1 {
            let base = row as usize * self.width as usize;
            for column in x0..x1 {
                let at = base + column as usize;
                self.pixels[at] = color.over(self.pixels[at]);
            }
        }
    }

    /// A rectangle's outline, `thickness` pixels drawn *inside* the bounds.
    ///
    /// Inside rather than centred on the edge, so an outline and a fill of the
    /// same rectangle cover exactly the same pixels. Centring would leave a
    /// shape half a pixel larger than its own coordinates, which is the kind
    /// of thing that only shows up once two of them are meant to line up.
    pub fn stroke_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        thickness: i32,
        color: Color,
    ) {
        if width <= 0 || height <= 0 || thickness <= 0 {
            return;
        }
        let t = thickness.min(width).min(height);
        self.fill_rect(x, y, width, t, color);
        self.fill_rect(x, y + height - t, width, t, color);
        self.fill_rect(x, y + t, t, height - 2 * t, color);
        self.fill_rect(x + width - t, y + t, t, height - 2 * t, color);
    }

    /// A one-pixel line, by Bresenham's algorithm.
    ///
    /// Integer arithmetic only: the same endpoints give the same pixels on
    /// every machine, which a game that records and replays input depends on
    /// and floating point would not guarantee.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let (mut x, mut y) = (x0, y0);
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let step_x = if x0 < x1 { 1 } else { -1 };
        let step_y = if y0 < y1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            self.draw(x, y, color);
            if x == x1 && y == y1 {
                return;
            }
            // Doubling compares the error against half a step without ever
            // dividing, which is what keeps this exact.
            let doubled = 2 * error;
            if doubled >= dy {
                if x == x1 {
                    return;
                }
                error += dy;
                x += step_x;
            }
            if doubled <= dx {
                if y == y1 {
                    return;
                }
                error += dx;
                y += step_y;
            }
        }
    }

    /// A filled circle, drawn as horizontal spans.
    ///
    /// One `fill_rect` per row rather than a test per pixel, and the radius
    /// test is `dx*dx + dy*dy <= r*r` -- squared on both sides, so no square
    /// root and no rounding decides whether an edge pixel is in or out.
    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        if radius < 0 {
            return;
        }
        let r2 = radius as i64 * radius as i64;
        for dy in -radius..=radius {
            let remaining = r2 - dy as i64 * dy as i64;
            let half = (remaining as f64).sqrt() as i32;
            self.fill_rect(cx - half, cy + dy, 2 * half + 1, 1, color);
        }
    }

    /// A circle's outline, one pixel thick, by the midpoint algorithm.
    pub fn stroke_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        if radius < 0 {
            return;
        }
        let (mut x, mut y) = (radius, 0);
        let mut error = 1 - radius;
        while x >= y {
            // Eight-way symmetry: one computed octant gives the other seven.
            for (px, py) in [
                (x, y),
                (y, x),
                (-y, x),
                (-x, y),
                (-x, -y),
                (-y, -x),
                (y, -x),
                (x, -y),
            ] {
                self.draw(cx + px, cy + py, color);
            }
            y += 1;
            if error < 0 {
                error += 2 * y + 1;
            } else {
                x -= 1;
                error += 2 * (y - x) + 1;
            }
        }
    }

    /// Draw a line of text, its top-left corner at `x, y`.
    ///
    /// `scale` is whole-number pixel doubling, not interpolation: at 2 every
    /// pixel becomes a 2x2 block. A bitmap font resampled to a fractional
    /// size turns to mush, and the honest options are the sizes it actually
    /// has.
    ///
    /// Newlines start a new line at the original `x`, so a multi-line string
    /// lays out the way it reads in source.
    pub fn text(&mut self, x: i32, y: i32, text: &str, scale: i32, color: Color) {
        if scale <= 0 {
            return;
        }
        let (mut pen_x, mut pen_y) = (x, y);
        for c in text.chars() {
            if c == '\n' {
                pen_x = x;
                pen_y += (font::HEIGHT + 1) * scale;
                continue;
            }
            for (row, pixels) in font::glyph(c).iter().enumerate() {
                for (column, pixel) in pixels.chars().enumerate() {
                    if pixel != '#' {
                        continue;
                    }
                    // One rect per lit pixel: at scale 1 that is a single
                    // pixel, and at larger scales it is the block. Going
                    // through fill_rect means the clipping is the same
                    // clipping everything else uses.
                    self.fill_rect(
                        pen_x + column as i32 * scale,
                        pen_y + row as i32 * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
            pen_x += font::ADVANCE * scale;
        }
    }

    /// How wide `text` will be, for laying something out around it.
    ///
    /// The trailing gap after the last character is not counted: a caller
    /// centring text would otherwise put it half a space to the left, which
    /// is the kind of thing that looks like carelessness rather than a bug.
    pub fn text_width(text: &str, scale: i32) -> i32 {
        if scale <= 0 || text.is_empty() {
            return 0;
        }
        let longest = text
            .split('\n')
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0) as i32;
        if longest == 0 {
            return 0;
        }
        (longest * font::ADVANCE - 1) * scale
    }

    /// How tall `text` will be: one line, or several separated by a row.
    pub fn text_height(text: &str, scale: i32) -> i32 {
        if scale <= 0 {
            return 0;
        }
        let lines = text.split('\n').count() as i32;
        (lines * font::HEIGHT + (lines - 1)) * scale
    }

    /// Draw another surface at a point, blending each pixel.
    pub fn blit(&mut self, source: &Surface, x: i32, y: i32) {
        for row in 0..source.height as i32 {
            for column in 0..source.width as i32 {
                if let Some(color) = source.get(column, row) {
                    self.draw(x + column, y + row, color);
                }
            }
        }
    }

    /// The pixels as 8-bit RGBA, row-major: what an image file wants.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 4);
        for pixel in &self.pixels {
            out.extend_from_slice(&[pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]);
        }
        out
    }

    pub fn to_png(&self) -> Vec<u8> {
        png::encode_rgba(self.width, self.height, &self.to_rgba())
    }

    pub fn save_png(&self, path: impl AsRef<Path>) -> io::Result<()> {
        std::fs::write(path, self.to_png())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    /// How many pixels of a colour a surface holds -- the question most of
    /// these tests are really asking.
    fn count(surface: &Surface, color: Color) -> usize {
        (0..surface.height as i32)
            .flat_map(|y| (0..surface.width as i32).map(move |x| (x, y)))
            .filter(|(x, y)| surface.get(*x, *y) == Some(color))
            .count()
    }

    #[test]
    fn a_colour_packs_and_unpacks() {
        let c = Color::rgba(1, 2, 3, 4);
        assert_eq!((c.red(), c.green(), c.blue(), c.alpha()), (1, 2, 3, 4));
        assert_eq!(Color::rgb(255, 0, 0).0, 0xFFFF_0000);
    }

    #[test]
    fn blending_hits_the_ends_exactly() {
        // Half-way blends are approximations, but fully opaque and fully
        // transparent must not be: a UI drawn at alpha 255 that came out one
        // shade off would be wrong everywhere and obvious nowhere.
        assert_eq!(RED.over(BLUE), RED);
        assert_eq!(Color::rgba(255, 0, 0, 0).over(BLUE), BLUE);
        let half = Color::rgba(255, 255, 255, 128).over(BLACK);
        assert_eq!(half.red(), 128, "half of 255, rounded");
        assert_eq!(half.alpha(), 255, "over an opaque pixel, still opaque");
    }

    #[test]
    fn a_fade_reaches_the_colour_it_is_fading_to() {
        // This is what the `+ 127` rounding is for, and it took a second
        // attempt to test: blending a colour with *itself* is exact either
        // way, because x*a + x*(255-a) is x*255 whatever a is, so the obvious
        // "does it drift" test discriminates nothing.
        //
        // A fade does. Half-alpha white over black, over and over, has to
        // converge on white. Truncating each step loses the last fraction
        // every time and the sequence stalls one short at 254 -- a fade-in
        // that never quite arrives, which is visible as a screen that stays
        // faintly grey and is maddening to track down.
        let mut surface = Surface::new(1, 1, BLACK);
        for _ in 0..60 {
            surface.draw(0, 0, Color::rgba(255, 255, 255, 128));
        }
        assert_eq!(surface.get(0, 0), Some(WHITE));

        // And the same going down: a fade to black reaches black.
        let mut down = Surface::new(1, 1, WHITE);
        for _ in 0..60 {
            down.draw(0, 0, Color::rgba(0, 0, 0, 128));
        }
        assert_eq!(down.get(0, 0), Some(BLACK));
    }

    #[test]
    fn drawing_off_the_edge_is_ordinary() {
        // The whole point of clipping: a shape that has walked out of shot is
        // not an error, and must not wrap to the far side either.
        let mut surface = Surface::new(4, 4, BLACK);
        surface.fill_rect(-10, -10, 5, 5, RED);
        assert_eq!(count(&surface, RED), 0, "entirely off the top-left");
        surface.fill_rect(-2, -2, 4, 4, BLUE);
        assert_eq!(count(&surface, BLUE), 4, "the overlapping corner only");
        surface.fill_rect(i32::MAX - 1, 0, 10, 10, RED);
        surface.line(-1000, -1000, 1000, 1000, RED);
        surface.fill_circle(0, 0, 1_000_000, BLUE);
        assert_eq!(surface.width, 4, "and none of that panicked");
    }

    #[test]
    fn a_filled_rectangle_covers_exactly_its_own_bounds() {
        let mut surface = Surface::new(8, 8, BLACK);
        surface.fill_rect(2, 3, 3, 2, RED);
        assert_eq!(count(&surface, RED), 6);
        assert_eq!(surface.get(2, 3), Some(RED), "the first corner is inside");
        assert_eq!(surface.get(4, 4), Some(RED), "the last corner is inside");
        assert_eq!(surface.get(5, 3), Some(BLACK), "one past the width is not");
        assert_eq!(surface.get(2, 5), Some(BLACK), "one past the height is not");
    }

    #[test]
    fn an_empty_or_backwards_rectangle_draws_nothing() {
        let mut surface = Surface::new(8, 8, BLACK);
        surface.fill_rect(2, 2, 0, 5, RED);
        surface.fill_rect(2, 2, -4, 5, RED);
        assert_eq!(count(&surface, RED), 0);
    }

    #[test]
    fn an_outline_stays_inside_the_bounds_its_fill_would_cover() {
        // So a border and a background of the same rectangle line up.
        let mut surface = Surface::new(10, 10, BLACK);
        surface.stroke_rect(1, 1, 6, 5, 1, RED);
        assert_eq!(count(&surface, RED), 2 * 6 + 2 * (5 - 2));
        assert_eq!(surface.get(1, 1), Some(RED), "corner drawn once");
        assert_eq!(surface.get(3, 3), Some(BLACK), "middle left alone");
        assert_eq!(surface.get(7, 1), Some(BLACK), "nothing past the width");

        let mut thick = Surface::new(10, 10, BLACK);
        thick.stroke_rect(0, 0, 4, 4, 10, RED);
        assert_eq!(count(&thick, RED), 16, "thickness clamps to the shape");
    }

    #[test]
    fn a_translucent_outline_does_not_darken_its_own_corners() {
        // The four bars are cut so they meet rather than overlap. Nothing
        // enforces that for an opaque colour -- drawing a pixel twice looks
        // identical -- so it is only visible through alpha, where a
        // double-blended corner comes out darker than the edge it joins.
        let mut surface = Surface::new(10, 10, WHITE);
        let ghost = Color::rgba(0, 0, 0, 100);
        surface.stroke_rect(1, 1, 8, 8, 2, ghost);
        let corner = surface.get(1, 1).expect("inside");
        let edge = surface.get(4, 1).expect("inside");
        assert_eq!(corner, edge, "the corner was blended a second time");
    }

    #[test]
    fn a_line_joins_its_endpoints_without_gaps() {
        let mut surface = Surface::new(16, 16, BLACK);
        surface.line(1, 1, 12, 5, RED);
        assert_eq!(surface.get(1, 1), Some(RED), "starts where asked");
        assert_eq!(surface.get(12, 5), Some(RED), "ends where asked");
        // A line's pixels step by one in the long direction, so a shallow
        // line has exactly as many pixels as it spans columns.
        assert_eq!(count(&surface, RED), 12);

        let mut single = Surface::new(4, 4, BLACK);
        single.line(2, 2, 2, 2, RED);
        assert_eq!(count(&single, RED), 1, "a line to itself is a point");
    }

    #[test]
    fn a_line_is_the_same_drawn_backwards() {
        // Not automatic: an algorithm that breaks ties by rounding one way
        // gives a subtly different staircase each direction, which shows up
        // as a shape that shimmers when it turns around.
        let mut forward = Surface::new(20, 20, BLACK);
        let mut backward = Surface::new(20, 20, BLACK);
        forward.line(2, 3, 17, 11, RED);
        backward.line(17, 11, 2, 3, RED);
        for y in 0..20 {
            for x in 0..20 {
                assert_eq!(forward.get(x, y), backward.get(x, y), "at {x},{y}");
            }
        }
    }

    #[test]
    fn a_circle_is_round_and_centred() {
        let mut surface = Surface::new(21, 21, BLACK);
        surface.fill_circle(10, 10, 5, RED);
        assert_eq!(surface.get(10, 10), Some(RED), "the centre");
        assert_eq!(surface.get(15, 10), Some(RED), "exactly on the radius");
        assert_eq!(surface.get(16, 10), Some(BLACK), "one past it");
        assert_eq!(surface.get(10, 5), Some(RED), "and the same vertically");
        // Symmetric about both axes, which a rounding mistake would break.
        for d in 1..=5 {
            assert_eq!(surface.get(10 + d, 10), surface.get(10 - d, 10));
            assert_eq!(surface.get(10, 10 + d), surface.get(10, 10 - d));
        }
        let mut dot = Surface::new(5, 5, BLACK);
        dot.fill_circle(2, 2, 0, RED);
        assert_eq!(count(&dot, RED), 1, "radius zero is one pixel");
    }

    #[test]
    fn a_circle_outline_is_hollow() {
        let mut surface = Surface::new(21, 21, BLACK);
        surface.stroke_circle(10, 10, 8, RED);
        assert_eq!(surface.get(10, 10), Some(BLACK), "the middle is untouched");
        assert_eq!(surface.get(18, 10), Some(RED));
        assert_eq!(surface.get(10, 2), Some(RED));
    }

    #[test]
    fn blitting_composites_rather_than_replacing() {
        let mut screen = Surface::new(4, 4, BLACK);
        let mut sprite = Surface::new(2, 2, TRANSPARENT);
        sprite.set(0, 0, RED);
        sprite.set(1, 1, Color::rgba(255, 255, 255, 128));

        screen.blit(&sprite, 1, 1);
        assert_eq!(screen.get(1, 1), Some(RED));
        assert_eq!(screen.get(2, 1), Some(BLACK), "transparent left it alone");
        assert_eq!(screen.get(2, 2).unwrap().red(), 128, "half over black");
    }

    #[test]
    fn text_lands_where_it_is_asked_and_is_the_size_it_reports() {
        let mut surface = Surface::new(80, 20, BLACK);
        surface.text(2, 3, "Hi", 1, RED);
        // 'H' is a full-height bar in its first column, so the top-left lit
        // pixel is exactly at the origin asked for.
        assert_eq!(surface.get(2, 3), Some(RED));
        assert_eq!(surface.get(1, 3), Some(BLACK), "nothing to the left");
        assert_eq!(surface.get(2, 2), Some(BLACK), "nothing above");
        assert_eq!(Surface::text_width("Hi", 1), 11, "two cells, one gap");
        assert_eq!(Surface::text_height("Hi", 1), 7);
    }

    #[test]
    fn scaling_text_multiplies_whole_pixels() {
        let mut one = Surface::new(40, 20, BLACK);
        let mut two = Surface::new(80, 40, BLACK);
        one.text(0, 0, "L", 1, RED);
        two.text(0, 0, "L", 2, RED);
        assert_eq!(count(&two, RED), 4 * count(&one, RED), "every pixel a 2x2");
        assert_eq!(Surface::text_width("L", 2), 2 * Surface::text_width("L", 1));
    }

    #[test]
    fn several_lines_stack_without_touching() {
        let text = "A\nA";
        assert_eq!(Surface::text_height(text, 1), 15, "7 + gap + 7");
        assert_eq!(
            Surface::text_width(text, 1),
            5,
            "as wide as its widest line"
        );

        let mut surface = Surface::new(20, 20, BLACK);
        surface.text(0, 0, text, 1, RED);
        // The row between the two lines has to be empty, or descenders and
        // capitals from neighbouring lines collide.
        let blank = (0..20).all(|x| surface.get(x, 7) == Some(BLACK));
        assert!(blank, "the gap row is not blank");
    }

    #[test]
    fn text_clips_like_everything_else() {
        let mut surface = Surface::new(8, 8, BLACK);
        surface.text(-100, -100, "hello", 1, RED);
        surface.text(1000, 2, "hello", 1, RED);
        surface.text(6, 2, "hello", 3, RED);
        assert_eq!(surface.width, 8, "and none of that panicked");
    }

    #[test]
    fn a_nonsense_scale_draws_nothing_rather_than_looping() {
        let mut surface = Surface::new(8, 8, BLACK);
        surface.text(0, 0, "x", 0, RED);
        surface.text(0, 0, "x", -4, RED);
        assert_eq!(count(&surface, RED), 0);
        assert_eq!(Surface::text_width("x", 0), 0);
    }

    #[test]
    fn a_surface_with_no_pixels_still_behaves() {
        // A size computed from data can come out zero, and that should read
        // as "nothing is visible", not as a crash halfway through a frame.
        let mut empty = Surface::new(0, 0, BLACK);
        empty.clear(RED);
        empty.fill_rect(0, 0, 10, 10, RED);
        assert!(empty.to_rgba().is_empty());
        assert!(!empty.to_png().is_empty(), "still a valid file");
    }
}
