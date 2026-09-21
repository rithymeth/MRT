//! A loadable bitmap font: a grid of glyphs cut from an image, addressed by
//! looking a character up in a caller-supplied order.
//!
//! -- Why this exists alongside `font` --
//!
//! `font` is one glyph set, baked into the binary, so a program never has to
//! ship a font just to draw a label. That is a deliberately small answer:
//! one look, ASCII only, whatever `font.rs` happens to contain. A game that
//! wants its own pixel art for text -- a different style, colour baked into
//! the glyphs themselves, characters `font` does not have -- needs a font it
//! can load, the same way a sprite is loaded rather than drawn by the
//! engine's own hand. This is that: a plain grid of same-sized cells in a
//! `Surface`, read in the same row-major order `gameDrawTilemap` already
//! reads a tile sheet in.
//!
//! -- What it is not --
//!
//! Not variable-width, not kerned. A grid cell is exactly `char_width` by
//! `char_height` for every character, the same simplicity a tile sheet
//! already rests on, and an artist who wants a narrower "i" than "M" pads
//! its cell -- the tradeoff a monospace terminal font makes on purpose.

use std::collections::HashMap;

use crate::Surface;

/// An atlas plus the character each of its cells stands for.
pub struct BitmapFont {
    surface: Surface,
    char_width: i32,
    char_height: i32,
    index: HashMap<char, u32>,
}

impl std::fmt::Debug for BitmapFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BitmapFont({}x{} cells, {} characters)",
            self.char_width,
            self.char_height,
            self.index.len()
        )
    }
}

impl BitmapFont {
    /// Build a font from an already-loaded atlas.
    ///
    /// `chars` lists which character sits in which cell, reading the grid
    /// left to right and then top to bottom. The atlas may hold more cells
    /// than `chars` names -- a caller drawing only a partial character set
    /// from a bigger sheet is ordinary -- but not fewer, and never the same
    /// character twice, since then a draw could not say which cell was
    /// meant.
    pub fn new(
        surface: Surface,
        char_width: i32,
        char_height: i32,
        chars: &str,
    ) -> Result<BitmapFont, String> {
        if char_width <= 0 || char_height <= 0 {
            return Err(format!(
                "a font's character cells must be at least 1x1 pixel, not {char_width}x{char_height}"
            ));
        }
        if chars.is_empty() {
            return Err("a font needs at least one character in its character set".to_string());
        }
        let (w, h) = (char_width as u32, char_height as u32);
        if surface.width % w != 0 || surface.height % h != 0 {
            return Err(format!(
                "a font atlas of {}x{} is not an exact grid of {char_width}x{char_height} cells",
                surface.width, surface.height
            ));
        }
        let cells = (surface.width / w) as usize * (surface.height / h) as usize;
        let count = chars.chars().count();
        if count > cells {
            return Err(format!(
                "{count} characters were given but the atlas only has {cells} cells"
            ));
        }

        let mut index = HashMap::new();
        for (cell, c) in chars.chars().enumerate() {
            if index.insert(c, cell as u32).is_some() {
                return Err(format!("{c:?} appears twice in the character set"));
            }
        }

        Ok(BitmapFont {
            surface,
            char_width,
            char_height,
            index,
        })
    }

    pub fn char_width(&self) -> i32 {
        self.char_width
    }

    pub fn char_height(&self) -> i32 {
        self.char_height
    }

    /// The atlas rectangle a character sits in, or `None` if this font has
    /// no glyph for it.
    fn rect(&self, c: char) -> Option<(i32, i32, u32, u32)> {
        let cell = *self.index.get(&c)?;
        let columns = self.surface.width / self.char_width as u32;
        let (col, row) = (cell % columns, cell / columns);
        Some((
            col as i32 * self.char_width,
            row as i32 * self.char_height,
            self.char_width as u32,
            self.char_height as u32,
        ))
    }

    /// Draw a line of text, its top-left corner at `x, y`.
    ///
    /// Every character the string needs is checked before any of them is
    /// drawn, the same reasoning `gameDrawTilemap` checks a whole grid
    /// before stamping the first tile: a label half-drawn because of one
    /// typo'd character is a worse thing to debug than one that was never
    /// touched. The character missing from this font is returned so the
    /// caller can name it.
    ///
    /// `scale` is whole-number pixel doubling, the same as `Surface::text`,
    /// for the same reason: a bitmap resampled to a fractional size turns to
    /// mush. `scale <= 0` draws nothing, matching `Surface::text`.
    pub fn draw(
        &self,
        target: &mut Surface,
        x: i32,
        y: i32,
        text: &str,
        scale: i32,
    ) -> Result<(), char> {
        if scale <= 0 {
            return Ok(());
        }
        for c in text.chars() {
            if c != '\n' && self.rect(c).is_none() {
                return Err(c);
            }
        }
        let (mut pen_x, mut pen_y) = (x, y);
        for c in text.chars() {
            if c == '\n' {
                pen_x = x;
                pen_y += self.char_height * scale;
                continue;
            }
            let region = self.rect(c).expect("checked above");
            target.blit_transformed(
                &self.surface,
                pen_x,
                pen_y,
                0.0,
                scale as f64,
                scale as f64,
                0.0,
                0.0,
                Some(region),
            );
            pen_x += self.char_width * scale;
        }
        Ok(())
    }

