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
    /// Loaded bitmap fonts, addressed by a 1-based id the same way a sprite
    /// is. A separate pool rather than a kind of sprite: a font is never a
    /// draw target and never the thing a camera or `gameDraw` addresses, so
    /// giving it sprites' id space would only invite calling `gameDraw` on
    /// one by mistake.
    pub fonts: Vec<Option<mrt_game::bitmapfont::BitmapFont>>,
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
    /// How far in the camera is zoomed: `1.0` (its value until
    /// `gameCameraZoom` is called) shows the world at its own pixel size,
    /// `2.0` shows it twice as large -- half as much of it, each world
    /// pixel twice as many screen ones -- and `0.5` the opposite.
    ///
    /// Applied to the screen only, the same rule `camera_x`/`camera_y`
    /// already follow: a sprite pre-rendered while `gameTarget`ed away
    /// from the screen is drawn in its own local space regardless of
    /// where the camera is pointed or how far it is zoomed.
    pub camera_zoom: f64,
    /// Every live particle: sparks, smoke, an explosion's debris.
    ///
    /// One pool, not addressed by id the way a sprite is -- there is nothing
    /// to draw once and stamp many times here, only points to spawn, age and
    /// forget, so this is state the screen carries rather than a handle a
    /// program has to keep.
    pub particles: mrt_game::particles::Particles,
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
    /// The distance from the camera is scaled by `camera_zoom` before it is
    /// floored to a pixel -- at the default zoom of `1.0` that multiply
    /// changes nothing (an exact integer times `1.0`, floored, is the same
    /// integer back), so a program that never calls `gameCameraZoom` sees
    /// exactly the plain subtraction this did before zoom existed.
    pub fn from_world(&self, x: i32, y: i32) -> (i32, i32) {
        if self.target == 0 {
            let zoom = self.camera_zoom;
            (
                (((x - self.camera_x) as f64) * zoom).floor() as i32,
                (((y - self.camera_y) as f64) * zoom).floor() as i32,
            )
        } else {
            (x, y)
        }
    }

    /// The camera's own zoom, but only while drawing onto the screen --
    /// `1.0` (no scaling at all) everywhere else, the same target check
    /// `from_world` already makes.
    fn zoom_factor(&self) -> f64 {
        if self.target == 0 {
            self.camera_zoom
        } else {
            1.0
        }
    }

    /// A size in world pixels, scaled by the camera's zoom the same way
    /// `from_world` scales a position -- what `gameRect`, `gameCircle` and
    /// the rest of the shape built-ins use so a shape drawn twice as far
    /// from the camera's reach is also drawn at twice the size, not just
    /// twice the distance.
    pub fn scale_size(&self, size: i32) -> i32 {
        (size as f64 * self.zoom_factor()).floor() as i32
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

    /// Check an id names a live font, and say why not if it does not -- the
    /// same reasoning `sprite_index` uses, for the same reason.
    fn font_index(&self, id: usize, who: &str) -> Result<usize, Signal> {
        if id == 0 {
            return Err(value_error(format!(
                "{who}() needs a font from gameFontLoad(); 0 is not one."
            )));
        }
        match self.fonts.get(id - 1) {
            Some(Some(_)) => Ok(id - 1),
            Some(None) => Err(value_error(format!("{who}(): font {id} was freed."))),
            None => Err(value_error(format!("{who}(): there is no font {id}."))),
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
        fonts: Vec::new(),
        target: 0,
        camera_x: 0,
        camera_y: 0,
        camera_zoom: 1.0,
        particles: mrt_game::particles::Particles::new(),
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

    /// Set how far in the camera is zoomed. Must be a positive, finite
    /// number -- zero or negative has no picture to show for it (nothing,
    /// or everything mirrored and inverted), the same reasoning a
    /// non-positive width or height gets at `gameInit`.
    pub fn set_camera_zoom(s: &mut Option<Screen>, zoom: f64) -> Result<Value, Signal> {
        if !zoom.is_finite() || zoom <= 0.0 {
            return Err(value_error(format!(
                "gameCameraZoom() needs a positive, finite zoom, not {zoom}."
            )));
        }
        let screen = screen(s, "gameCameraZoom")?;
        screen.camera_zoom = zoom;
        Ok(Value::Null)
    }

    /// The camera's current zoom: `1.0` until `gameCameraZoom` changes it.
    pub fn camera_zoom_level(s: &Option<Screen>) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameCameraZoomLevel"))?;
        Ok(Value::Number(screen.camera_zoom))
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
        let (w, h) = (screen.scale_size(w), screen.scale_size(h));
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
        let (w, h, t) = (
            screen.scale_size(w),
            screen.scale_size(h),
            screen.scale_size(t),
        );
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
        let r = screen.scale_size(r);
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
        let scale = screen.scale_size(scale);
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
        let r = screen.scale_size(r);
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
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameSetFullscreen(true);"#),
            "Runtime Error: gameSetFullscreen() needs a build with window support; this one was built without it. [line 1]"
        );
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameIsFullscreen();"#),
            "Runtime Error: gameIsFullscreen() needs a build with window support; this one was built without it. [line 1]"
        );
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameSetCursorVisible(false);"#),
            "Runtime Error: gameSetCursorVisible() needs a build with window support; this one was built without it. [line 1]"
        );
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameSetCursorLocked(true);"#),
            "Runtime Error: gameSetCursorLocked() needs a build with window support; this one was built without it. [line 1]"
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
    fn fullscreen_and_cursor_calls_before_opening_a_window_are_quiet_no_ops() {
        // The same rule gameClose follows: there is no window yet to act on,
        // and that is not a mistake worth raising over -- a program can call
        // these in whatever order suits it without checking gameOpen() first.
        assert_eq!(
            main_of(
                r#"gameInit(4, 4, "t");
                   gameSetFullscreen(true);
                   print(gameIsFullscreen());
                   gameSetCursorVisible(false);
                   print(gameSetCursorLocked(true));"#
            ),
            "false\nfalse"
        );
    }

    #[test]
    #[cfg(feature = "window")]
    fn fullscreen_and_cursor_calls_are_type_checked() {
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameSetFullscreen(1);"#),
            "Runtime Error: gameSetFullscreen() needs a boolean, not number. [line 1]"
        );
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameSetCursorLocked("yes");"#),
            "Runtime Error: gameSetCursorLocked() needs a boolean, not string. [line 1]"
        );
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
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameGamepadRumble(0, 1, 0.1);"#),
            "Runtime Error: gameGamepadRumble() needs a build with gamepad support; this one was built without it. [line 1]"
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
                   print(gameGamepadAxis(0, "LeftX"));
                   print(gameGamepadRumble(0, 1, 0.1));"#
            ),
            "0\nfalse\nfalse\n0\nfalse"
        );
    }

    #[test]
    #[cfg(feature = "gamepad")]
    fn rumble_at_a_non_positive_strength_or_duration_never_starts_it() {
        // Answered rather than raised, the same rule every other malformed
        // gamepad query follows -- and true regardless of whether a real
        // pad happens to be connected, since a program handing this a
        // negative duration made a mistake no controller could fix.
        assert_eq!(
            main_of(
                r#"gameInit(4, 4, "t");
                   print(gameGamepadRumble(0, 0, 1));
                   print(gameGamepadRumble(0, -1, 1));
                   print(gameGamepadRumble(0, 1, 0));
                   print(gameGamepadRumble(0, 1, -1));"#
            ),
            "false\nfalse\nfalse\nfalse"
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
    fn gamedrawtilemap_draws_a_grid_of_tiles_from_one_sheet() {
        assert_eq!(
            main_of(
                r#"gameInit(6, 6, "t");
                   var red = gameColor(255, 0, 0);
                   var green = gameColor(0, 255, 0);
                   var blue = gameColor(0, 0, 255);
                   var yellow = gameColor(255, 255, 0);
                   // A 2x2 grid of 2x2 tiles: 0 top-left, 1 top-right,
                   // 2 bottom-left, 3 bottom-right, row-major.
                   var sheet = gameSurface(4, 4);
                   gameTarget(sheet);
                   gameRect(0, 0, 2, 2, red);
                   gameRect(2, 0, 2, 2, green);
                   gameRect(0, 2, 2, 2, blue);
                   gameRect(2, 2, 2, 2, yellow);
                   gameTarget(0);
                   // A null cell leaves the background showing through.
                   gameDrawTilemap(sheet, 0, 0, 2, 2, [[3, 1], [null, 0]]);
                   print(gameColorAt(0, 0) == yellow, gameColorAt(2, 0) == green);
                   print(gameColorAt(0, 2) == gameColor(0, 0, 0), gameColorAt(2, 2) == red);"#
            ),
            "true true\ntrue true"
        );
    }

    #[test]
    fn gamedrawtilemap_follows_the_camera_like_gamedraw_does() {
        assert_eq!(
            main_of(
                r#"gameInit(10, 10, "t");
                   var red = gameColor(255, 0, 0);
                   var sheet = gameSurface(2, 2);
                   gameTarget(sheet);
                   gameRect(0, 0, 2, 2, red);
                   gameTarget(0);
                   gameCamera(1, 1);
                   gameDrawTilemap(sheet, 5, 5, 2, 2, [[0]]);
                   print(gameColorAt(5, 5) == red);"#
            ),
            "true"
        );
    }

    #[test]
    fn gamedrawtilemap_requires_the_sheet_to_be_an_exact_grid_of_tiles() {
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var sheet = gameSurface(5, 4);
                   gameDrawTilemap(sheet, 0, 0, 2, 2, [[0]]);"#
            ),
            "Runtime Error: gameDrawTilemap(): surface 1 (5x4) is not an exact grid of 2x2 tiles. [line 3]"
        );
    }

    #[test]
    fn gamedrawtilemap_rejects_an_out_of_range_tile_index() {
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var sheet = gameSurface(4, 4);
                   gameDrawTilemap(sheet, 0, 0, 2, 2, [[4]]);"#
            ),
            "Runtime Error: gameDrawTilemap(): tile 4 at row 0, column 0 is out of range -- surface 1 only has 4 tiles. [line 3]"
        );
    }

    #[test]
    fn gamedrawtilemap_rejects_a_surface_drawn_onto_itself() {
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var sheet = gameSurface(4, 4);
                   gameTarget(sheet);
                   gameDrawTilemap(sheet, 0, 0, 2, 2, [[0]]);"#
            ),
            "Runtime Error: gameDrawTilemap(): surface 1 cannot be drawn onto itself. [line 4]"
        );
    }

    #[test]
    fn gamedrawtilemap_type_checks_its_tiles_argument() {
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var sheet = gameSurface(4, 4);
                   gameDrawTilemap(sheet, 0, 0, 2, 2, 5);"#
            ),
            "Runtime Error: gameDrawTilemap() needs tiles to be an array of arrays, not number. [line 3]"
        );
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var sheet = gameSurface(4, 4);
                   gameDrawTilemap(sheet, 0, 0, 2, 2, [5]);"#
            ),
            "Runtime Error: gameDrawTilemap(): row 0 of tiles must be an array, not number. [line 3]"
        );
        assert_eq!(
            main_of(
                r#"gameInit(8, 8, "t");
                   var sheet = gameSurface(4, 4);
                   gameDrawTilemap(sheet, 0, 0, 2, 2, [["a"]]);"#
            ),
            "Runtime Error: gameDrawTilemap(): tile at row 0, column 0 must be null or a number, not string. [line 3]"
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
    fn zoom_defaults_to_one_and_reports_back_what_was_set() {
        assert_eq!(
            drawing(
                r#"print(gameCameraZoomLevel());
                   gameCameraZoom(2.5);
                   print(gameCameraZoomLevel());"#
            ),
            "1\n2.5"
        );
    }

    #[test]
    fn a_non_positive_zoom_is_refused() {
        // Non-finite is guarded against too (see set_camera_zoom), but MRT
        // itself has no way to construct one -- every arithmetic op that
        // would produce infinity or NaN is already its own catchable
        // error (division by zero, pow() overflowing), so there is no
        // source-level program that could reach that branch to test here.
        assert_eq!(
            drawing("gameCameraZoom(0);"),
            "Runtime Error: gameCameraZoom() needs a positive, finite zoom, not 0. [line 1]"
        );
        assert_eq!(
            drawing("gameCameraZoom(-1);"),
            "Runtime Error: gameCameraZoom() needs a positive, finite zoom, not -1. [line 1]"
        );
    }

    #[test]
    fn zoom_scales_a_shape_position_and_size_together() {
        // A 2x2 rect at zoomed-in 3x should occupy a 6x6 block of screen
        // pixels starting where its own corner lands -- world (1, 1) is
        // outside the rect's own 2x2 footprint but its zoomed screen
        // position (3, 3) is still inside that 6x6 block, which is only
        // true if the *size*, not just the position, scaled with it.
        assert_eq!(
            drawing(
                r#"var red = gameColor(255, 0, 0);
                   gameCameraZoom(3);
                   gameRect(0, 0, 2, 2, red);
                   print(gameColorAt(0, 0) == red, gameColorAt(1, 1) == red);
                   print(gameColorAt(2, 0) == red, gameColorAt(2, 2) == red);"#
            ),
            "true true\nfalse false"
        );
    }

    #[test]
    fn zoom_composes_with_the_camera() {
        // screen = (world - camera) * zoom: moved and scaled together, not
        // one undoing the other.
        assert_eq!(
            drawing(
                r#"var red = gameColor(255, 0, 0);
                   gameCamera(5, 5);
                   gameCameraZoom(2);
                   gamePixel(7, 7, red);
                   print(gameColorAt(7, 7) == red);"#
            ),
            "true"
        );
    }

    #[test]
    fn zoom_never_reaches_a_sprite_being_drawn_on() {
        // The same rule the camera's position already follows: pre-rendering
        // into a sprite works in that sprite's own local, unscaled space.
        assert_eq!(
            drawing(
                r#"var red = gameColor(255, 0, 0);
                   gameCameraZoom(5);
                   var s = gameSurface(4, 4);
                   gameTarget(s);
                   gameRect(0, 0, 2, 2, red);
                   print(gameColorAt(1, 1) == red, gameColorAt(2, 2) == red);"#
            ),
            "true false"
        );
    }

    #[test]
    fn zoom_scales_a_drawn_sprite_too() {
        assert_eq!(
            drawing(
                r#"var red = gameColor(255, 0, 0);
                   var s = gameSurface(2, 2);
                   gameTarget(s);
                   gameClear(red);
                   gameTarget(0);
                   gameCameraZoom(3);
                   gameDraw(s, 0, 0);
                   print(gameColorAt(1, 1) == red, gameColorAt(2, 2) == red);"#
            ),
            "true false"
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
    fn circle_overlap_needs_no_screen_either() {
        assert_eq!(
            main_of(
                r#"var a = {x: 0, y: 0, radius: 5};
                   print(gameCircleOverlap(a, {x: 9, y: 0, radius: 5}));
                   print(gameCircleOverlap(a, {x: 10, y: 0, radius: 5}));
                   print(gameCircleOverlap(a, {x: 100, y: 0, radius: 5}));"#
            ),
            "true\nfalse\nfalse"
        );
    }

    #[test]
    fn resolving_circles_pushes_them_apart_on_the_line_between_them() {
        assert_eq!(
            main_of(
                r#"var a = {x: 0, y: 0, radius: 5};
                   var b = {x: 6, y: 0, radius: 5};
                   var push = gameCircleResolve(a, b);
                   print(push.x, push.y);
                   print(gameCircleResolve(a, {x: 100, y: 100, radius: 5}));"#
            ),
            "-4 0\nnull"
        );
    }

    #[test]
    fn a_circle_and_a_box_can_overlap_and_resolve() {
        assert_eq!(
            main_of(
                r#"var wall = {x: 0, y: 0, width: 10, height: 10};
                   var ball = {x: 15, y: 5, radius: 7};
                   print(gameCircleBoxOverlap(ball, wall));
                   var push = gameCircleBoxResolve(ball, wall);
                   print(push.x, push.y);
                   print(gameCircleBoxOverlap({x: 100, y: 100, radius: 1}, wall));
                   print(gameCircleBoxResolve({x: 100, y: 100, radius: 1}, wall));"#
            ),
            "true\n2 0\nfalse\nnull"
        );
    }

    #[test]
    fn a_circle_that_is_not_one_says_which_field_is_missing() {
        assert_eq!(
            main_of(r#"gameCircleOverlap({x: 0, y: 0}, {x: 0, y: 0, radius: 1});"#),
            "Runtime Error: gameCircleOverlap(): the first circle has no 'radius'. [line 1]"
        );
        assert_eq!(
            main_of(r#"gameCircleOverlap(5, {x: 0, y: 0, radius: 1});"#),
            "Runtime Error: gameCircleOverlap() needs the first circle to be a circle object with x, y and radius, not number. [line 1]"
        );
    }

    #[test]
    fn a_circle_with_a_negative_radius_is_refused_rather_than_answered() {
        assert_eq!(
            main_of(r#"gameCircleOverlap({x: 0, y: 0, radius: -1}, {x: 0, y: 0, radius: 1});"#),
            "Runtime Error: gameCircleOverlap(): the first circle has a negative radius, -1. [line 1]"
        );
    }

    #[test]
    fn pathfind_finds_the_shortest_route_as_an_array_of_waypoints() {
        assert_eq!(
            main_of(
                r#"var grid = [
                       [true, true, true],
                       [true, true, true]
                   ];
                   var path = gamePathfind(grid, 0, 0, 2, 0);
                   print(len(path));
                   print(path[0].x, path[0].y);
                   print(path[2].x, path[2].y);"#
            ),
            "3\n0 0\n2 0"
        );
    }

    #[test]
    fn pathfind_needs_no_screen_at_all() {
        // Finding a route is arithmetic on an array of booleans, the same
        // reasoning that lets gameOverlap and friends work headlessly.
        assert_eq!(
            main_of(r#"print(len(gamePathfind([[true, true]], 0, 0, 1, 0)));"#),
            "2"
        );
    }

    #[test]
    fn pathfind_answers_null_when_there_is_no_route() {
        assert_eq!(
            main_of(
                r#"var grid = [
                       [true, false, true],
                       [true, false, true],
                       [true, false, true]
                   ];
                   print(gamePathfind(grid, 0, 0, 2, 0));"#
            ),
            "null"
        );
    }

    #[test]
    fn pathfind_answers_null_for_a_start_or_goal_on_a_wall() {
        // Blocked but inside the grid is an ordinary question with an
        // ordinary answer -- unlike being outside the grid entirely, which
        // is refused below.
        assert_eq!(
            main_of(r#"print(gamePathfind([[true, false], [true, true]], 1, 0, 0, 0));"#),
            "null"
        );
    }

    #[test]
    fn pathfind_rejects_a_start_or_goal_outside_the_grid() {
        assert_eq!(
            main_of(r#"gamePathfind([[true, true], [true, true]], 5, 5, 0, 0);"#),
            "Runtime Error: gamePathfind(): the start (5, 5) is outside the 2x2 grid. [line 1]"
        );
        assert_eq!(
            main_of(r#"gamePathfind([[true, true], [true, true]], 0, 0, 5, 5);"#),
            "Runtime Error: gamePathfind(): the goal (5, 5) is outside the 2x2 grid. [line 1]"
        );
    }

    #[test]
    fn pathfind_type_checks_its_grid_argument() {
        assert_eq!(
            main_of(r#"gamePathfind(5, 0, 0, 1, 1);"#),
            "Runtime Error: gamePathfind() needs the grid to be an array of rows, not number. [line 1]"
        );
        assert_eq!(
            main_of(r#"gamePathfind([1, 2], 0, 0, 1, 0);"#),
            "Runtime Error: gamePathfind(): row 0 of the grid must be an array, not number. [line 1]"
        );
        assert_eq!(
            main_of(r#"gamePathfind([[true, 1]], 0, 0, 1, 0);"#),
            "Runtime Error: gamePathfind(): the cell at row 0, column 1 must be true or false, not number. [line 1]"
        );
    }

    #[test]
    fn pathfind_rejects_a_ragged_grid() {
        assert_eq!(
            main_of(r#"gamePathfind([[true, true], [true]], 0, 0, 1, 0);"#),
            "Runtime Error: gamePathfind(): row 1 has 1 cells, but row 0 has 2. [line 1]"
        );
    }

    #[test]
    fn pathfind_rejects_an_empty_grid() {
        assert_eq!(
            main_of(r#"gamePathfind([], 0, 0, 0, 0);"#),
            "Runtime Error: gamePathfind(): the grid has no rows. [line 1]"
        );
        assert_eq!(
            main_of(r#"gamePathfind([[]], 0, 0, 0, 0);"#),
            "Runtime Error: gamePathfind(): the grid has no columns. [line 1]"
        );
    }

    #[test]
    fn pathfind_rejects_a_negative_or_fractional_coordinate() {
        assert_eq!(
            main_of(r#"gamePathfind([[true, true]], -1, 0, 0, 0);"#),
            "Runtime Error: gamePathfind() needs the start x to be a non-negative whole number, not -1. [line 1]"
        );
        assert_eq!(
            main_of(r#"gamePathfind([[true, true]], 0.5, 0, 0, 0);"#),
            "Runtime Error: gamePathfind() needs the start x to be a non-negative whole number, not 0.5. [line 1]"
        );
    }

    #[test]
    fn particles_spawn_move_age_and_draw() {
        assert_eq!(
            drawing(
                r#"var white = gameColor(255, 255, 255);
                   print(gameParticleCount());
                   gameParticleSpawn(2, 2, 3, 0, 1, white);
                   print(gameParticleCount());
                   gameParticleDraw();
                   print(gameColorAt(2, 2) == white);
                   gameParticleUpdate(1);
                   print(gameParticleCount());"#
            ),
            "0\n1\ntrue\n0"
        );
    }

    #[test]
    fn a_particle_with_non_positive_life_is_never_spawned() {
        assert_eq!(
            drawing(
                r#"gameParticleSpawn(0, 0, 0, 0, 0, gameColor(255, 255, 255));
                   print(gameParticleCount());"#
            ),
            "0"
        );
    }

    #[test]
    fn particle_drawing_follows_the_camera_like_everything_else() {
        assert_eq!(
            drawing(
                r#"var white = gameColor(255, 255, 255);
                   gameParticleSpawn(5, 5, 0, 0, 10, white);
                   gameCamera(2, 1);
                   gameParticleDraw();
                   print(gameColorAt(5, 5) == white);"#
            ),
            "true",
            "gameColorAt takes a world position too, so the same particle should still \
             read back as white at its own world position once the camera has moved it"
        );
    }

    #[test]
    fn particle_calls_need_no_window_at_all() {
        // Spawning, ageing and counting are arithmetic on a Vec, the same
        // reason collision needs no screen either -- a program doing this
        // headlessly should not have to open a framebuffer first, and
        // drawing them needs only the target surface gameInit already made.
        assert_eq!(
            main_of(
                r#"gameInit(4, 4, "t");
                   gameParticleSpawn(0, 0, 0, 0, 1, gameColor(255, 0, 0));
                   gameParticleUpdate(0.5);
                   print(gameParticleCount());"#
            ),
            "1"
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

    /// A 4x2 atlas, two 2x2 cells: the left one red, the right one blue.
    /// Built from MRT itself with `gameSurface`/`gameRect`/`gameSave`, the
    /// same way the sprite-loading tests build their own fixture rather
    /// than shipping a binary PNG in the repository.
    fn two_cell_font_atlas(path: &str) -> String {
        format!(
            r#"gameInit(8, 8, "t");
               var atlas = gameSurface(4, 2);
               gameTarget(atlas);
               gameRect(0, 0, 2, 2, gameColor(255, 0, 0));
               gameRect(2, 0, 2, 2, gameColor(0, 0, 255));
               gameSave("{path}");
               gameTarget(0);"#
        )
    }

    #[test]
    fn a_loaded_font_draws_the_right_cell_for_each_character() {
        let path = std::env::temp_dir().join("mrt_game_font_draw.png");
        let escaped = path.to_string_lossy().replace('\\', "\\\\");
        assert_eq!(
            main_of(&format!(
                r#"{}
                   var font = gameFontLoad("{escaped}", 2, 2, "ab");
                   gameDrawText(font, 0, 0, "ba", 1);
                   print(gameColorAt(0, 0) == gameColor(0, 0, 255), "b drawn first");
                   print(gameColorAt(2, 0) == gameColor(255, 0, 0), "then a, advanced by one cell");"#,
                two_cell_font_atlas(&escaped)
            )),
            "true b drawn first\ntrue then a, advanced by one cell"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_loaded_font_follows_the_camera_like_gametext_does() {
        let path = std::env::temp_dir().join("mrt_game_font_camera.png");
        let escaped = path.to_string_lossy().replace('\\', "\\\\");
        assert_eq!(
            main_of(&format!(
                r#"{}
                   gameCamera(1, 0);
                   var font = gameFontLoad("{escaped}", 2, 2, "a");
                   gameDrawText(font, 1, 0, "a", 1);
                   print(gameColorAt(1, 0) == gameColor(255, 0, 0));"#,
                two_cell_font_atlas(&escaped)
            )),
            "true"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn drawing_a_character_a_font_does_not_have_names_it() {
        let path = std::env::temp_dir().join("mrt_game_font_missing_char.png");
        let escaped = path.to_string_lossy().replace('\\', "\\\\");
        let output = main_of(&format!(
            r#"{}
               var font = gameFontLoad("{escaped}", 2, 2, "ab");
               gameDrawText(font, 0, 0, "aqb", 1);"#,
            two_cell_font_atlas(&escaped)
        ));
        let _ = std::fs::remove_file(&path);
        assert!(
            output.contains("gameDrawText():") && output.contains("'q'"),
            "got {output:?}"
        );
    }

    #[test]
    fn a_font_atlas_that_is_not_an_exact_grid_says_so() {
        let path = std::env::temp_dir().join("mrt_game_font_bad_grid.png");
        let escaped = path.to_string_lossy().replace('\\', "\\\\");
        let output = main_of(&format!(
            r#"{}
               gameFontLoad("{escaped}", 3, 2, "a");"#,
            two_cell_font_atlas(&escaped)
        ));
        let _ = std::fs::remove_file(&path);
        assert!(
            output.contains("gameFontLoad():") && output.contains("not an exact grid"),
            "got {output:?}"
        );
    }

    #[test]
    fn a_font_measures_text_without_drawing_it() {
        let path = std::env::temp_dir().join("mrt_game_font_measure.png");
        let escaped = path.to_string_lossy().replace('\\', "\\\\");
        assert_eq!(
            main_of(&format!(
                r#"{}
                   var font = gameFontLoad("{escaped}", 2, 2, "ab");
                   print(gameFontTextWidth(font, "ab", 1), gameFontTextHeight(font, "ab", 1));
                   print(gameFontTextWidth(font, "a\nb", 2), gameFontTextHeight(font, "a\nb", 2));"#,
                two_cell_font_atlas(&escaped)
            )),
            "4 2\n4 8"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_freed_font_is_told_apart_from_one_that_never_existed() {
        let path = std::env::temp_dir().join("mrt_game_font_freed.png");
        let escaped = path.to_string_lossy().replace('\\', "\\\\");
        let output = main_of(&format!(
            r#"{}
               var font = gameFontLoad("{escaped}", 2, 2, "a");
               gameFontFree(font);
               gameDrawText(font, 0, 0, "a", 1);"#,
            two_cell_font_atlas(&escaped)
        ));
        assert!(
            output.contains("gameDrawText(): font 1 was freed."),
            "got {output:?}"
        );
        assert_eq!(
            main_of(r#"gameInit(4, 4, "t"); gameFontTextWidth(9, "a", 1);"#),
            "Runtime Error: gameFontTextWidth(): there is no font 9. [line 1]"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn font_calls_need_a_screen_first() {
        assert_eq!(
            main_of(r#"gameFontLoad("x.png", 2, 2, "a");"#),
            "Runtime Error: gameFontLoad() needs a screen; call gameInit(width, height, title) first. [line 1]"
        );
        assert_eq!(
            main_of("gameDrawText(1, 0, 0, \"a\", 1);"),
            "Runtime Error: gameDrawText() needs a screen; call gameInit(width, height, title) first. [line 1]"
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
    use mrt_game::collide::{Aabb, Circle};

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

    /// Read `{x, y, radius}` out of an MRT object.
    fn circle_of(value: &Value, who: &str, which: &str) -> Result<Circle, Signal> {
        let Value::Object(map) = value else {
            return Err(type_error(format!(
                "{who}() needs {which} to be a circle object with x, y and radius, not {}.",
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
        let radius = field("radius")?;
        // The same complaint a negative box size gets: a shape whose own
        // definition is nonsense, not something every test below would have
        // to guess an answer for.
        if radius < 0.0 {
            return Err(value_error(format!(
                "{who}(): {which} has a negative radius, {radius}."
            )));
        }
        Ok(Circle::new(field("x")?, field("y")?, radius))
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

    /// Whether two circles share any area. A circle crosses as `{x, y,
    /// radius}`, the same rule a box's `{x, y, width, height}` follows.
    pub fn circle_overlap(a: &Value, b: &Value) -> Result<Value, Signal> {
        Ok(Value::Bool(mrt_game::collide::circle_overlap(
            &circle_of(a, "gameCircleOverlap", "the first circle")?,
            &circle_of(b, "gameCircleOverlap", "the second circle")?,
        )))
    }

    /// How far to move the first circle to separate it, or `null` if apart.
    pub fn circle_resolve(a: &Value, b: &Value) -> Result<Value, Signal> {
        let a = circle_of(a, "gameCircleResolve", "the first circle")?;
        let b = circle_of(b, "gameCircleResolve", "the second circle")?;
        Ok(match mrt_game::collide::circle_resolve(&a, &b) {
            Some((x, y)) => {
                let mut map = ObjMap::new();
                map.insert(key("x"), Value::Number(x));
                map.insert(key("y"), Value::Number(y));
                Value::object(map)
            }
            None => Value::Null,
        })
    }

    /// Whether a circle and a box share any area.
    pub fn circle_box_overlap(c: &Value, b: &Value) -> Result<Value, Signal> {
        Ok(Value::Bool(mrt_game::collide::circle_box_overlap(
            &circle_of(c, "gameCircleBoxOverlap", "the circle")?,
            &box_of(b, "gameCircleBoxOverlap", "the box")?,
        )))
    }

    /// How far to move the circle to separate it from the box, or `null`.
    pub fn circle_box_resolve(c: &Value, b: &Value) -> Result<Value, Signal> {
        let c = circle_of(c, "gameCircleBoxResolve", "the circle")?;
        let b = box_of(b, "gameCircleBoxResolve", "the box")?;
        Ok(match mrt_game::collide::circle_box_resolve(&c, &b) {
            Some((x, y)) => {
                let mut map = ObjMap::new();
                map.insert(key("x"), Value::Number(x));
                map.insert(key("y"), Value::Number(y));
                Value::object(map)
            }
            None => Value::Null,
        })
    }
}

/// Grid pathfinding, needing no screen: finding a route through a grid is
/// arithmetic on an array of booleans, the same reasoning that lets
/// `collide` above work headlessly.
pub mod pathfind {
    use super::*;
    use crate::value::{ObjKey, ObjMap};
    use mrt_game::pathfind::{find_path, Grid};

    fn key(name: &str) -> ObjKey {
        ObjKey::Str(Rc::from(name))
    }

    /// Read a grid of booleans out of an array of rows -- the same shape
    /// `gameDrawTilemap`'s `tiles` array takes, so a level that already
    /// has one for drawing does not need a second one for finding a way
    /// across it.
    fn grid_of(value: &Value, who: &str) -> Result<Grid, Signal> {
        let Value::Array(rows) = value else {
            return Err(type_error(format!(
                "{who}() needs the grid to be an array of rows, not {}.",
                crate::value::type_name(value)
            )));
        };
        let rows = rows.borrow();
        if rows.is_empty() {
            return Err(value_error(format!("{who}(): the grid has no rows.")));
        }
        let height = rows.len();
        let mut width = None;
        let mut walkable = Vec::new();
        for (row_index, row) in rows.iter().enumerate() {
            let Value::Array(cells) = row else {
                return Err(type_error(format!(
                    "{who}(): row {row_index} of the grid must be an array, not {}.",
                    crate::value::type_name(row)
                )));
            };
            let cells = cells.borrow();
            match width {
                None => width = Some(cells.len()),
                Some(w) if w != cells.len() => {
                    return Err(value_error(format!(
                        "{who}(): row {row_index} has {} cells, but row 0 has {w}.",
                        cells.len()
                    )));
                }
                _ => {}
            }
            for (col_index, cell) in cells.iter().enumerate() {
                match cell {
                    Value::Bool(b) => walkable.push(*b),
                    other => {
                        return Err(type_error(format!(
                            "{who}(): the cell at row {row_index}, column {col_index} must be true or false, not {}.",
                            crate::value::type_name(other)
                        )));
                    }
                }
            }
        }
        let width = width
            .filter(|w| *w > 0)
            .ok_or_else(|| value_error(format!("{who}(): the grid has no columns.")))?;
        Grid::new(width, height, walkable).map_err(|e| value_error(format!("{who}(): {e}.")))
    }

    /// A grid coordinate: a non-negative whole number. Pathfinding works on
    /// cells, not pixels, so this has no fractional part to floor the way
    /// `coord` does for a drawing position.
    fn cell(value: &Value, who: &str, what: &str) -> Result<i32, Signal> {
        match value {
            Value::Number(n) if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 => Ok(*n as i32),
            Value::Number(n) => Err(value_error(format!(
                "{who}() needs {what} to be a non-negative whole number, not {n}."
            ))),
            other => Err(type_error(format!(
                "{who}() needs {what} to be a number, not {}.",
                crate::value::type_name(other)
            ))),
        }
    }

    /// The shortest walkable route from one cell to another, as an array of
    /// `{x, y}` waypoints including both ends, or `null` if none exists.
    ///
    /// A start or goal outside the grid's own bounds is raised rather than
    /// answered with `null`, the same reasoning `gameDrawTilemap` raises on
    /// a tile index outside its sheet: a level authored by hand is exactly
    /// where a typo'd coordinate is likely, and `null` reads as "no route",
    /// not "you asked about a cell that does not exist". A start or goal
    /// that is *inside* the grid but blocked is not this case -- asking
    /// for a route to or from a wall is an ordinary question with an
    /// ordinary answer, `null`.
    #[allow(clippy::too_many_arguments)]
    pub fn find(
        grid: &Value,
        start_x: &Value,
        start_y: &Value,
        goal_x: &Value,
        goal_y: &Value,
    ) -> Result<Value, Signal> {
        let who = "gamePathfind";
        let grid = grid_of(grid, who)?;
        let start = (
            cell(start_x, who, "the start x")?,
            cell(start_y, who, "the start y")?,
        );
        let goal = (
            cell(goal_x, who, "the goal x")?,
            cell(goal_y, who, "the goal y")?,
        );
        for (label, (x, y)) in [("start", start), ("goal", goal)] {
            if !grid.contains(x, y) {
                return Err(value_error(format!(
                    "{who}(): the {label} ({x}, {y}) is outside the {}x{} grid.",
                    grid.width(),
                    grid.height()
                )));
            }
        }
        Ok(match find_path(&grid, start, goal) {
            Some(path) => Value::array(
                path.into_iter()
                    .map(|(x, y)| {
                        let mut map = ObjMap::new();
                        map.insert(key("x"), Value::Number(x as f64));
                        map.insert(key("y"), Value::Number(y as f64));
                        Value::object(map)
                    })
                    .collect(),
            ),
            None => Value::Null,
        })
    }
}

/// Particles: sparks, smoke, an explosion's debris.
///
/// One pool per screen rather than one id per burst, the same shape the
/// camera and the current draw target already take -- there is nothing here
/// to draw once and stamp many times the way a sprite is, only points to
/// spawn, age, and forget.
pub mod particles {
    use super::*;

    /// Add one particle at `(x, y)` in world pixels, moving at `(vx, vy)`
    /// pixels a second, living for `life` seconds.
    ///
    /// A non-positive or non-finite `life` spawns nothing rather than
    /// raising -- the same rule a negative box size gets elsewhere in this
    /// extension: a particle already dead the instant it exists is a
    /// caller's arithmetic coming out degenerate, not a mistake worth
    /// stopping the program over.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        s: &mut Option<Screen>,
        x: f64,
        y: f64,
        vx: f64,
        vy: f64,
        life: f64,
        color: Color,
    ) -> Result<Value, Signal> {
        let screen = screen(s, "gameParticleSpawn")?;
        screen.particles.spawn(x, y, vx, vy, life, color);
        Ok(Value::Null)
    }

    /// Age every particle by `dt` seconds and drop whichever ones have run
    /// out of life.
    pub fn update(s: &mut Option<Screen>, dt: f64) -> Result<Value, Signal> {
        screen(s, "gameParticleUpdate")?.particles.update(dt);
        Ok(Value::Null)
    }

    /// Draw every live particle onto the current target.
    ///
    /// Particles are spawned in world pixels like everything else a program
    /// draws, so a camera move scrolls them the same as it scrolls the rest
    /// of the world -- taken out and put back around the draw call for the
    /// same reason `sprites::draw_transformed` does: `target_mut` and
    /// `particles` are two fields of one `Screen`, and this is the cheap way
    /// to convince the borrow checker they are different ones.
    pub fn draw(s: &mut Option<Screen>) -> Result<Value, Signal> {
        let screen = screen(s, "gameParticleDraw")?;
        let (offset_x, offset_y) = if screen.target == 0 {
            (-screen.camera_x, -screen.camera_y)
        } else {
            (0, 0)
        };
        let pool = std::mem::take(&mut screen.particles);
        pool.draw(screen.target_mut(), offset_x, offset_y);
        screen.particles = pool;
        Ok(Value::Null)
    }

    /// How many particles are alive right now.
    pub fn count(s: &Option<Screen>) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameParticleCount"))?;
        Ok(Value::Number(screen.particles.count() as f64))
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
    ///
    /// `pub(super)` rather than private: `fonts` below needs the same check
    /// for a font's character cells, and two copies of "a whole number of
    /// pixels, at least 1" would eventually disagree about the wording.
    pub(super) fn dimension(value: &Value, who: &str, what: &str) -> Result<u32, Signal> {
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
        let zoom = screen.zoom_factor();
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
            transform.scale_x * zoom,
            transform.scale_y * zoom,
            transform.anchor_x,
            transform.anchor_y,
            transform.region,
        );
        screen.sprites[index] = Some(sprite);
        Ok(Value::Null)
    }

    /// Draw a whole grid of tiles from one sprite sheet in a single call.
    ///
    /// A level is data -- a 2D array of tile indices -- rather than the
    /// hundreds or thousands of individual `gameDraw` calls it would take to
    /// stamp each one from MRT itself, one call per tile, every single
    /// frame. The sheet's own grid is inferred from its size and the tile
    /// size alone: tile `n` is at column `n % columns`, row `n / columns`,
    /// where `columns` is the sheet's width divided by `tileWidth` -- the
    /// same row-major order a program would reach for on its own.
    ///
    /// `tiles` is an array of rows, each an array of tile indices; `null`
    /// in place of an index leaves that cell empty rather than drawing
    /// anything -- the shape a sparse level (a mostly-empty overlay of
    /// decorations, say) actually needs.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_tilemap(
        s: &mut Option<Screen>,
        id: usize,
        x: i32,
        y: i32,
        tile_width: &Value,
        tile_height: &Value,
        tiles: &Value,
    ) -> Result<Value, Signal> {
        let screen = screen(s, "gameDrawTilemap")?;
        let index = screen.sprite_index(id, "gameDrawTilemap")?;
        if screen.target == id {
            return Err(value_error(format!(
                "gameDrawTilemap(): surface {id} cannot be drawn onto itself."
            )));
        }
        let tile_width = dimension(tile_width, "gameDrawTilemap", "the tile width")?;
        let tile_height = dimension(tile_height, "gameDrawTilemap", "the tile height")?;
        let sprite = screen.sprites[index].as_ref().expect("checked live");
        if sprite.width % tile_width != 0 || sprite.height % tile_height != 0 {
            return Err(value_error(format!(
                "gameDrawTilemap(): surface {id} ({}x{}) is not an exact grid of {tile_width}x{tile_height} tiles.",
                sprite.width, sprite.height
            )));
        }
        let sheet_columns = sprite.width / tile_width;
        let sheet_rows = sprite.height / tile_height;
        let tile_count = (sheet_columns * sheet_rows) as usize;

        // Read the whole grid into a plain Rust shape and check every cell
        // before drawing any of them -- a program authoring a level by hand
        // is exactly the case a typo'd tile index is likely, and a level
        // half-drawn before the error surfaced would be a worse thing to
        // debug than one that never started.
        let Value::Array(rows) = tiles else {
            return Err(type_error(format!(
                "gameDrawTilemap() needs tiles to be an array of arrays, not {}.",
                crate::value::type_name(tiles)
            )));
        };
        let rows = rows.borrow();
        let mut grid: Vec<Vec<Option<usize>>> = Vec::with_capacity(rows.len());
        for (row_index, row) in rows.iter().enumerate() {
            let Value::Array(row) = row else {
                return Err(type_error(format!(
                    "gameDrawTilemap(): row {row_index} of tiles must be an array, not {}.",
                    crate::value::type_name(row)
                )));
            };
            let row = row.borrow();
            let mut cells = Vec::with_capacity(row.len());
            for (col_index, cell) in row.iter().enumerate() {
                cells.push(match cell {
                    Value::Null => None,
                    Value::Number(n) if n.fract() == 0.0 && *n >= 0.0 => {
                        let tile = *n as usize;
                        if tile >= tile_count {
                            return Err(value_error(format!(
                                "gameDrawTilemap(): tile {tile} at row {row_index}, column {col_index} is out of range -- surface {id} only has {tile_count} tiles."
                            )));
                        }
                        Some(tile)
                    }
                    Value::Number(n) => {
                        return Err(value_error(format!(
                            "gameDrawTilemap(): tile at row {row_index}, column {col_index} must be null or a non-negative whole number, not {n}."
                        )));
                    }
                    other => {
                        return Err(type_error(format!(
                            "gameDrawTilemap(): tile at row {row_index}, column {col_index} must be null or a number, not {}.",
                            crate::value::type_name(other)
                        )));
                    }
                });
            }
            grid.push(cells);
        }

        // Taken out once for the whole grid rather than per tile, for the
        // same reason `draw_transformed` takes it out at all: the source
        // and the target are two entries of one Vec, and this is the cheap
        // way to convince the compiler they are different ones.
        let sprite = screen.sprites[index].take().expect("checked live");
        for (row_index, row) in grid.iter().enumerate() {
            for (col_index, cell) in row.iter().enumerate() {
                let Some(tile) = cell else { continue };
                let src_x = (tile % sheet_columns as usize) as i32 * tile_width as i32;
                let src_y = (tile / sheet_columns as usize) as i32 * tile_height as i32;
                let dest_x = x + col_index as i32 * tile_width as i32;
                let dest_y = y + row_index as i32 * tile_height as i32;
                let (dest_x, dest_y) = screen.from_world(dest_x, dest_y);
                screen.target_mut().blit_transformed(
                    &sprite,
                    dest_x,
                    dest_y,
                    0.0,
                    1.0,
                    1.0,
                    0.0,
                    0.0,
                    Some((src_x, src_y, tile_width, tile_height)),
                );
            }
        }
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

/// Text drawn from a loaded bitmap font, rather than the one `font` bakes
/// into the binary.
///
/// `gameText` and this are complementary, not competing: the built-in font
/// needs no asset and is always there, and this is for a game that wants
/// its own pixel art for text instead -- a different style, characters the
/// built-in font does not have, colour baked into the glyphs themselves.
pub mod fonts {
    use super::*;

    /// Load a font atlas and hand back its id.
    ///
    /// `chars` names which character sits in which cell of the atlas,
    /// reading left to right and then top to bottom -- the same order
    /// `gameDrawTilemap` already reads a tile sheet in.
    pub fn load(
        s: &mut Option<Screen>,
        path: &str,
        char_width: &Value,
        char_height: &Value,
        chars: &str,
    ) -> Result<Value, Signal> {
        let char_width = sprites::dimension(char_width, "gameFontLoad", "the character width")?;
        let char_height = sprites::dimension(char_height, "gameFontLoad", "the character height")?;
        let screen = screen(s, "gameFontLoad")?;
        let surface =
            Surface::load_png(path).map_err(|e| value_error(format!("gameFontLoad(): {e}")))?;
        let font = mrt_game::bitmapfont::BitmapFont::new(
            surface,
            char_width as i32,
            char_height as i32,
            chars,
        )
        .map_err(|e| value_error(format!("gameFontLoad(): {e}.")))?;
        screen.fonts.push(Some(font));
        Ok(Value::Number(screen.fonts.len() as f64))
    }

    /// Give a font up. Its id is never reused, the same as a freed surface.
    pub fn free(s: &mut Option<Screen>, id: usize) -> Result<Value, Signal> {
        let screen = screen(s, "gameFontFree")?;
        let index = screen.font_index(id, "gameFontFree")?;
        screen.fonts[index] = None;
        Ok(Value::Null)
    }

    /// Draw text with a loaded font, its top-left corner at `x, y`.
    ///
    /// No self-draw guard, unlike `sprites::draw_transformed`: a font's
    /// atlas lives in its own pool, never in `sprites` and never the
    /// screen, so it can never be the surface currently being drawn onto.
    /// It still has to be taken out of that pool before `target_mut` can
    /// borrow the rest of `screen`, the same borrow-checker reasoning
    /// `draw_transformed` explains -- just without a hazard to guard
    /// against once it is back.
    pub fn draw(
        s: &mut Option<Screen>,
        id: usize,
        x: i32,
        y: i32,
        text: &str,
        scale: i32,
    ) -> Result<Value, Signal> {
        let screen = screen(s, "gameDrawText")?;
        let index = screen.font_index(id, "gameDrawText")?;
        let (x, y) = screen.from_world(x, y);
        let font = screen.fonts[index].take().expect("checked live");
        let result = font.draw(screen.target_mut(), x, y, text, scale);
        screen.fonts[index] = Some(font);
        result.map_err(|c| {
            value_error(format!(
                "gameDrawText(): font {id} has no {c:?} in its character set."
            ))
        })?;
        Ok(Value::Null)
    }

    /// How wide `text` would be drawn with a loaded font -- needs the
    /// screen a font's own pool lives on, unlike `gameTextWidth`'s built-in
    /// font, which needs nothing but the string itself.
    pub fn text_width(
        s: &Option<Screen>,
        id: usize,
        text: &str,
        scale: i32,
    ) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameFontTextWidth"))?;
        let index = screen.font_index(id, "gameFontTextWidth")?;
        let font = screen.fonts[index].as_ref().expect("checked live");
        Ok(Value::Number(font.text_width(text, scale) as f64))
    }

    /// How tall `text` would be drawn with a loaded font.
    pub fn text_height(
        s: &Option<Screen>,
        id: usize,
        text: &str,
        scale: i32,
    ) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameFontTextHeight"))?;
        let index = screen.font_index(id, "gameFontTextHeight")?;
        let font = screen.fonts[index].as_ref().expect("checked live");
        Ok(Value::Number(font.text_height(text, scale) as f64))
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

    /// Text typed since the previous frame, in the order it was typed.
    ///
    /// Distinct from `key_down`/`key_pressed`: those report a physical
    /// key's name, normalised so a game can ask "is W held" the same way
    /// regardless of Shift or the keyboard's own layout. This instead
    /// reports whatever text that key actually produced -- case as typed,
    /// a symbol needing Shift, an accented letter or a non-Latin layout's
    /// own character coming through as itself -- which is what a text
    /// field wants and a "which key is this" query does not.
    #[cfg(feature = "window")]
    pub fn text_input(s: &Option<Screen>) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameTextInput"))?;
        Ok(Value::str(
            screen
                .window
                .as_ref()
                .map(|w| w.text_input())
                .unwrap_or_default(),
        ))
    }

    #[cfg(not(feature = "window"))]
    pub fn text_input(s: &Option<Screen>) -> Result<Value, Signal> {
        s.as_ref().ok_or_else(|| no_screen("gameTextInput"))?;
        Err(unsupported("gameTextInput"))
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

    /// Switch the window between borderless fullscreen and windowed. A no-op
    /// before the window has been opened -- the same rule `gameClose`
    /// follows, since there is nothing yet to switch.
    #[cfg(feature = "window")]
    pub fn set_fullscreen(s: &mut Option<Screen>, fullscreen: bool) -> Result<Value, Signal> {
        let screen = screen(s, "gameSetFullscreen")?;
        if let Some(window) = screen.window.as_mut() {
            window.set_fullscreen(fullscreen);
        }
        Ok(Value::Null)
    }

    #[cfg(not(feature = "window"))]
    pub fn set_fullscreen(s: &mut Option<Screen>, _fullscreen: bool) -> Result<Value, Signal> {
        screen(s, "gameSetFullscreen")?;
        Err(unsupported("gameSetFullscreen"))
    }

    /// Whether the window is fullscreen right now.
    #[cfg(feature = "window")]
    pub fn is_fullscreen(s: &Option<Screen>) -> Result<Value, Signal> {
        let screen = s.as_ref().ok_or_else(|| no_screen("gameIsFullscreen"))?;
        Ok(Value::Bool(
            screen.window.as_ref().is_some_and(|w| w.is_fullscreen()),
        ))
    }

    #[cfg(not(feature = "window"))]
    pub fn is_fullscreen(s: &Option<Screen>) -> Result<Value, Signal> {
        s.as_ref().ok_or_else(|| no_screen("gameIsFullscreen"))?;
        Err(unsupported("gameIsFullscreen"))
    }

    /// Show or hide the pointer while it is over the window.
    #[cfg(feature = "window")]
    pub fn set_cursor_visible(s: &mut Option<Screen>, visible: bool) -> Result<Value, Signal> {
        let screen = screen(s, "gameSetCursorVisible")?;
        if let Some(window) = screen.window.as_mut() {
            window.set_cursor_visible(visible);
        }
        Ok(Value::Null)
    }

    #[cfg(not(feature = "window"))]
    pub fn set_cursor_visible(s: &mut Option<Screen>, _visible: bool) -> Result<Value, Signal> {
        screen(s, "gameSetCursorVisible")?;
        Err(unsupported("gameSetCursorVisible"))
    }

    /// Confine the cursor to the window, or release it. Reports whether it
    /// actually took hold -- `false` before the window is open or on a
    /// platform that refuses every grab mode, either of which leaves the
    /// cursor free to wander despite being asked not to.
    #[cfg(feature = "window")]
    pub fn set_cursor_locked(s: &mut Option<Screen>, locked: bool) -> Result<Value, Signal> {
        let screen = screen(s, "gameSetCursorLocked")?;
        Ok(Value::Bool(
            screen
                .window
                .as_mut()
                .is_some_and(|w| w.set_cursor_locked(locked)),
        ))
    }

    #[cfg(not(feature = "window"))]
    pub fn set_cursor_locked(s: &mut Option<Screen>, _locked: bool) -> Result<Value, Signal> {
        screen(s, "gameSetCursorLocked")?;
        Err(unsupported("gameSetCursorLocked"))
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

    /// Rumble pad `index` at `strength` (0 to 1) for `seconds`, and report
    /// whether it actually started. `false` covers an unknown or
    /// disconnected pad, one with no rumble motor, and a non-positive
    /// strength or duration alike -- the same rule an unrecognised button
    /// or axis name already follows, so a program does not need a
    /// different check for "this controller cannot rumble" than it already
    /// has for "nothing is plugged in".
    #[cfg(feature = "gamepad")]
    pub fn gamepad_rumble(
        s: &mut Option<Screen>,
        index: usize,
        strength: f64,
        seconds: f64,
    ) -> Result<Value, Signal> {
        let screen = screen(s, "gameGamepadRumble")?;
        let gamepads = ready_gamepads(screen, "gameGamepadRumble")?;
        Ok(Value::Bool(gamepads.rumble(index, strength, seconds)))
    }

    #[cfg(not(feature = "gamepad"))]
    pub fn gamepad_rumble(
        s: &mut Option<Screen>,
        _index: usize,
        _strength: f64,
        _seconds: f64,
    ) -> Result<Value, Signal> {
        screen(s, "gameGamepadRumble")?;
        Err(gamepad_unsupported("gameGamepadRumble"))
    }
}
