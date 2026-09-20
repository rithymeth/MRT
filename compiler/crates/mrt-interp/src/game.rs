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

/// The engine's screen: the picture, and the window showing it if one is open.
pub struct Screen {
    pub surface: Surface,
    pub title: String,
    /// Offscreen surfaces a program has asked for, addressed by a 1-based id.
    ///
    /// An **id**, not a handle: a plain number is already an MRT value, so
    /// sprites cost the language no new type either -- the same rule the
    /// screen follows, applied to there being more than one of them. A freed
    /// slot becomes `None` and is not reused, so a stale id reports that it
    /// was freed instead of silently drawing some later sprite.
    pub sprites: Vec<Option<Surface>>,
    /// Which surface drawing lands on: 0 is the screen, otherwise a sprite id.
    pub target: usize,
    /// How far the visible screen has scrolled through the world, in world
    /// pixels. `(0, 0)` until `gameCamera` is called.
    ///
    /// Applied to the screen only, never to a sprite: a camera scrolls the
    /// world a program is drawing, not the picture it is still drawing *with*
    /// -- `gameTarget`ing a sprite to pre-render it works in that sprite's own
    /// local space regardless of where the camera happens to be pointed.
    pub camera_x: i32,
    pub camera_y: i32,
    /// The window, once `gameOpen` has made one.
    ///
    /// Opened lazily rather than by `gameInit`, which is what lets a program
    /// that only draws and saves a PNG run on a machine with no display at
    /// all -- a build server, or this repository's own test suite. A program
    /// asks for a window by asking whether one is open.
    #[cfg(feature = "window")]
    pub window: Option<mrt_window::Window>,
    /// Every connected gamepad, once a query has opened the subsystem.
    ///
    /// Opened lazily on the first gamepad call, the same reasoning as the
    /// window and the speaker: a program that never asks about a controller
    /// should run the same on a machine with none.
    #[cfg(feature = "gamepad")]
    pub gamepads: Option<mrt_gamepad::Gamepads>,
    /// Why opening the gamepad subsystem failed, if it did. Remembered so
    /// it is tried once, the same reason `Bank` remembers `speaker_failed`.
    #[cfg(feature = "gamepad")]
    pub gamepads_failed: Option<String>,
}

impl Screen {
    /// The surface drawing currently lands on.
    ///
    /// Every drawing built-in goes through here rather than touching
    /// `surface` directly, which is what makes `gameTarget` a one-line
    /// feature instead of an extra argument on fifteen calls.
    pub fn target(&self) -> &Surface {
        match self.target {
            0 => &self.surface,
            id => self.sprites[id - 1]
                .as_ref()
                .expect("a freed sprite is never left as the target"),
        }
    }

    pub fn target_mut(&mut self) -> &mut Surface {
        match self.target {
            0 => &mut self.surface,
            id => self.sprites[id - 1]
                .as_mut()
                .expect("a freed sprite is never left as the target"),
        }
    }

    /// A world position, translated into a position on the current target.
    ///
    /// The subtraction is plain integer arithmetic, not floating point,
    /// because both operands already went through `coord`'s floor: the
    /// camera is a coordinate like any other and is floored the same way
    /// when it is set, so there is no fractional part left for a second
    /// rounding step to disagree with the first one about.
    pub fn from_world(&self, x: i32, y: i32) -> (i32, i32) {
        if self.target == 0 {
            (x - self.camera_x, y - self.camera_y)
        } else {
            (x, y)
        }
    }

    /// Check an id names a live sprite, and say why not if it does not.
    fn sprite_index(&self, id: usize, who: &str) -> Result<usize, Signal> {
        if id == 0 {
            return Err(value_error(format!(
                "{who}() needs a surface from gameSurface(); 0 is the screen."
            )));
        }
        match self.sprites.get(id - 1) {
            Some(Some(_)) => Ok(id - 1),
            // Told apart on purpose: a freed id is a use-after-free in the
            // program's own logic, and reporting it as "no such surface"
            // would send its author looking for a typo instead.
            Some(None) => Err(value_error(format!("{who}(): surface {id} was freed."))),
            None => Err(value_error(format!("{who}(): there is no surface {id}."))),
        }
    }
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
        sprites: Vec::new(),
        target: 0,
        camera_x: 0,
        camera_y: 0,
        #[cfg(feature = "window")]
        window: None,
        #[cfg(feature = "gamepad")]
        gamepads: None,
        #[cfg(feature = "gamepad")]
        gamepads_failed: None,
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
        .target()
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
    use crate::value::{ObjKey, ObjMap};

    fn key(name: &str) -> ObjKey {
        ObjKey::Str(Rc::from(name))
    }

    pub fn clear(s: &mut Option<Screen>, c: Color) -> Result<Value, Signal> {
        screen(s, "gameClear")?.target_mut().clear(c);
        Ok(Value::Null)
    }

    /// Move the camera to a position in world pixels.
    ///
    /// Only ever affects the screen, per `Screen::from_world` -- setting it
    /// while `gameTarget`ed to a sprite is allowed (a program might scroll
    /// before switching targets and back) but has no visible effect until
    /// the target is the screen again.
    pub fn set_camera(s: &mut Option<Screen>, x: i32, y: i32) -> Result<Value, Signal> {
        let screen = screen(s, "gameCamera")?;
        screen.camera_x = x;
        screen.camera_y = y;
        Ok(Value::Null)
    }

