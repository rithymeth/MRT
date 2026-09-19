//! A bitmap font, legible in its own source.
//!
//! -- Why the glyphs are drawings --
//!
//! A bitmap font is normally a wall of hex: `0x7E, 0x11, 0x11, 0x7E`. That is
//! compact, unreadable, and impossible to check by eye -- a single wrong
//! nibble is a letter with a hole in it, and you find out by rendering.
//!
//! Here each glyph is written as the shape it draws. `#` is a lit pixel and a
//! space is not, so the `A` below looks like an `A` in the file. It costs
//! five bytes per row instead of one, in a constant, in a program that is
//! about to allocate a megabyte for a framebuffer -- which is no cost at all.
//! What it buys is that a reader can check the font without running it, and
//! can add a glyph by drawing one.
//!
//! -- The dimensions --
//!
//! Five wide by seven tall, advancing six: the smallest cell in which every
//! ASCII character stays distinguishable. Descenders on `g j p q y` hang into
//! the last row rather than below the cell, so a line of text occupies
//! exactly the height it claims and two lines never touch.

/// A glyph's height in pixels.
pub const HEIGHT: i32 = 7;
/// A glyph's width in pixels.
pub const WIDTH: i32 = 5;
/// The distance from one character's left edge to the next: the width plus a
/// column of space, so text does not need a kerning table to be readable.
pub const ADVANCE: i32 = WIDTH + 1;

