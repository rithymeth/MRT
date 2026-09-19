//! The MRT-facing side of the game extension.
//!
//! -- Where this differs from MRT-AI, and why --
//!
//! The AI extension made a model *ordinary MRT data*: an array of layer
//! objects, printable and indexable, so the language learned no new type. The
//! same principle applies here and reaches a different answer, which is worth
//! saying plainly rather than letting it look like an inconsistency.
//!
//! A model is small -- a few thousand numbers -- so it can be nested arrays
//! and the conversion cost is a rounding error. A 1280x720 framebuffer is
//! **921,600 pixels**. As MRT values that is a million `Value`s rebuilt every
//! frame, which is not a slow design, it is an unusable one.
//!
//! So the surface stays in the engine, and the drawing built-ins address it
//! implicitly -- one screen, like the canvas context every drawing API
//! eventually settles on. The language still learns no new value kind, which
//! was the actual goal; it just gets there by holding *nothing* rather than
//! by holding data. Everything that does cross the boundary -- coordinates,
//! sizes, colours -- is a plain number.
//!
//! -- Colours are numbers --
//!
//! `gameColor(255, 0, 0)` packs to `0xAARRGGBB` and hands back a number. A
//! number rather than an object because colours are named once and then used
//! in a draw loop: `var RED = gameColor(255, 0, 0);` and then `gameRect(x, y,
//! w, h, RED)` allocates nothing per frame.
//!
//! -- Coordinates may be fractional --
//!
//! A player at `x = 100.5` moving at 250 pixels a second is the normal case,
//! not a mistake, so unlike the AI built-ins' sizes these are not required to
//! be whole. They are floored to land on a pixel. Sizes that *describe the
//! surface itself* -- the width and height passed to `gameInit` -- are counts,
//! and are checked as such.

use std::rc::Rc;

use mrt_game::{Color, Surface};

use crate::error::{type_error, value_error, Signal};
use crate::value::Value;

/// The engine's screen, plus what a window will need when there is one.
pub struct Screen {
    pub surface: Surface,
    /// Kept from `gameInit` and unused until a window exists to wear it.
    /// Stored rather than dropped so that adding the window later changes no
    /// program: a title set today is the title shown then.
    pub title: String,
}

/// The error every drawing call gets before `gameInit` has run.
///
/// Its own function because the alternative -- each built-in inventing a
/// sentence -- is how a library ends up with eight spellings of one mistake.
fn no_screen(who: &str) -> Signal {
    value_error(format!(
        "{who}() needs a screen; call gameInit(width, height, title) first."
    ))
}

fn screen<'a>(screen: &'a mut Option<Screen>, who: &str) -> Result<&'a mut Screen, Signal> {
    screen.as_mut().ok_or_else(|| no_screen(who))
}

/// A coordinate: any number, floored to a pixel.
///
/// Floor rather than truncate, because truncation rounds *towards zero* and
/// so treats -0.5 and 0.5 as the same pixel. A sprite drifting left across
/// the origin would stutter there and nowhere else, which is a horrible bug
/// to go looking for.
pub fn coord(value: &Value, who: &str, what: &str) -> Result<i32, Signal> {
    match value {
        Value::Number(n) if n.is_finite() => Ok(n.floor().clamp(-1.0e9, 1.0e9) as i32),
        Value::Number(n) => Err(value_error(format!(
            "{who}() needs {what} to be a real number, not {n}."
        ))),
        other => Err(type_error(format!(
            "{who}() needs {what} to be a number, not {}.",
            crate::value::type_name(other)
        ))),
    }
}

/// A packed colour, as `gameColor` produced it.
pub fn color(value: &Value, who: &str) -> Result<Color, Signal> {
    match value {
        Value::Number(n) if n.fract() == 0.0 && *n >= 0.0 && *n <= u32::MAX as f64 => {
            Ok(Color(*n as u32))
        }
        Value::Number(n) => Err(value_error(format!(
            "{who}() needs a colour from gameColor(), not {n}."
        ))),
        other => Err(type_error(format!(
            "{who}() needs a colour to be a number, not {}.",
            crate::value::type_name(other)
        ))),
    }
}