    /// How wide `text` would be if drawn, in pixels.
    ///
    /// Every cell is the same width, so this is a character count away from
    /// a glyph lookup -- unlike `draw`, it never fails on a missing
    /// character, the same way `Surface::text_width` never fails either.
    pub fn text_width(&self, text: &str, scale: i32) -> i32 {
        if scale <= 0 || text.is_empty() {
            return 0;
        }
        let longest = text
            .split('\n')
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0) as i32;
        longest * self.char_width * scale
    }

    /// How tall `text` would be if drawn: one line, or several stacked with
    /// no gap between them -- any spacing a font wants is already inside
    /// its own cell height.
    pub fn text_height(&self, text: &str, scale: i32) -> i32 {
        if scale <= 0 {
            return 0;
        }
        let lines = text.split('\n').count() as i32;
        lines * self.char_height * scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, TRANSPARENT};

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);
    const GREEN: Color = Color::rgb(0, 255, 0);

    /// A 3-cell, 2x2-per-cell atlas: 'a' red, 'b' blue, 'c' green.
    fn atlas() -> Surface {
        let mut surface = Surface::new(6, 2, TRANSPARENT);
        for y in 0..2 {
            for x in 0..2 {
                surface.set(x, y, RED);
                surface.set(x + 2, y, BLUE);
                surface.set(x + 4, y, GREEN);
            }
        }
        surface
    }

    #[test]
    fn a_font_atlas_must_be_an_exact_grid() {
        let message = BitmapFont::new(Surface::new(5, 2, TRANSPARENT), 2, 2, "a").unwrap_err();
        assert!(message.contains("not an exact grid"), "{message}");
    }

    #[test]
    fn cells_cannot_be_smaller_than_a_pixel() {
        let message = BitmapFont::new(Surface::new(4, 4, TRANSPARENT), 0, 2, "a").unwrap_err();
        assert!(message.contains("1x1 pixel"), "{message}");
    }

    #[test]
    fn an_empty_character_set_is_refused() {
        let message = BitmapFont::new(Surface::new(4, 4, TRANSPARENT), 2, 2, "").unwrap_err();
        assert!(message.contains("at least one character"), "{message}");
    }

    #[test]
    fn a_repeated_character_names_itself() {
        let message = BitmapFont::new(atlas(), 2, 2, "aa").unwrap_err();
        assert!(message.contains("'a'"), "{message}");
        assert!(message.contains("twice"), "{message}");
    }

    #[test]
    fn more_characters_than_cells_is_refused() {
        let message = BitmapFont::new(atlas(), 2, 2, "abcd").unwrap_err();
        assert!(message.contains("only has 3 cells"), "{message}");
    }

    #[test]
    fn fewer_characters_than_cells_is_an_ordinary_partial_set() {
        assert!(BitmapFont::new(atlas(), 2, 2, "a").is_ok());
    }

    #[test]
    fn drawing_picks_out_the_right_cell_for_each_character() {
        let font = BitmapFont::new(atlas(), 2, 2, "abc").unwrap();
        let mut screen = Surface::new(10, 10, TRANSPARENT);
        font.draw(&mut screen, 0, 0, "ba", 1).unwrap();
        assert_eq!(screen.get(0, 0), Some(BLUE), "b's cell drawn first");
        assert_eq!(
            screen.get(2, 0),
            Some(RED),
            "then a's cell, advanced by one"
        );
        assert_eq!(screen.get(4, 0), Some(TRANSPARENT), "nothing past the text");
    }

    #[test]
    fn an_unknown_character_names_itself_and_draws_nothing() {
        let font = BitmapFont::new(atlas(), 2, 2, "ab").unwrap();
        let mut screen = Surface::new(10, 10, TRANSPARENT);
        let error = font.draw(&mut screen, 0, 0, "aqb", 1).unwrap_err();
        assert_eq!(error, 'q');
        // Checked before any of it is drawn, not stopped partway through.
        assert_eq!(
            screen.get(0, 0),
            Some(TRANSPARENT),
            "not even the leading a"
        );
    }

    #[test]
    fn a_newline_starts_a_new_line_at_the_original_x() {
        let font = BitmapFont::new(atlas(), 2, 2, "ab").unwrap();
        let mut screen = Surface::new(10, 10, TRANSPARENT);
        font.draw(&mut screen, 1, 0, "a\nb", 1).unwrap();
        assert_eq!(screen.get(1, 0), Some(RED));
        assert_eq!(screen.get(1, 2), Some(BLUE), "second line, same x");
    }

    #[test]
    fn scale_multiplies_whole_cells() {
        let font = BitmapFont::new(atlas(), 2, 2, "a").unwrap();
        let mut one = Surface::new(10, 10, TRANSPARENT);
        let mut two = Surface::new(10, 10, TRANSPARENT);
        font.draw(&mut one, 0, 0, "a", 1).unwrap();
        font.draw(&mut two, 0, 0, "a", 2).unwrap();
        let count = |s: &Surface, c: Color| {
            (0..10)
                .flat_map(|y| (0..10).map(move |x| (x, y)))
                .filter(|(x, y)| s.get(*x, *y) == Some(c))
                .count()
        };
        assert_eq!(count(&two, RED), 4 * count(&one, RED));
    }

    #[test]
    fn a_nonpositive_scale_draws_nothing_rather_than_erroring() {
        let font = BitmapFont::new(atlas(), 2, 2, "ab").unwrap();
        let mut screen = Surface::new(10, 10, TRANSPARENT);
        // Even the unknown 'q' does not surface: there is nothing to draw.
        assert!(font.draw(&mut screen, 0, 0, "aqb", 0).is_ok());
        assert_eq!(screen.get(0, 0), Some(TRANSPARENT));
    }

    #[test]
    fn text_width_and_height_do_not_depend_on_which_characters_are_used() {
        let font = BitmapFont::new(atlas(), 2, 2, "abc").unwrap();
        assert_eq!(font.text_width("abc", 1), 6);
        assert_eq!(font.text_width("abc", 2), 12);
        assert_eq!(font.text_width("", 1), 0);
        assert_eq!(font.text_height("a\nb", 1), 4);
        assert_eq!(font.text_height("a", 1), 2);
    }
}