    /// Where the camera currently is: `{0, 0}` until `gameCamera` moves it.
    pub fn camera_position(s: &Option<Screen>) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameCameraPosition"))?;
        let mut map = ObjMap::new();
        map.insert(key("x"), Value::Number(screen.camera_x as f64));
        map.insert(key("y"), Value::Number(screen.camera_y as f64));
        Ok(Value::object(map))
    }

    pub fn pixel(s: &mut Option<Screen>, x: i32, y: i32, c: Color) -> Result<Value, Signal> {
        let screen = screen(s, "gamePixel")?;
        let (x, y) = screen.from_world(x, y);
        screen.target_mut().draw(x, y, c);
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
        let screen = screen(s, "gameRect")?;
        let (x, y) = screen.from_world(x, y);
        screen.target_mut().fill_rect(x, y, w, h, c);
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
        let screen = screen(s, "gameRectOutline")?;
        let (x, y) = screen.from_world(x, y);
        screen.target_mut().stroke_rect(x, y, w, h, t, c);
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
        let screen = screen(s, "gameLine")?;
        let (x0, y0) = screen.from_world(x0, y0);
        let (x1, y1) = screen.from_world(x1, y1);
        screen.target_mut().line(x0, y0, x1, y1, c);
        Ok(Value::Null)
    }

    pub fn circle(
        s: &mut Option<Screen>,
        x: i32,
        y: i32,
        r: i32,
        c: Color,
    ) -> Result<Value, Signal> {
        let screen = screen(s, "gameCircle")?;
        let (x, y) = screen.from_world(x, y);
        screen.target_mut().fill_circle(x, y, r, c);
        Ok(Value::Null)
    }

    pub fn text(
        s: &mut Option<Screen>,
        x: i32,
        y: i32,
        text: &str,
        scale: i32,
        c: Color,
    ) -> Result<Value, Signal> {
        let screen = screen(s, "gameText")?;
        let (x, y) = screen.from_world(x, y);
        screen.target_mut().text(x, y, text, scale, c);
        Ok(Value::Null)
    }

    pub fn circle_outline(
        s: &mut Option<Screen>,
        x: i32,
        y: i32,
        r: i32,
        c: Color,
    ) -> Result<Value, Signal> {
        let screen = screen(s, "gameCircleOutline")?;
        let (x, y) = screen.from_world(x, y);
        screen.target_mut().stroke_circle(x, y, r, c);
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

    /// The window built-ins behave differently depending on how the crate
    /// was built, so the tests do too. Both arrangements are real and both
    /// ship: `cargo test -p mrt-interp` runs without the feature, and a
    /// workspace build turns it on through the CLI.
    #[test]
    #[cfg(not(feature = "window"))]
    fn a_build_without_window_support_says_which_it_is() {
        // Not a quiet `false`. A loop whose gameOpen() silently returned
        // false would look exactly like a window that closed immediately,
        // and the author would go hunting in their own program.
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameOpen();"#),
            "Runtime Error: gameOpen() needs a build with window support; this one was built without it. [line 1]"
        );
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameKeyDown("W");"#),
            "Runtime Error: gameKeyDown() needs a build with window support; this one was built without it. [line 1]"
        );
    }

    #[test]
    #[cfg(feature = "window")]
    fn with_no_display_opening_a_window_is_an_error_a_program_can_catch() {
        // This is the property examples/game_bounce.mrt is built on: it runs
        // on a build server by catching this. A panic here would abort the
        // program instead, and there would be no way to write one game that
        // runs both places.
        //
        // Skipped where a display exists, since there the window opens.
        if std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some() {
            return;
        }
        let output = main_of(
            r#"gameInit(4, 4, "t");
               try { gameOpen(); } catch (e) { print("caught", e.kind); }"#,
        );
        assert_eq!(output, "caught ValueError");
    }

    #[test]
    #[cfg(feature = "window")]
    fn presenting_before_opening_names_the_call_that_fixes_it() {
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gamePresent();"#),
            "Runtime Error: gamePresent() needs a window; call gameOpen() first. [line 1]"
        );
    }

    #[test]
    #[cfg(not(feature = "gamepad"))]
    fn a_build_without_gamepad_support_says_which_it_is() {
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameGamepadCount();"#),
            "Runtime Error: gameGamepadCount() needs a build with gamepad support; this one was built without it. [line 1]"
        );
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameGamepadButtonDown(0, "A");"#),
            "Runtime Error: gameGamepadButtonDown() needs a build with gamepad support; this one was built without it. [line 1]"
        );
    }

    #[test]
    #[cfg(feature = "gamepad")]
    fn with_no_controllers_connected_every_query_answers_rather_than_erroring() {
        // gilrs opens fine with zero gamepads plugged in -- the same promise
        // opening the speaker or the window makes about a machine with none
        // of those either -- so this is the ordinary case, not a fallback
        // path only a build server takes.
        assert_eq!(
            main_of(
                r#"gameInit(4, 4, "t");
                   print(gameGamepadCount());
                   print(gameGamepadButtonDown(0, "A"));
                   print(gameGamepadButtonPressed(0, "A"));
                   print(gameGamepadAxis(0, "LeftX"));"#
            ),
            "0\nfalse\nfalse\n0"
        );
    }

    #[test]
    #[cfg(feature = "gamepad")]
    fn an_unrecognised_button_or_axis_name_is_never_down_rather_than_an_error() {
        // The same rule gameKeyDown follows for a key name it does not
        // recognise: a typo reads as "not held", not a crash.
        assert_eq!(
            main_of(
                r#"gameInit(4, 4, "t");
                   print(gameGamepadButtonDown(0, "Nonsense"));
                   print(gameGamepadAxis(0, "Nonsense"));"#
            ),
            "false\n0"
        );
    }

    #[test]
    #[cfg(feature = "gamepad")]
    fn gamepad_queries_need_no_window_at_all() {
        // Polled from the interpreter's screen, not from mrt-window: a
        // program checking for a controller headlessly -- a server, a test
        // -- should not have to open a display first.
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); print(gameGamepadCount());"#),
            "0"
        );
    }

    #[test]
    fn text_draws_and_measures() {
        assert_eq!(
            drawing(
                r#"var ink = gameColor(255, 255, 255);
                   gameText(0, 0, "L", 1, ink);
                   print(gameColorAt(0, 0) == ink, gameColorAt(4, 6) == ink);
                   print(gameColorAt(4, 0) == ink);"#
            ),
            "true true\nfalse",
            "an L is a full left bar and a full bottom bar, and nothing top-right"
        );
    }

    #[test]
    fn measuring_text_needs_no_screen() {
        // A program laying out a menu before it opens a window should not
        // have to call gameInit to find out how wide a word is.
        assert_eq!(
            main_of(r#"print(gameTextWidth("Hi", 1), gameTextHeight("Hi", 1));"#),
            "11 7"
        );
        assert_eq!(
            main_of(r#"print(gameTextWidth("Hi", 3), gameTextHeight("a\nb", 2));"#),
            "33 30"
        );
        assert_eq!(
            main_of(r#"print(gameTextWidth("", 2));"#),
            "0",
            "nothing is no wide"
        );
    }

    #[test]
    fn an_unknown_character_is_visible_rather_than_silently_dropped() {
        // Text that quietly loses characters reads as a bug in the program's
        // own logic, and sends the author looking in the wrong place.
        assert_eq!(
            drawing(
                r#"var ink = gameColor(255, 255, 255);
                   gameText(0, 0, "\t", 1, ink);
                   var lit = 0;
                   for (var y = 0; y < 7; y = y + 1) {
                       for (var x = 0; x < 5; x = x + 1) {
                           if (gameColorAt(x, y) == ink) { lit = lit + 1; }
                       }
                   }
                   print(lit > 0);"#
            ),
            "true"
        );
    }

    #[test]
    fn a_sprite_is_drawn_once_and_stamped_many_times() {
        assert_eq!(
            main_of(
                r#"gameInit(20, 10, "t");
                   gameClear(gameColor(0, 0, 0));
                   var red = gameColor(255, 0, 0);
                   var dot = gameSurface(2, 2);
                   gameTarget(dot);
                   gameRect(0, 0, 2, 2, red);
                   gameTarget(0);
                   gameDraw(dot, 1, 1);
                   gameDraw(dot, 10, 4);
                   print(gameColorAt(1, 1) == red, gameColorAt(11, 5) == red);
                   print(gameColorAt(5, 5) == red);"#
            ),
            "true true\nfalse"
        );
    }

    #[test]
    fn gamedraw_without_options_is_unchanged() {
        // The three-argument call is what every program written before
        // rotation and scale existed already makes, so it has to keep
        // meaning exactly what it always meant.
        assert_eq!(
            main_of(
                r#"gameInit(10, 10, "t");
                   var red = gameColor(255, 0, 0);
                   var dot = gameSurface(2, 2);
                   gameTarget(dot);
                   gameRect(0, 0, 2, 2, red);
                   gameTarget(0);
                   gameDraw(dot, 3, 3, null);
                   print(gameColorAt(3, 3) == red, gameColorAt(4, 4) == red);"#
            ),
            "true true"
        );
    }

    #[test]
    fn gamedraw_can_rotate_and_scale_a_sprite() {
        assert_eq!(
            main_of(
                r#"gameInit(20, 20, "t");
                   var red = gameColor(255, 0, 0);
                   var bar = gameSurface(4, 2);
                   gameTarget(bar);
                   gameRect(0, 0, 4, 2, red);
                   gameTarget(0);
                   // A quarter turn about the sprite's own top-left corner:
                   // 4 wide by 2 tall becomes 2 wide (columns 9 and 10) by
                   // 4 tall (rows 10 through 13).
                   gameDraw(bar, 10, 10, {angle: 1.5707963267948966});
                   print(gameColorAt(9, 10) == red, gameColorAt(8, 10) == red);
                   print(gameColorAt(10, 13) == red, gameColorAt(10, 14) == red);
                   // Scaled up 3x with no rotation: a solid block, no seams.
                   gameDraw(bar, 0, 0, {scaleX: 3, scaleY: 3});
                   print(gameColorAt(11, 5) == red, gameColorAt(0, 0) == red);"#
            ),
            "true false\ntrue false\ntrue true"
        );
    }

    #[test]
    fn gamedraw_options_are_type_checked_like_any_other_object_argument() {
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var s = gameSurface(2, 2);
                   gameDraw(s, 0, 0, 5);"#
            ),
            "Runtime Error: gameDraw() needs the options to be an object or null, not number. [line 3]"
        );
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var s = gameSurface(2, 2);
                   gameDraw(s, 0, 0, {angle: "sideways"});"#
            ),
            "Runtime Error: gameDraw(): angle must be a real number, not sideways. [line 3]"
        );
    }

    #[test]
    fn gamedraw_can_crop_to_one_frame_of_a_sprite_sheet() {
        assert_eq!(
            main_of(
                r#"gameInit(20, 20, "t");
                   var red = gameColor(255, 0, 0);
                   var blue = gameColor(0, 0, 255);
                   // A 2-frame sheet: left frame red, right frame blue.
                   var sheet = gameSurface(4, 2);
                   gameTarget(sheet);
                   gameRect(0, 0, 2, 2, red);
                   gameRect(2, 0, 2, 2, blue);
                   gameTarget(0);
                   // Drawing just the right (blue) frame should not paint
                   // any red at all.
                   gameDraw(sheet, 0, 0, {srcX: 2, srcY: 0, srcWidth: 2, srcHeight: 2});
                   print(gameColorAt(0, 0) == blue, gameColorAt(1, 1) == blue);
                   print(gameColorAt(2, 0) == red, gameColorAt(2, 0) == blue);"#
            ),
            "true true\nfalse false"
        );
    }

    #[test]
    fn gamedraw_requires_all_four_source_rectangle_fields_together() {
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var s = gameSurface(4, 4);
                   gameDraw(s, 0, 0, {srcX: 0, srcY: 0, srcWidth: 2});"#
            ),
            "Runtime Error: gameDraw(): srcX, srcY, srcWidth and srcHeight must be given together, or not at all. [line 3]"
        );
    }

    #[test]
    fn gamedraw_rejects_a_source_rectangle_that_does_not_fit_the_sprite() {
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var s = gameSurface(4, 4);
                   gameDraw(s, 0, 0, {srcX: 2, srcY: 0, srcWidth: 4, srcHeight: 4});"#
            ),
            "Runtime Error: gameDraw(): the source rectangle (2, 0, 4, 4) does not fit inside surface 1 (4x4). [line 3]"
        );
    }

    #[test]
    fn a_new_sprite_is_transparent_rather_than_black() {
        // A sprite is a shape with nothing around it. One that began opaque
        // would stamp a rectangle of background over whatever it landed on,
        // and clearing it first is a step that is only ever forgotten once --
        // but is always forgotten once.
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var blue = gameColor(0, 0, 255);
                   gameClear(blue);
                   var s = gameSurface(4, 4);
                   gameTarget(s);
                   gameRect(0, 0, 1, 1, gameColor(255, 0, 0));
                   gameTarget(0);
                   gameDraw(s, 2, 2);
                   print(gameColorAt(3, 3) == blue);"#
            ),
            "true",
            "the untouched part of the sprite let the background through"
        );
    }

    #[test]
    fn the_target_is_what_gets_drawn_measured_and_read() {
        // Everything follows the target together, or a program that sets one
        // would draw into a sprite while reading from the screen.
        assert_eq!(
            main_of(
                r#"gameInit(40, 30, "t");
                   var s = gameSurface(4, 6);
                   print(gameWidth(), gameHeight(), gameTargetId());
                   gameTarget(s);
                   print(gameWidth(), gameHeight(), gameTargetId());
                   var red = gameColor(255, 0, 0);
                   gamePixel(0, 0, red);
                   print(gameColorAt(0, 0) == red);
                   gameTarget(0);
                   print(gameColorAt(0, 0) == red, gameWidth());"#
            ),
            "40 30 0\n4 6 1\ntrue\nfalse 40"
        );
    }

    #[test]
    fn a_surface_cannot_be_drawn_onto_itself() {
        // Not a pedantic check: the implementation lifts the source out of
        // its slot to blit it, so drawing onto itself would blit from an
        // empty one.
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var s = gameSurface(4, 4);
                   gameTarget(s);
                   gameDraw(s, 0, 0);"#
            ),
            "Runtime Error: gameDraw(): surface 1 cannot be drawn onto itself. [line 4]"
        );
    }

    #[test]
    fn a_freed_surface_is_told_apart_from_one_that_never_existed() {
        // A freed id is a use-after-free in the program's own logic; calling
        // it "no such surface" would send its author looking for a typo.
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var s = gameSurface(2, 2);
                   gameSurfaceFree(s);
                   gameDraw(s, 0, 0);"#
            ),
            "Runtime Error: gameDraw(): surface 1 was freed. [line 4]"
        );
        assert_eq!(
            main_of(r#"gameInit(8, 8, "t"); gameDraw(7, 0, 0);"#),
            "Runtime Error: gameDraw(): there is no surface 7. [line 1]"
        );
        assert_eq!(
            main_of(r#"gameInit(8, 8, "t"); gameDraw(0, 0, 0);"#),
            "Runtime Error: gameDraw() needs a surface from gameSurface(); 0 is the screen. [line 1]"
        );
    }

    #[test]
    fn freeing_the_surface_being_drawn_into_is_refused() {
        // Otherwise the next draw reaches for a slot that is now empty.
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var s = gameSurface(2, 2);
                   gameTarget(s);
                   gameSurfaceFree(s);"#
            ),
            "Runtime Error: gameSurfaceFree(): surface 1 is the current target; call gameTarget(0) first. [line 4]"
        );
    }

    #[test]
    fn ids_are_not_reused_after_a_free() {
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var a = gameSurface(2, 2);
                   gameSurfaceFree(a);
                   var b = gameSurface(2, 2);
                   print(a, b);"#
            ),
            "1 2",
            "a stale id never starts naming some later sprite"
        );
    }

    #[test]
    fn an_image_can_be_saved_and_loaded_from_mrt() {
        let dir = std::env::temp_dir().join("mrt_game_load_test.png");
        let escaped = dir.to_string_lossy().replace('\\', "\\\\");
        let _ = std::fs::remove_file(&dir);

        assert_eq!(
            main_of(&format!(
                r#"gameInit(8, 8, "t");
                   var red = gameColor(255, 0, 0);
                   var s = gameSurface(4, 3);
                   gameTarget(s);
                   gamePixel(1, 1, red);
                   gameSave("{escaped}");
                   gameTarget(0);
                   var loaded = gameLoad("{escaped}");
                   var size = gameSurfaceSize(loaded);
                   print(size.width, size.height);
                   gameTarget(loaded);
                   print(gameColorAt(1, 1) == red);
                   print(gameColorParts(gameColorAt(0, 0)).alpha);"#
            )),
            "4 3\ntrue\n0",
            "the pixel and the transparency both survived"
        );
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn a_loaded_image_is_an_ordinary_sprite() {
        // Ids come from the same counter, so loading and drawing are not two
        // separate worlds a program has to keep straight.
        let path = std::env::temp_dir().join("mrt_game_load_ids.png");
        let escaped = path.to_string_lossy().replace('\\', "\\\\");
        assert_eq!(
            main_of(&format!(
                r#"gameInit(8, 8, "t");
                   var a = gameSurface(2, 2);
                   gameTarget(a);
                   gameRect(0, 0, 2, 2, gameColor(0, 255, 0));
                   gameSave("{escaped}");
                   gameTarget(0);
                   var b = gameLoad("{escaped}");
                   print(a, b);
                   gameClear(gameColor(0, 0, 0));
                   gameDraw(b, 3, 3);
                   print(gameColorAt(3, 3) == gameColor(0, 255, 0));"#
            )),
            "1 2\ntrue"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loading_something_that_is_not_an_image_says_why() {
        let path = std::env::temp_dir().join("mrt_game_not_png.png");
        std::fs::write(&path, b"nope").expect("writes");
        let escaped = path.to_string_lossy().replace('\\', "\\\\");
        let output = main_of(&format!(r#"gameInit(4, 4, "t"); gameLoad("{escaped}");"#));
        let _ = std::fs::remove_file(&path);
        assert!(
            output.contains("gameLoad():") && output.contains("signature"),
            "got {output:?}"
        );

        let missing = main_of(r#"gameInit(4, 4, "t"); gameLoad("/nope/nowhere.png");"#);
        assert!(missing.contains("could not read"), "got {missing:?}");
    }

    #[test]
    fn the_screens_size_is_available_without_switching_to_it() {
        assert_eq!(
            main_of(
                r#"gameInit(40, 30, "t");
                   var s = gameSurface(4, 6);
                   gameTarget(s);
                   var screen = gameSurfaceSize(0);
                   print(screen.width, screen.height, gameWidth());"#
            ),
            "40 30 4",
            "gameSurfaceSize(0) is the screen even while a sprite is the target"
        );
    }

    #[test]
    fn the_camera_shifts_where_drawing_and_reading_land() {
        assert_eq!(
            main_of(
                r#"gameInit(20, 20, "t");
                   print(gameCameraPosition().x, gameCameraPosition().y);
                   var red = gameColor(255, 0, 0);
                   gameCamera(5, 5);
                   print(gameCameraPosition().x, gameCameraPosition().y);
                   // gameColorAt takes a world position too, for the same
                   // reason a thermostat reads the same scale it is set on:
                   // a pixel drawn at world (8, 8) is read back at (8, 8),
                   // regardless of where the camera happens to be pointed.
                   gamePixel(8, 8, red);
                   print(gameColorAt(8, 8) == red, gameColorAt(3, 3) == red);"#
            ),
            "0 0\n5 5\ntrue false"
        );
    }

    #[test]
    fn the_camera_never_reaches_a_sprite_being_drawn_on() {
        // Switching gameTarget to a sprite is switching *what* is being
        // drawn, not scrolling *through* it -- a sprite pre-rendered while
        // the camera happens to be elsewhere must come out identical to one
        // pre-rendered with the camera at the origin.
        assert_eq!(
            main_of(
                r#"gameInit(20, 20, "t");
                   gameCamera(100, 100);
                   var red = gameColor(255, 0, 0);
                   var s = gameSurface(4, 4);
                   gameTarget(s);
                   gamePixel(1, 1, red);
                   print(gameColorAt(1, 1) == red);
                   gameTarget(0);
                   // world (102, 102) lands at screen (2, 2) with this
                   // camera, and the sprite's own (1, 1) pixel one further
                   // in -- read back at the matching world position (103,
                   // 103), the sprite's contents having scrolled with it
                   // even though drawing *into* the sprite did not.
                   gameDraw(s, 102, 102);
                   print(gameColorAt(103, 103) == red);"#
            ),
            "true\ntrue"
        );
    }

    #[test]
    fn a_line_shifts_both_endpoints_together() {
        assert_eq!(
            main_of(
                r#"gameInit(20, 20, "t");
                   gameCamera(2, 0);
                   var white = gameColor(255, 255, 255);
                   gameLine(0, 5, 10, 5, white);
                   print(gameColorAt(0, 5) == white, gameColorAt(8, 5) == white);"#
            ),
            "false true",
            "both endpoints moved by the same amount, so the line is unbroken"
        );
    }

    #[test]
    fn collision_needs_no_screen_at_all() {
        // It is arithmetic on boxes. A program doing physics headlessly --
        // a server, a test -- should not have to open a framebuffer first.
        assert_eq!(
            main_of(
                r#"var a = {x: 0, y: 0, width: 10, height: 10};
                   var b = {x: 5, y: 5, width: 10, height: 10};
                   var c = {x: 50, y: 50, width: 10, height: 10};
                   print(gameOverlap(a, b), gameOverlap(a, c));"#
            ),
            "true false"
        );
    }

    #[test]
    fn touching_boxes_do_not_count_as_overlapping() {
        // Tiles laid edge to edge are the common case; the other answer
        // reports a collision at every seam of a level.
        assert_eq!(
            main_of(
                r#"var a = {x: 0, y: 0, width: 10, height: 10};
                   print(gameOverlap(a, {x: 10, y: 0, width: 10, height: 10}));
                   print(gameOverlap(a, {x: 9.99, y: 0, width: 10, height: 10}));"#
            ),
            "false\ntrue"
        );
    }

    #[test]
    fn resolving_gives_the_shortest_push_on_one_axis() {
        assert_eq!(
            main_of(
                r#"var a = {x: 8, y: 5, width: 10, height: 10};
                   var wall = {x: 16, y: 0, width: 20, height: 20};
                   var push = gameResolve(a, wall);
                   print(push.x, push.y);
                   print(gameResolve(a, {x: 90, y: 90, width: 2, height: 2}));"#
            ),
            "-2 0\nnull"
        );
    }

    #[test]
    fn a_sweep_catches_the_wall_a_frame_check_would_miss() {
        // The reason gameSweep exists. Clear of the wall before the step and
        // clear of it after, so gameOverlap on either position sees nothing.
        assert_eq!(
            main_of(
                r#"var mover = {x: 0, y: 0, width: 4, height: 4};
                   var after = {x: 200, y: 0, width: 4, height: 4};
                   var wall = {x: 100, y: 0, width: 2, height: 10};
                   print(gameOverlap(mover, wall), gameOverlap(after, wall));
                   var hit = gameSweep(mover, 200, 0, wall);
                   print(hit.time, hit.normalX, hit.normalY, hit.x);"#
            ),
            "false false\n0.48 -1 0 96"
        );
    }

    #[test]
    fn a_sweep_that_hits_nothing_is_null() {
        assert_eq!(
            main_of(
                r#"var mover = {x: 0, y: 0, width: 4, height: 4};
                   var wall = {x: 100, y: 0, width: 2, height: 10};
                   print(gameSweep(mover, 50, 0, wall));
                   print(gameSweep(mover, -50, 0, wall));"#
            ),
            "null\nnull"
        );
    }

    #[test]
    fn a_box_that_is_not_one_says_which_field_is_missing() {
        assert_eq!(
            main_of(r#"gameOverlap({x: 0, y: 0, width: 1}, {x: 0, y: 0, width: 1, height: 1});"#),
            "Runtime Error: gameOverlap(): the first box has no 'height'. [line 1]"
        );
        assert_eq!(
            main_of(r#"gameOverlap(5, {x: 0, y: 0, width: 1, height: 1});"#),
            "Runtime Error: gameOverlap() needs the first box to be a box object with x, y, width and height, not number. [line 1]"
        );
        assert_eq!(
            main_of(
                r#"gameOverlap({x: 0, y: 0, width: "a", height: 1}, {x: 0, y: 0, width: 1, height: 1});"#
            ),
            "Runtime Error: gameOverlap(): the first box.width is string, not a number. [line 1]"
        );
    }

    #[test]
    fn a_backwards_box_is_refused_rather_than_answered() {
        // Every test would answer something for a box whose right edge is
        // left of its left one, and every answer would be wrong.
        assert_eq!(
            main_of(
                r#"gameOverlap({x: 0, y: 0, width: -5, height: 1}, {x: 0, y: 0, width: 1, height: 1});"#
            ),
            "Runtime Error: gameOverlap(): the first box has a negative size, -5x1. [line 1]"
        );
    }

    #[test]
    fn the_loop_builtins_still_need_a_screen_first() {
        // Whichever way the crate was built, asking about a window before
        // there is anything to show reports the missing gameInit rather than
        // whatever comes second.
        assert_eq!(
            main_of("gamePresent();"),
            "Runtime Error: gamePresent() needs a screen; call gameInit(width, height, title) first. [line 1]"
        );
        assert_eq!(
            main_of("gameClose();"),
            "Runtime Error: gameClose() needs a screen; call gameInit(width, height, title) first. [line 1]"
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

/// Collision: boxes, and what happens when they meet.
///
/// A box crosses as an ordinary MRT object, `{x, y, width, height}`. Eight
/// loose numbers per call would read as noise at the call site and invite
/// transposing two of them, which is a bug no error message can catch. An
/// object is data the language already has, so this costs it no new type --
/// the same rule the rest of the extension follows.
pub mod collide {
    use super::*;
    use crate::value::{ObjKey, ObjMap};
    use mrt_game::collide::Aabb;

    /// An object key, spelled once.
    fn key(name: &str) -> ObjKey {
        ObjKey::Str(Rc::from(name))
    }

    /// Read `{x, y, width, height}` out of an MRT object.
    fn box_of(value: &Value, who: &str, which: &str) -> Result<Aabb, Signal> {
        let Value::Object(map) = value else {
            return Err(type_error(format!(
                "{who}() needs {which} to be a box object with x, y, width and height, not {}.",
                crate::value::type_name(value)
            )));
        };
        let map = map.borrow();
        let field = |name: &str| -> Result<f64, Signal> {
            match map.get(&key(name)) {
                Some(Value::Number(n)) if n.is_finite() => Ok(*n),
                Some(Value::Number(n)) => {
                    Err(value_error(format!("{who}(): {which}.{name} is {n}.")))
                }
                Some(other) => Err(type_error(format!(
                    "{who}(): {which}.{name} is {}, not a number.",
                    crate::value::type_name(other)
                ))),
                None => Err(value_error(format!("{who}(): {which} has no '{name}'."))),
            }
        };
        let width = field("width")?;
        let height = field("height")?;
        // A negative size describes a box whose right edge is left of its
        // left one. Every test below would answer something for it, and all
        // of the answers would be wrong.
        if width < 0.0 || height < 0.0 {
            return Err(value_error(format!(
                "{who}(): {which} has a negative size, {width}x{height}."
            )));
        }
        Ok(Aabb::new(field("x")?, field("y")?, width, height))
    }

    pub fn overlap(a: &Value, b: &Value) -> Result<Value, Signal> {
        Ok(Value::Bool(mrt_game::collide::overlap(
            &box_of(a, "gameOverlap", "the first box")?,
            &box_of(b, "gameOverlap", "the second box")?,
        )))
    }

    /// How far to move the first box to separate it, or `null` if apart.
    pub fn resolve(a: &Value, b: &Value) -> Result<Value, Signal> {
        let a = box_of(a, "gameResolve", "the first box")?;
        let b = box_of(b, "gameResolve", "the second box")?;
        Ok(match mrt_game::collide::resolve(&a, &b) {
            Some((x, y)) => {
                let mut map = ObjMap::new();
                map.insert(key("x"), Value::Number(x));
                map.insert(key("y"), Value::Number(y));
                Value::object(map)
            }
            None => Value::Null,
        })
    }

    /// Where a moving box first touches a stationary one, or `null`.
    pub fn sweep(a: &Value, dx: f64, dy: f64, b: &Value) -> Result<Value, Signal> {
        let a = box_of(a, "gameSweep", "the moving box")?;
        let b = box_of(b, "gameSweep", "the box it might hit")?;
        Ok(match mrt_game::collide::sweep(&a, dx, dy, &b) {
            Some(hit) => {
                let mut map = ObjMap::new();
                map.insert(key("time"), Value::Number(hit.time));
                map.insert(key("normalX"), Value::Number(hit.normal_x));
                map.insert(key("normalY"), Value::Number(hit.normal_y));
                // Where it ends up at the moment of contact: touching, not
                // inside. Computed here because every caller wants it and
                // getting it slightly wrong leaves things embedded in walls.
                map.insert(key("x"), Value::Number(a.x + dx * hit.time));
                map.insert(key("y"), Value::Number(a.y + dy * hit.time));
                Value::object(map)
            }
            None => Value::Null,
        })
    }
}

/// Offscreen surfaces: the sprites a program draws once and stamps many times.
pub mod sprites {
    use super::*;
    use crate::value::{ObjKey, ObjMap};

    fn key(name: &str) -> ObjKey {
        ObjKey::Str(Rc::from(name))
    }

    /// Make an offscreen surface and hand back its id.
    ///
    /// It starts **transparent**, not black. A sprite is a shape with nothing
    /// around it; one that began opaque would stamp a rectangle of background
    /// over whatever it landed on, and the author would have to know to clear
    /// it to transparent first -- a step that is only ever forgotten once,
    /// but is always forgotten once.
    pub fn create(s: &mut Option<Screen>, width: usize, height: usize) -> Result<Value, Signal> {
        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Err(value_error(format!(
                "gameSurface() needs a width and height from 1 to 16384, not {width}x{height}."
            )));
        }
        let screen = screen(s, "gameSurface")?;
        screen.sprites.push(Some(Surface::new(
            width as u32,
            height as u32,
            mrt_game::TRANSPARENT,
        )));
        Ok(Value::Number(screen.sprites.len() as f64))
    }

    /// Load a PNG into a new surface and hand back its id.
    ///
    /// A loaded image is an ordinary sprite -- same ids, same `gameDraw`,
    /// same `gameTarget` if a program wants to draw on top of it. Nothing
    /// about it is a new kind of thing, which is the point.
    pub fn load(s: &mut Option<Screen>, path: &str) -> Result<Value, Signal> {
        let screen = screen(s, "gameLoad")?;
        let surface =
            Surface::load_png(path).map_err(|e| value_error(format!("gameLoad(): {e}")))?;
        screen.sprites.push(Some(surface));
        Ok(Value::Number(screen.sprites.len() as f64))
    }

    /// How big a surface is, without making it the target to ask.
    pub fn size(s: &Option<Screen>, id: usize) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameSurfaceSize"))?;
        let surface = if id == 0 {
            &screen.surface
        } else {
            let index = screen.sprite_index(id, "gameSurfaceSize")?;
            screen.sprites[index].as_ref().expect("checked live")
        };
        let mut map = ObjMap::new();
        map.insert(key("width"), Value::Number(surface.width as f64));
        map.insert(key("height"), Value::Number(surface.height as f64));
        Ok(Value::object(map))
    }

    /// Point subsequent drawing at a surface. 0 is the screen.
    pub fn target(s: &mut Option<Screen>, id: usize) -> Result<Value, Signal> {
        let screen = screen(s, "gameTarget")?;
        if id != 0 {
            screen.sprite_index(id, "gameTarget")?;
        }
        screen.target = id;
        Ok(Value::Null)
    }

    /// Which surface drawing is landing on.
    pub fn current_target(s: &Option<Screen>) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameTargetId"))?;
        Ok(Value::Number(screen.target as f64))
    }

    /// A rotation and/or scale for `gameDraw`, anchored at a point in the
    /// sprite's own pixel space.
    ///
    /// The default is angle 0, scale 1, anchored at the sprite's own
    /// top-left corner (0, 0) -- exactly what plain `gameDraw(id, x, y)` has
    /// always meant. `draw` below is defined *in terms of* this default
    /// rather than the other way around, so the two cannot drift apart the
    /// way two independent implementations of "no transform" eventually do.
    pub struct Transform {
        pub angle: f64,
        pub scale_x: f64,
        pub scale_y: f64,
        pub anchor_x: f64,
        pub anchor_y: f64,
        /// One frame of a sprite sheet: `(x, y, width, height)` in the
        /// sprite's own pixel space, or `None` for the whole sprite. Left
        /// unchecked against the sprite's actual size here -- this is built
        /// from the options object alone, before `draw_transformed` knows
        /// which sprite it will be applied to.
        pub region: Option<(i32, i32, u32, u32)>,
    }

    impl Default for Transform {
        fn default() -> Transform {
            Transform {
                angle: 0.0,
                scale_x: 1.0,
                scale_y: 1.0,
                anchor_x: 0.0,
                anchor_y: 0.0,
                region: None,
            }
        }
    }

    /// Read `{angle, scaleX, scaleY, anchorX, anchorY, srcX, srcY, srcWidth,
    /// srcHeight}`, filling in what it leaves out -- the same shape
    /// `soundPlay`'s options take, for the same reason: a game calls this
    /// every frame, and a positional `gameDraw(id, x, y, 0, 1.5, 1.5, 8, 8)`
    /// says nothing at the call site about which number is which.
    pub fn transform_of(value: &Value, who: &str) -> Result<Transform, Signal> {
        let mut t = Transform::default();
        if matches!(value, Value::Null) {
            return Ok(t);
        }
        let Value::Object(map) = value else {
            return Err(type_error(format!(
                "{who}() needs the options to be an object or null, not {}.",
                crate::value::type_name(value)
            )));
        };
        let map = map.borrow();
        let number = |name: &str, fallback: f64| -> Result<f64, Signal> {
            match map.get(&key(name)) {
                None => Ok(fallback),
                Some(Value::Number(n)) if n.is_finite() => Ok(*n),
                Some(other) => Err(value_error(format!(
                    "{who}(): {name} must be a real number, not {}.",
                    crate::value::stringify(other)
                ))),
            }
        };
        t.angle = number("angle", t.angle)?;
        t.scale_x = number("scaleX", t.scale_x)?;
        t.scale_y = number("scaleY", t.scale_y)?;
        t.anchor_x = number("anchorX", t.anchor_x)?;
        t.anchor_y = number("anchorY", t.anchor_y)?;

        // srcX/srcY/srcWidth/srcHeight crop the sprite to one frame of a
        // sheet before the rest of the transform applies. They come as a
        // set -- a caller who names one but not the other three almost
        // certainly meant all four and mistyped one, and silently treating
        // the missing ones as "whole sprite" would draw a frame nobody
        // asked for instead of raising it.
        let names = ["srcX", "srcY", "srcWidth", "srcHeight"];
        let present: Vec<bool> = names.iter().map(|n| map.contains_key(&key(n))).collect();
        if present.iter().any(|p| *p) {
            if !present.iter().all(|p| *p) {
                return Err(value_error(format!(
                    "{who}(): srcX, srcY, srcWidth and srcHeight must be given together, or not at all."
                )));
            }
            let src_x = coord(map.get(&key("srcX")).expect("checked present"), who, "srcX")?;
            let src_y = coord(map.get(&key("srcY")).expect("checked present"), who, "srcY")?;
            let src_w = dimension(
                map.get(&key("srcWidth")).expect("checked present"),
                who,
                "srcWidth",
            )?;
            let src_h = dimension(
                map.get(&key("srcHeight")).expect("checked present"),
                who,
                "srcHeight",
            )?;
            t.region = Some((src_x, src_y, src_w, src_h));
        }
        Ok(t)
    }

    /// A sprite-sheet frame's width or height: a whole number of pixels,
    /// at least 1. Not bounded above here -- whether it actually fits
    /// inside the sprite it crops is checked once `draw_transformed` knows
    /// which sprite that is.
    fn dimension(value: &Value, who: &str, what: &str) -> Result<u32, Signal> {
        match value {
            Value::Number(n) if n.is_finite() && *n >= 1.0 => Ok(n.floor() as u32),
            Value::Number(n) => Err(value_error(format!(
                "{who}(): {what} must be at least 1, not {n}."
            ))),
            other => Err(type_error(format!(
                "{who}() needs {what} to be a number, not {}.",
                crate::value::type_name(other)
            ))),
        }
    }

    /// Stamp a sprite onto the current target, blending it.
    pub fn draw(s: &mut Option<Screen>, id: usize, x: i32, y: i32) -> Result<Value, Signal> {
        draw_transformed(s, id, x, y, &Transform::default())
    }

    /// `draw`, rotated and/or scaled about a point in the sprite's own pixel
    /// space -- see `Surface::blit_transformed` for the geometry.
    pub fn draw_transformed(
        s: &mut Option<Screen>,
        id: usize,
        x: i32,
        y: i32,
        transform: &Transform,
    ) -> Result<Value, Signal> {
        let screen = screen(s, "gameDraw")?;
        let index = screen.sprite_index(id, "gameDraw")?;
        if screen.target == id {
            return Err(value_error(format!(
                "gameDraw(): surface {id} cannot be drawn onto itself."
            )));
        }
        let sprite = screen.sprites[index].as_ref().expect("checked live");
        if let Some((rx, ry, rw, rh)) = transform.region {
            let (fits_x, fits_y) = (
                rx >= 0 && (rx as i64) + (rw as i64) <= sprite.width as i64,
                ry >= 0 && (ry as i64) + (rh as i64) <= sprite.height as i64,
            );
            if !fits_x || !fits_y {
                return Err(value_error(format!(
                    "gameDraw(): the source rectangle ({rx}, {ry}, {rw}, {rh}) does not fit inside surface {id} ({}x{}).",
                    sprite.width, sprite.height
                )));
            }
        }
        let (x, y) = screen.from_world(x, y);
        // Lifted out and put back rather than cloned: the source and the
        // target are two entries of one Vec, and this is the cheap way to
        // convince the compiler they are different ones. The self-draw check
        // above is what makes it sound -- taking the target would leave the
        // slot empty underneath the blit.
        let sprite = screen.sprites[index].take().expect("checked live");
        screen.target_mut().blit_transformed(
            &sprite,
            x,
            y,
            transform.angle,
            transform.scale_x,
            transform.scale_y,
            transform.anchor_x,
            transform.anchor_y,
            transform.region,
        );
        screen.sprites[index] = Some(sprite);
        Ok(Value::Null)
    }

    /// Give a surface up.
    ///
    /// Its id is never reused, so a program still holding one is told the
    /// surface was freed rather than quietly drawing whatever was made next.
    pub fn free(s: &mut Option<Screen>, id: usize) -> Result<Value, Signal> {
        let screen = screen(s, "gameSurfaceFree")?;
        let index = screen.sprite_index(id, "gameSurfaceFree")?;
        if screen.target == id {
            return Err(value_error(format!(
                "gameSurfaceFree(): surface {id} is the current target; call gameTarget(0) first."
            )));
        }
        screen.sprites[index] = None;
        Ok(Value::Null)
    }
}

/// The window half of the extension.
///
/// Every function here exists twice: once against a real window, and once as
/// a refusal for a build without the `window` feature. Two bodies rather than
/// one that silently does nothing, because a game loop whose `gameOpen`
/// quietly returned false would look like a window that closed immediately,
/// and the author would go looking for the bug in their own program.
pub mod live {
    use super::*;

    /// The message a build without window support gives.
    #[cfg(not(feature = "window"))]
    fn unsupported(who: &str) -> Signal {
        value_error(format!(
            "{who}() needs a build with window support; this one was built without it."
        ))
    }

    /// Open the window if it is not open yet, and say whether it is still there.
    ///
    /// `while (gameOpen())` is the shape a game's main loop takes, so this is
    /// also where events get pumped on a frame that draws nothing -- an
    /// unpumped window is one the desktop marks as not responding.
    #[cfg(feature = "window")]
    pub fn open(s: &mut Option<Screen>) -> Result<Value, Signal> {
        let screen = screen(s, "gameOpen")?;
        if screen.window.is_none() {
            let window =
                mrt_window::Window::new(screen.surface.width, screen.surface.height, &screen.title)
                    .map_err(|e| value_error(format!("gameOpen(): {e}")))?;
            screen.window = Some(window);
        }
        let window = screen.window.as_mut().expect("just opened");
        Ok(Value::Bool(window.is_open()))
    }

    #[cfg(not(feature = "window"))]
    pub fn open(s: &mut Option<Screen>) -> Result<Value, Signal> {
        screen(s, "gameOpen")?;
        Err(unsupported("gameOpen"))
    }

    /// Put the current frame on the screen.
    #[cfg(feature = "window")]
    pub fn present(s: &mut Option<Screen>) -> Result<Value, Signal> {
        let screen = screen(s, "gamePresent")?;
        let Some(window) = screen.window.as_mut() else {
            return Err(value_error(
                "gamePresent() needs a window; call gameOpen() first.",
            ));
        };
        window
            .present(&screen.surface)
            .map_err(|e| value_error(format!("gamePresent(): {e}")))?;
        // Ends a frame for the gamepad's "pressed since" the same moment it
        // ends one for the keyboard's -- only if a query has actually opened
        // the subsystem; a game that never asks about a controller should
        // not pay to keep one polled.
        #[cfg(feature = "gamepad")]
        if let Some(gamepads) = screen.gamepads.as_mut() {
            gamepads.end_frame();
        }
        Ok(Value::Null)
    }

    #[cfg(not(feature = "window"))]
    pub fn present(s: &mut Option<Screen>) -> Result<Value, Signal> {
        screen(s, "gamePresent")?;
        Err(unsupported("gamePresent"))
    }

    /// Seconds since the previous frame was presented.
    ///
    /// What movement is multiplied by, so a thing crossing the screen takes
    /// the same time on a fast machine and a slow one. A simulation that has
    /// to be *reproducible* should use a fixed step instead and ignore this:
    /// real elapsed time is never the same twice.
    #[cfg(feature = "window")]
    pub fn delta(s: &Option<Screen>) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameDelta"))?;
        Ok(Value::Number(
            screen.window.as_ref().map(|w| w.delta()).unwrap_or(0.0),
        ))
    }

    #[cfg(not(feature = "window"))]
    pub fn delta(s: &Option<Screen>) -> Result<Value, Signal> {
        s.as_ref().ok_or_else(|| no_screen("gameDelta"))?;
        Err(unsupported("gameDelta"))
    }

    /// Whether a key is held right now.
    #[cfg(feature = "window")]
    pub fn key_down(s: &Option<Screen>, key: &str) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameKeyDown"))?;
        Ok(Value::Bool(
            screen.window.as_ref().is_some_and(|w| w.key_down(key)),
        ))
    }

    #[cfg(not(feature = "window"))]
    pub fn key_down(s: &Option<Screen>, _key: &str) -> Result<Value, Signal> {
        s.as_ref().ok_or_else(|| no_screen("gameKeyDown"))?;
        Err(unsupported("gameKeyDown"))
    }

    /// Whether a key went down since the last frame, held or not.
    ///
    /// Separate from `key_down` because a key can be pressed and released
    /// inside one frame, and a menu that only asked what is held would drop
    /// it -- which feels like the game ignoring input and cannot be
    /// reproduced on purpose.
    #[cfg(feature = "window")]
    pub fn key_pressed(s: &Option<Screen>, key: &str) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameKeyPressed"))?;
        Ok(Value::Bool(
            screen.window.as_ref().is_some_and(|w| w.key_pressed(key)),
        ))
    }

    #[cfg(not(feature = "window"))]
    pub fn key_pressed(s: &Option<Screen>, _key: &str) -> Result<Value, Signal> {
        s.as_ref().ok_or_else(|| no_screen("gameKeyPressed"))?;
        Err(unsupported("gameKeyPressed"))
    }

    /// Where the pointer is, and whether it is held: `{x, y, down}`.
    #[cfg(feature = "window")]
    pub fn pointer(s: &Option<Screen>) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gamePointer"))?;
        let ((x, y), down) = match screen.window.as_ref() {
            Some(w) => (w.pointer(), w.pointer_down()),
            None => ((0.0, 0.0), false),
        };
        let mut map = crate::value::ObjMap::new();
        map.insert(crate::value::ObjKey::Str(Rc::from("x")), Value::Number(x));
        map.insert(crate::value::ObjKey::Str(Rc::from("y")), Value::Number(y));
        map.insert(
            crate::value::ObjKey::Str(Rc::from("down")),
            Value::Bool(down),
        );
        Ok(Value::object(map))
    }

    #[cfg(not(feature = "window"))]
    pub fn pointer(s: &Option<Screen>) -> Result<Value, Signal> {
        s.as_ref().ok_or_else(|| no_screen("gamePointer"))?;
        Err(unsupported("gamePointer"))
    }

    /// End the loop, as if the close button had been pressed.
    #[cfg(feature = "window")]
    pub fn close(s: &mut Option<Screen>) -> Result<Value, Signal> {
        let screen = screen(s, "gameClose")?;
        if let Some(window) = screen.window.as_mut() {
            window.close();
        }
        Ok(Value::Null)
    }

    #[cfg(not(feature = "window"))]
    pub fn close(s: &mut Option<Screen>) -> Result<Value, Signal> {
        screen(s, "gameClose")?;
        Err(unsupported("gameClose"))
    }

    /// The message a build without gamepad support gives.
    #[cfg(not(feature = "gamepad"))]
    fn gamepad_unsupported(who: &str) -> Signal {
        value_error(format!(
            "{who}() needs a build with gamepad support; this one was built without it."
        ))
    }

    /// Open the gamepad subsystem if it has not been already, remembering
    /// failure so it is tried once -- the same reasoning `Bank` remembers
    /// `speaker_failed` -- and bring it up to date with whatever has
    /// happened since the last query.
    #[cfg(feature = "gamepad")]
    fn ready_gamepads<'a>(
        screen: &'a mut Screen,
        who: &str,
    ) -> Result<&'a mut mrt_gamepad::Gamepads, Signal> {
        if screen.gamepads.is_none() {
            if let Some(why) = &screen.gamepads_failed {
                return Err(value_error(format!("{who}(): {why}")));
            }
            match mrt_gamepad::Gamepads::new() {
                Ok(gamepads) => screen.gamepads = Some(gamepads),
                Err(why) => {
                    screen.gamepads_failed = Some(why.clone());
                    return Err(value_error(format!("{who}(): {why}")));
                }
            }
        }
        let gamepads = screen.gamepads.as_mut().expect("just opened");
        gamepads.poll();
        Ok(gamepads)
    }

    /// How many gamepads are connected right now.
    #[cfg(feature = "gamepad")]
    pub fn gamepad_count(s: &mut Option<Screen>) -> Result<Value, Signal> {
        let screen = screen(s, "gameGamepadCount")?;
        let gamepads = ready_gamepads(screen, "gameGamepadCount")?;
        Ok(Value::Number(gamepads.count() as f64))
    }

    #[cfg(not(feature = "gamepad"))]
    pub fn gamepad_count(s: &mut Option<Screen>) -> Result<Value, Signal> {
        screen(s, "gameGamepadCount")?;
        Err(gamepad_unsupported("gameGamepadCount"))
    }

    /// Whether `name` is held on pad `index` right now. An unknown button
    /// name or a disconnected or never-connected pad both just read as not
    /// held, the same as `gameKeyDown` on a key name it does not recognise
    /// -- a typo is a bug to notice by the game never responding, not a
    /// crash to notice by a stack trace.
    #[cfg(feature = "gamepad")]
    pub fn gamepad_button_down(
        s: &mut Option<Screen>,
        index: usize,
        name: &str,
    ) -> Result<Value, Signal> {
        let screen = screen(s, "gameGamepadButtonDown")?;
        let gamepads = ready_gamepads(screen, "gameGamepadButtonDown")?;
        Ok(Value::Bool(gamepads.button_down(index, name)))
    }

    #[cfg(not(feature = "gamepad"))]
    pub fn gamepad_button_down(
        s: &mut Option<Screen>,
        _index: usize,
        _name: &str,
    ) -> Result<Value, Signal> {
        screen(s, "gameGamepadButtonDown")?;
        Err(gamepad_unsupported("gameGamepadButtonDown"))
    }

    /// Whether `name` went down on pad `index` since the last frame, held or
    /// not -- the gamepad's `gameKeyPressed`, for the same reason: a button
    /// tapped and released inside one frame would otherwise vanish.
    #[cfg(feature = "gamepad")]
    pub fn gamepad_button_pressed(
        s: &mut Option<Screen>,
        index: usize,
        name: &str,
    ) -> Result<Value, Signal> {
        let screen = screen(s, "gameGamepadButtonPressed")?;
        let gamepads = ready_gamepads(screen, "gameGamepadButtonPressed")?;
        Ok(Value::Bool(gamepads.button_pressed(index, name)))
    }

    #[cfg(not(feature = "gamepad"))]
    pub fn gamepad_button_pressed(
        s: &mut Option<Screen>,
        _index: usize,
        _name: &str,
    ) -> Result<Value, Signal> {
        screen(s, "gameGamepadButtonPressed")?;
        Err(gamepad_unsupported("gameGamepadButtonPressed"))
    }

    /// `name`'s current value on pad `index`, from -1 to 1.
    #[cfg(feature = "gamepad")]
    pub fn gamepad_axis(s: &mut Option<Screen>, index: usize, name: &str) -> Result<Value, Signal> {
        let screen = screen(s, "gameGamepadAxis")?;
        let gamepads = ready_gamepads(screen, "gameGamepadAxis")?;
        Ok(Value::Number(gamepads.axis(index, name)))
    }

    #[cfg(not(feature = "gamepad"))]
    pub fn gamepad_axis(
        s: &mut Option<Screen>,
        _index: usize,
        _name: &str,
    ) -> Result<Value, Signal> {
        screen(s, "gameGamepadAxis")?;
        Err(gamepad_unsupported("gameGamepadAxis"))
    }
}