/// A colour channel: 0 to 255.
///
/// Out of range is an error rather than a clamp. Clamping would quietly
/// accept `gameColor(255, 300, 0)` as yellow-green and leave the author
/// wondering why their arithmetic never changes the picture past a point.
pub fn channel(value: &Value, who: &str, what: &str) -> Result<u8, Signal> {
    match value {
        Value::Number(n) if n.fract() == 0.0 && *n >= 0.0 && *n <= 255.0 => Ok(*n as u8),
        Value::Number(n) => Err(value_error(format!(
            "{who}() needs {what} to be a whole number from 0 to 255, not {n}."
        ))),
        other => Err(type_error(format!(
            "{who}() needs {what} to be a number, not {}.",
            crate::value::type_name(other)
        ))),
    }
}

/// Create the screen. Running it twice replaces the old one, so a program can
/// resize rather than being stuck with its first guess.
pub fn init(width: usize, height: usize, title: &str) -> Result<Screen, Signal> {
    // An upper bound so a typo asks for a picture rather than all of memory:
    // 16384 square is larger than any display and still only a gigabyte.
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(value_error(format!(
            "gameInit() needs a width and height from 1 to 16384, not {width}x{height}."
        )));
    }
    Ok(Screen {
        surface: Surface::new(width as u32, height as u32, mrt_game::BLACK),
        title: title.to_string(),
    })
}

/// Write the frame out as a PNG.
///
/// Until there is a window, this is how a program is *seen* -- and it stays
/// useful after there is one, because a test can compare a saved frame and a
/// person cannot compare a window.
pub fn save(screen: &Option<Screen>, path: &str) -> Result<Value, Signal> {
    let screen = screen.as_ref().ok_or_else(|| no_screen("gameSave"))?;
    screen
        .surface
        .save_png(path)
        .map_err(|e| value_error(format!("gameSave() could not write {path}: {e}")))?;
    Ok(Value::Null)
}

/// `gameColor` and `gameColorAlpha` both land here.
pub fn make_color(r: u8, g: u8, b: u8, a: u8) -> Value {
    Value::Number(Color::rgba(r, g, b, a).0 as f64)
}

/// Read a colour back apart, for a program that stored one and wants to fade
/// it. Without this a packed colour would be a one-way door, and MRT has no
/// bit-shifting to take it apart by hand.
pub fn split_color(c: Color) -> Value {
    let mut map = crate::value::ObjMap::new();
    let mut put = |name: &str, n: u8| {
        map.insert(
            crate::value::ObjKey::Str(Rc::from(name)),
            Value::Number(n as f64),
        );
    };
    put("red", c.red());
    put("green", c.green());
    put("blue", c.blue());
    put("alpha", c.alpha());
    Value::object(map)
}

/// Every drawing built-in, once its arguments are numbers.
///
/// Gathered here rather than left in `builtins.rs` so that the argument
/// checking and the drawing are not interleaved: what each call does to the
/// surface is one line, and the surrounding noise lives at the call site.
pub mod draw {
    use super::*;

    pub fn clear(s: &mut Option<Screen>, c: Color) -> Result<Value, Signal> {
        screen(s, "gameClear")?.surface.clear(c);
        Ok(Value::Null)
    }

    pub fn pixel(s: &mut Option<Screen>, x: i32, y: i32, c: Color) -> Result<Value, Signal> {
        screen(s, "gamePixel")?.surface.draw(x, y, c);
        Ok(Value::Null)
    }