/// The printable ASCII range, drawn. Anything outside it renders as `?`.
#[rustfmt::skip]
const GLYPHS: [(char, [&str; HEIGHT as usize]); 95] = [
    (' ',  ["     ", "     ", "     ", "     ", "     ", "     ", "     "]),
    ('!',  ["  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "     ", "  #  "]),
    ('"',  [" # # ", " # # ", "     ", "     ", "     ", "     ", "     "]),
    ('#',  [" # # ", " # # ", "#####", " # # ", "#####", " # # ", " # # "]),
    ('$',  ["  #  ", " ####", "#  # ", " ### ", "  # #", "#### ", "  #  "]),
    ('%',  ["##   ", "##  #", "   # ", "  #  ", " #   ", "#  ##", "   ##"]),
    ('&',  [" ##  ", "#  # ", "#  # ", " ##  ", "#  ##", "#   #", " ## #"]),
    ('\'', ["  #  ", "  #  ", "     ", "     ", "     ", "     ", "     "]),
    ('(',  ["   # ", "  #  ", " #   ", " #   ", " #   ", "  #  ", "   # "]),
    (')',  [" #   ", "  #  ", "   # ", "   # ", "   # ", "  #  ", " #   "]),
    ('*',  ["     ", "#   #", " # # ", "#####", " # # ", "#   #", "     "]),
    ('+',  ["     ", "  #  ", "  #  ", "#####", "  #  ", "  #  ", "     "]),
    (',',  ["     ", "     ", "     ", "     ", "  ## ", "  ## ", "   # "]),
    ('-',  ["     ", "     ", "     ", "#####", "     ", "     ", "     "]),
    ('.',  ["     ", "     ", "     ", "     ", "     ", " ##  ", " ##  "]),
    ('/',  ["    #", "   # ", "  #  ", "  #  ", "  #  ", " #   ", "#    "]),
    ('0',  [" ### ", "#   #", "#  ##", "# # #", "##  #", "#   #", " ### "]),
    ('1',  ["  #  ", " ##  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "]),
    ('2',  [" ### ", "#   #", "    #", "   # ", "  #  ", " #   ", "#####"]),
    ('3',  ["#####", "   # ", "  #  ", "   # ", "    #", "#   #", " ### "]),
    ('4',  ["   # ", "  ## ", " # # ", "#  # ", "#####", "   # ", "   # "]),
    ('5',  ["#####", "#    ", "#### ", "    #", "    #", "#   #", " ### "]),
    ('6',  ["  ## ", " #   ", "#    ", "#### ", "#   #", "#   #", " ### "]),
    ('7',  ["#####", "    #", "   # ", "  #  ", " #   ", " #   ", " #   "]),
    ('8',  [" ### ", "#   #", "#   #", " ### ", "#   #", "#   #", " ### "]),
    ('9',  [" ### ", "#   #", "#   #", " ####", "    #", "   # ", " ##  "]),
    (':',  ["     ", " ##  ", " ##  ", "     ", " ##  ", " ##  ", "     "]),
    (';',  ["     ", " ##  ", " ##  ", "     ", " ##  ", "  #  ", " #   "]),
    ('<',  ["   # ", "  #  ", " #   ", "#    ", " #   ", "  #  ", "   # "]),
    ('=',  ["     ", "     ", "#####", "     ", "#####", "     ", "     "]),
    ('>',  [" #   ", "  #  ", "   # ", "    #", "   # ", "  #  ", " #   "]),
    ('?',  [" ### ", "#   #", "    #", "   # ", "  #  ", "     ", "  #  "]),
    ('@',  [" ### ", "#   #", "# ###", "# # #", "# ## ", "#    ", " ####"]),
    ('A',  [" ### ", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"]),
    ('B',  ["#### ", "#   #", "#   #", "#### ", "#   #", "#   #", "#### "]),
    ('C',  [" ### ", "#   #", "#    ", "#    ", "#    ", "#   #", " ### "]),
    ('D',  ["###  ", "#  # ", "#   #", "#   #", "#   #", "#  # ", "###  "]),
    ('E',  ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#####"]),
    ('F',  ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#    "]),
    ('G',  [" ### ", "#   #", "#    ", "#  ##", "#   #", "#   #", " ####"]),
    ('H',  ["#   #", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"]),
    ('I',  [" ### ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "]),
    ('J',  ["    #", "    #", "    #", "    #", "    #", "#   #", " ### "]),
    ('K',  ["#   #", "#  # ", "# #  ", "##   ", "# #  ", "#  # ", "#   #"]),
    ('L',  ["#    ", "#    ", "#    ", "#    ", "#    ", "#    ", "#####"]),
    ('M',  ["#   #", "## ##", "# # #", "#   #", "#   #", "#   #", "#   #"]),
    ('N',  ["#   #", "##  #", "# # #", "#  ##", "#   #", "#   #", "#   #"]),
    ('O',  [" ### ", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "]),
    ('P',  ["#### ", "#   #", "#   #", "#### ", "#    ", "#    ", "#    "]),
    ('Q',  [" ### ", "#   #", "#   #", "#   #", "# # #", "#  # ", " ## #"]),
    ('R',  ["#### ", "#   #", "#   #", "#### ", "# #  ", "#  # ", "#   #"]),
    ('S',  [" ####", "#    ", "#    ", " ### ", "    #", "    #", "#### "]),
    ('T',  ["#####", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('U',  ["#   #", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "]),
    ('V',  ["#   #", "#   #", "#   #", "#   #", "#   #", " # # ", "  #  "]),
    ('W',  ["#   #", "#   #", "#   #", "# # #", "# # #", "## ##", "#   #"]),
    ('X',  ["#   #", "#   #", " # # ", "  #  ", " # # ", "#   #", "#   #"]),
    ('Y',  ["#   #", "#   #", " # # ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('Z',  ["#####", "    #", "   # ", "  #  ", " #   ", "#    ", "#####"]),
    ('[',  ["  ## ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  ## "]),
    ('\\', ["#    ", " #   ", "  #  ", "  #  ", "  #  ", "   # ", "    #"]),
    (']',  [" ##  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", " ##  "]),
    ('^',  ["  #  ", " # # ", "#   #", "     ", "     ", "     ", "     "]),
    ('_',  ["     ", "     ", "     ", "     ", "     ", "     ", "#####"]),
    ('`',  [" #   ", "  #  ", "     ", "     ", "     ", "     ", "     "]),
    ('a',  ["     ", "     ", " ### ", "    #", " ####", "#   #", " ####"]),
    ('b',  ["#    ", "#    ", "#### ", "#   #", "#   #", "#   #", "#### "]),
    ('c',  ["     ", "     ", " ####", "#    ", "#    ", "#    ", " ####"]),
    ('d',  ["    #", "    #", " ####", "#   #", "#   #", "#   #", " ####"]),
    ('e',  ["     ", "     ", " ### ", "#   #", "#####", "#    ", " ####"]),
    ('f',  ["  ## ", " #  #", " #   ", "###  ", " #   ", " #   ", " #   "]),
    ('g',  ["     ", " ####", "#   #", "#   #", " ####", "    #", " ### "]),
    ('h',  ["#    ", "#    ", "#### ", "#   #", "#   #", "#   #", "#   #"]),
    ('i',  ["  #  ", "     ", " ##  ", "  #  ", "  #  ", "  #  ", " ### "]),
    ('j',  ["   # ", "     ", "  ## ", "   # ", "   # ", "#  # ", " ##  "]),
    ('k',  ["#    ", "#    ", "#  # ", "# #  ", "##   ", "# #  ", "#  # "]),
    ('l',  [" ##  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "]),
    ('m',  ["     ", "     ", "## # ", "# # #", "# # #", "#   #", "#   #"]),
    ('n',  ["     ", "     ", "#### ", "#   #", "#   #", "#   #", "#   #"]),
    ('o',  ["     ", "     ", " ### ", "#   #", "#   #", "#   #", " ### "]),
    ('p',  ["     ", "#### ", "#   #", "#   #", "#### ", "#    ", "#    "]),
    ('q',  ["     ", " ####", "#   #", "#   #", " ####", "    #", "    #"]),
    ('r',  ["     ", "     ", "# ## ", "##  #", "#    ", "#    ", "#    "]),
    ('s',  ["     ", "     ", " ####", "#    ", " ### ", "    #", "#### "]),
    ('t',  [" #   ", " #   ", "###  ", " #   ", " #   ", " #  #", "  ## "]),
    ('u',  ["     ", "     ", "#   #", "#   #", "#   #", "#  ##", " ## #"]),
    ('v',  ["     ", "     ", "#   #", "#   #", "#   #", " # # ", "  #  "]),
    ('w',  ["     ", "     ", "#   #", "#   #", "# # #", "# # #", " # # "]),
    ('x',  ["     ", "     ", "#   #", " # # ", "  #  ", " # # ", "#   #"]),
    ('y',  ["     ", "#   #", "#   #", "#   #", " ####", "    #", " ### "]),
    ('z',  ["     ", "     ", "#####", "   # ", "  #  ", " #   ", "#####"]),
    ('{',  ["   ##", "  #  ", "  #  ", " #   ", "  #  ", "  #  ", "   ##"]),
    ('|',  ["  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('}',  ["##   ", "  #  ", "  #  ", "   # ", "  #  ", "  #  ", "##   "]),
    ('~',  ["     ", "     ", " #  #", "# ## ", "     ", "     ", "     "]),
];

/// The rows of a character's drawing, or `?`'s for one there is no glyph for.
///
/// A visible substitute rather than a blank: text that silently loses
/// characters reads as a program with a bug in its *logic*, and the author
/// goes looking in the wrong place.
pub fn glyph(c: char) -> &'static [&'static str; HEIGHT as usize] {
    let index = GLYPHS.iter().position(|(g, _)| *g == c);
    match index {
        Some(at) => &GLYPHS[at].1,
        None => glyph('?'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_the_size_it_claims() {
        // One short row is a letter with a bite out of it, and the drawings
        // are hand-written, so this is worth checking rather than trusting.
        for (c, rows) in GLYPHS.iter() {
            for (n, row) in rows.iter().enumerate() {
                assert_eq!(
                    row.chars().count(),
                    WIDTH as usize,
                    "{c:?} row {n} is {row:?}"
                );
                assert!(
                    row.chars().all(|p| p == '#' || p == ' '),
                    "{c:?} row {n} has something other than '#' and ' ': {row:?}"
                );
            }
        }
    }

    #[test]
    fn the_table_covers_printable_ascii_exactly_once() {
        let expected: Vec<char> = (32u8..127).map(char::from).collect();
        let actual: Vec<char> = GLYPHS.iter().map(|(c, _)| *c).collect();
        assert_eq!(actual, expected, "in order, with no gaps or repeats");
    }

    #[test]
    fn an_unknown_character_draws_a_question_mark() {
        assert_eq!(glyph('\u{1F600}'), glyph('?'));
        assert_eq!(glyph('\t'), glyph('?'));
    }

    #[test]
    fn a_space_is_blank_and_a_letter_is_not() {
        assert!(glyph(' ').iter().all(|r| r.trim().is_empty()));
        assert!(glyph('A').iter().any(|r| r.contains('#')));
    }
}