    pub fn rect(
        s: &mut Option<Screen>,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        c: Color,
    ) -> Result<Value, Signal> {
        screen(s, "gameRect")?.surface.fill_rect(x, y, w, h, c);
        Ok(Value::Null)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rect_outline(
        s: &mut Option<Screen>,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        t: i32,
        c: Color,
    ) -> Result<Value, Signal> {
        screen(s, "gameRectOutline")?
            .surface
            .stroke_rect(x, y, w, h, t, c);
        Ok(Value::Null)
    }

    pub fn line(
        s: &mut Option<Screen>,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        c: Color,
    ) -> Result<Value, Signal> {
        screen(s, "gameLine")?.surface.line(x0, y0, x1, y1, c);
        Ok(Value::Null)
    }

    pub fn circle(
        s: &mut Option<Screen>,
        x: i32,
        y: i32,
        r: i32,
        c: Color,
    ) -> Result<Value, Signal> {
        screen(s, "gameCircle")?.surface.fill_circle(x, y, r, c);
        Ok(Value::Null)
    }

    pub fn circle_outline(
        s: &mut Option<Screen>,
        x: i32,
        y: i32,
        r: i32,
        c: Color,
    ) -> Result<Value, Signal> {
        screen(s, "gameCircleOutline")?
            .surface
            .stroke_circle(x, y, r, c);
        Ok(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use crate::{run, run_vm, SourceFile};

    /// Run a program on both engines and insist they agree.
    ///
    /// Both, for the same reason the AI bridge does it: the conformance
    /// corpus deliberately does not cover an extension, so a `game*` built-in
    /// that worked on the tree-walker and not the VM would be caught by
    /// nothing else in the repository.
    fn go(source: &str) -> String {
        let file = SourceFile::new("test.mrt", source);
        let text = |outcome: crate::Outcome| {
            let mut lines = outcome.output;
            if let Some(error) = outcome.error {
                lines.push(error);
            }
            lines.join("\n")
        };
        let walked = text(run(&file));
        let compiled = text(run_vm(&file));
        assert_eq!(walked, compiled, "the engines disagree");
        walked
    }

    fn main_of(body: &str) -> String {
        go(&format!("func main() {{ {body} }}"))
    }

    /// A program that sets up a screen first, since almost every test needs
    /// one before it can do anything at all.
    fn drawing(body: &str) -> String {
        main_of(&format!(r#"gameInit(10, 8, "test"); {body}"#))
    }

    #[test]
    fn drawing_before_there_is_a_screen_says_so() {
        // The first mistake anyone makes, and the message has to name the
        // call that fixes it rather than reporting a null somewhere.
        assert_eq!(
            main_of("gameClear(0);"),
            "Runtime Error: gameClear() needs a screen; call gameInit(width, height, title) first. [line 1]"
        );
        assert_eq!(
            main_of("print(gameWidth());"),
            "Runtime Error: gameWidth() needs a screen; call gameInit(width, height, title) first. [line 1]"
        );
    }

    #[test]
    fn a_colour_is_a_number_that_comes_back_apart() {
        // Packed, because a colour is used in a draw loop and an object per
        // call would allocate every frame. Reversible, because MRT has no bit
        // shifting to take one apart by hand.
        assert_eq!(
            main_of(
                r#"var c = gameColor(255, 0, 0);
                   print(c);
                   var p = gameColorParts(c);
                   print(p.red, p.green, p.blue, p.alpha);"#
            ),
            "4294901760\n255 0 0 255"
        );
        assert_eq!(
            main_of(
                r#"var p = gameColorParts(gameColorAlpha(1, 2, 3, 4));
                   print(p.red, p.green, p.blue, p.alpha);"#
            ),
            "1 2 3 4"
        );
    }

    #[test]
    fn a_channel_outside_the_range_is_refused_rather_than_clamped() {
        // Clamping would accept gameColor(255, 300, 0) as yellow-green and
        // leave the author wondering why the arithmetic stopped mattering.
        assert_eq!(
            main_of("gameColor(255, 300, 0);"),
            "Runtime Error: gameColor() needs green to be a whole number from 0 to 255, not 300. [line 1]"
        );
        assert_eq!(
            main_of("gameColor(255, -1, 0);"),
            "Runtime Error: gameColor() needs green to be a whole number from 0 to 255, not -1. [line 1]"
        );
        assert_eq!(
            main_of("gameColor(255, 0.5, 0);"),
            "Runtime Error: gameColor() needs green to be a whole number from 0 to 255, not 0.5. [line 1]"
        );
    }

    #[test]
    fn what_is_drawn_can_be_read_back() {
        assert_eq!(
            drawing(
                r#"var red = gameColor(255, 0, 0);
                   gameClear(gameColor(0, 0, 0));
                   gameRect(2, 2, 3, 2, red);
                   print(gameColorAt(2, 2) == red, gameColorAt(4, 3) == red);
                   print(gameColorAt(5, 2) == red, gameColorAt(2, 4) == red);"#
            ),
            "true true\nfalse false"
        );
    }

    #[test]
    fn reading_off_the_surface_is_null_rather_than_an_error() {
        // A game asks about positions that have left the screen constantly;
        // that is not a mistake, so it answers "nothing there".
        assert_eq!(
            drawing("print(gameColorAt(-1, 0), gameColorAt(0, -1), gameColorAt(10, 0));"),
            "null null null"
        );
    }

    #[test]
    fn a_fractional_coordinate_lands_on_the_pixel_below_it() {
        // Floor, not truncate. Truncation rounds towards zero, so -0.5 and
        // 0.5 would be the same pixel and a sprite drifting left across the
        // origin would stutter there and nowhere else.
        assert_eq!(
            drawing(
                r#"var red = gameColor(255, 0, 0);
                   gamePixel(3.9, 2.1, red);
                   print(gameColorAt(3, 2) == red, gameColorAt(4, 2) == red);"#
            ),
            "true false"
        );
        assert_eq!(
            drawing(
                r#"var red = gameColor(255, 0, 0);
                   gamePixel(-0.5, 0, red);
                   print(gameColorAt(0, 0) == red);"#
            ),
            "false",
            "-0.5 floors to -1, which is off the surface"
        );
    }

    #[test]
    fn drawing_off_the_edge_is_ordinary_from_mrt_too() {
        assert_eq!(
            drawing(
                r#"var red = gameColor(255, 0, 0);
                   gameRect(-100, -100, 5, 5, red);
                   gameCircle(500, 500, 40, red);
                   gameLine(-9999, -9999, 9999, 9999, red);
                   print("still here");"#
            ),
            "still here"
        );
    }

    #[test]
    fn translucent_drawing_blends_with_what_is_under_it() {
        assert_eq!(
            drawing(
                r#"gameClear(gameColor(0, 0, 0));
                   gameRect(0, 0, 4, 4, gameColorAlpha(255, 255, 255, 128));
                   var p = gameColorParts(gameColorAt(1, 1));
                   print(p.red, p.alpha);"#
            ),
            "128 255"
        );
    }

    #[test]
    fn a_screen_has_to_have_a_believable_size() {
        // The upper bound is what stops a typo asking for all of memory
        // instead of a picture.
        assert_eq!(
            main_of(r#"gameInit(0, 10, "t");"#),
            "Runtime Error: gameInit() needs a width and height from 1 to 16384, not 0x10. [line 1]"
        );
        assert_eq!(
            main_of(r#"gameInit(99999, 10, "t");"#),
            "Runtime Error: gameInit() needs a width and height from 1 to 16384, not 99999x10. [line 1]"
        );
        assert_eq!(
            main_of(r#"gameInit(2.5, 10, "t");"#),
            "Runtime Error: gameInit() needs the width to be a whole number that is not negative, not 2.5. [line 1]"
        );
        assert_eq!(
            main_of("gameInit(4, 4, 7);"),
            "Runtime Error: gameInit() needs the title to be a string, not number. [line 1]"
        );
    }

    #[test]
    fn calling_init_again_replaces_the_screen() {
        // So a program can resize rather than being stuck with its first
        // guess -- and the new screen starts blank, not holding the old one.
        assert_eq!(
            main_of(
                r#"gameInit(4, 4, "small");
                   gamePixel(1, 1, gameColor(255, 0, 0));
                   gameInit(20, 10, "bigger");
                   print(gameWidth(), gameHeight());
                   var p = gameColorParts(gameColorAt(1, 1));
                   print(p.red, p.green, p.blue);"#
            ),
            "20 10\n0 0 0"
        );
    }

    #[test]
    fn saving_writes_a_png_a_decoder_would_accept() {
        let path = std::env::temp_dir().join("mrt_game_bridge_test.png");
        let _ = std::fs::remove_file(&path);
        let escaped = path.to_string_lossy().replace('\\', "\\\\");

        assert_eq!(
            main_of(&format!(
                r#"gameInit(3, 2, "t");
                   gameClear(gameColor(10, 20, 30));
                   gameSave("{escaped}");
                   print("saved");"#
            )),
            "saved"
        );

        let bytes = std::fs::read(&path).expect("the file was written");
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert_eq!(&bytes[12..16], b"IHDR");
        assert_eq!(&bytes[16..20], &3u32.to_be_bytes());
        assert_eq!(&bytes[20..24], &2u32.to_be_bytes());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_path_that_cannot_be_written_reports_the_reason() {
        let output = main_of(r#"gameInit(2, 2, "t"); gameSave("/nope/nowhere/x.png");"#);
        assert!(
            output.starts_with("Runtime Error: gameSave() could not write /nope/nowhere/x.png:"),
            "got {output:?}"
        );
    }

    #[test]
    fn two_programs_do_not_share_a_screen() {
        // The screen is interpreter state rather than a global, which this
        // test suite depends on more than anything else does: it runs every
        // program twice, once per engine, in one process.
        assert_eq!(main_of(r#"gameInit(4, 4, "a"); print(gameWidth());"#), "4");
        assert_eq!(
            main_of("print(gameWidth());"),
            "Runtime Error: gameWidth() needs a screen; call gameInit(width, height, title) first. [line 1]",
            "the previous program's screen did not leak into this one"
        );
    }
}
