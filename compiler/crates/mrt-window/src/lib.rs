//! The window: the one place in this workspace that talks to an operating
//! system.
//!
//! -- Why this is a crate of its own --
//!
//! Every other crate here has zero dependencies, and the manifest says so in
//! its first line. Showing a framebuffer needs platform FFI and there is no
//! zero-dependency path to it, so the dependencies live here, at a leaf, and
//! nowhere else. `mrt-game` -- which is where all the drawing logic actually
//! is -- stays hermetic, and `cargo test -p mrt-game` stays instant.
//!
//! -- Who owns the loop --
//!
//! A windowing library usually wants the loop: you hand it a callback and it
//! calls you back forever. That is the wrong shape for MRT. It would mean
//! re-entering the interpreter from inside a native call, once per frame,
//! with the VM's frame stack already part-way through a builtin -- and it
//! would make a game's main loop invisible in its own source, hidden inside
//! an engine the reader cannot see.
//!
//! So MRT keeps the loop and this crate is polled:
//!
//! ```text
//! while (gameOpen()) {
//!     gameClear(sky);
//!     ...
//!     gamePresent();
//! }
//! ```
//!
//! `present` pumps the operating system's event queue and puts the frame on
//! the screen. That is what `pump_app_events` is for: it runs the queue until
//! it is empty and then *returns*, instead of taking the thread forever.
//! Without it this design would not be available at all.
//!
//! The cost is that a program which never calls `present` never pumps, and an
//! unpumped window is one the desktop marks as not responding. That is a real
//! trap, and it is the reason `gameOpen` pumps too: a `while (gameOpen())`
//! loop stays responsive even on a frame that draws nothing.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mrt_game::Surface;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::window::WindowId;

/// What the operating system has told us since the last frame.
#[derive(Default)]
struct Input {
    /// Keys currently held, by the name MRT uses for them.
    ///
    /// A set of held keys rather than a stream of events, because a game asks
    /// "is W down *now*" far more often than it asks what happened. Presses
    /// and releases are recorded into the same set as they arrive, so a key
    /// tapped and released between two frames correctly reads as up.
    held: Vec<String>,
    /// Pressed since the last frame was presented, whether or not still held.
    ///
    /// Needed because a key can go down and up inside one frame, and a menu
    /// that only checks `held` would miss it entirely -- which feels like the
    /// game dropping input, and is impossible to reproduce on purpose.
    pressed: Vec<String>,
    pointer: (f64, f64),
    pointer_down: bool,
}

/// The parts winit hands back through its callback.
struct App {
    window: Option<Rc<winit::window::Window>>,
    surface: Option<softbuffer::Surface<Rc<winit::window::Window>, Rc<winit::window::Window>>>,
    input: Input,
    open: bool,
    width: u32,
    height: u32,
    title: String,
    /// Set once the window exists, so `Window::new` can tell the difference
    /// between "still starting" and "this environment has no display".
    created: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = winit::window::Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::LogicalSize::new(self.width, self.height));
        let Ok(window) = event_loop.create_window(attributes) else {
            // No display, or the compositor refused. Reported rather than
            // panicked: a program run over ssh should say so, not abort.
            self.open = false;
            self.created = true;
            return;
        };
        let window = Rc::new(window);
        if let Ok(context) = softbuffer::Context::new(window.clone()) {
            if let Ok(surface) = softbuffer::Surface::new(&context, window.clone()) {
                self.surface = Some(surface);
            }
        }
        self.window = Some(window);
        self.created = true;
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.open = false,
            WindowEvent::KeyboardInput { event, .. } => {
                let Some(name) = key_name(&event.logical_key) else {
                    return;
                };
                match event.state {
                    ElementState::Pressed => {
                        if !self.input.held.contains(&name) {
                            self.input.held.push(name.clone());
                        }
                        if !self.input.pressed.contains(&name) {
                            self.input.pressed.push(name);
                        }
                    }
                    ElementState::Released => self.input.held.retain(|held| held != &name),
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.input.pointer = (position.x, position.y);
            }
            WindowEvent::MouseInput { state, .. } => {
                self.input.pointer_down = state == ElementState::Pressed;
            }
            _ => {}
        }
    }
}

/// The name MRT knows a key by.
///
/// Letters are upper-cased so `gameKeyDown("W")` works whether or not shift
/// is held -- a game asking about W means the physical key, not the character
/// it would type. Named keys get the spelling a person would guess.
fn key_name(key: &Key) -> Option<String> {
    match key {
        Key::Character(text) => {
            let name = text.to_uppercase();
            (!name.is_empty()).then_some(name)
        }
        Key::Named(named) => Some(
            match named {
                NamedKey::ArrowUp => "Up",
                NamedKey::ArrowDown => "Down",
                NamedKey::ArrowLeft => "Left",
                NamedKey::ArrowRight => "Right",
                NamedKey::Space => "Space",
                NamedKey::Enter => "Enter",
                NamedKey::Escape => "Escape",
                NamedKey::Tab => "Tab",
                NamedKey::Backspace => "Backspace",
                NamedKey::Shift => "Shift",
                NamedKey::Control => "Control",
                NamedKey::Alt => "Alt",
                _ => return None,
            }
            .to_string(),
        ),
        _ => None,
    }
}

/// Stretch a frame across a window's pixels, nearest-neighbour.
///
/// The window is whatever size the desktop made it and the game drew at the
/// size it asked for, so something has to reconcile the two. Nearest rather
/// than smooth on purpose: this is a pixel framebuffer, and quietly blurring
/// someone's pixel art would be a strange thing to do on their behalf.
///
/// Split out of `present` because it is the only real logic in this file --
/// everything else is handing values to the operating system -- and a
/// function taking a slice can be tested on a machine with no display, while
/// a window cannot.
fn scale_into(frame: &Surface, out: &mut [u32], width: usize, height: usize) {
    if width == 0 || height == 0 || frame.width == 0 || frame.height == 0 {
        return;
    }
    for y in 0..height {
        // The multiply comes first so the division truncates once, at the
        // end. Scaling y by a ratio first would lose the fraction on every
        // row and drift a line or two off by the bottom of a tall window.
        let source_y = (y * frame.height as usize / height) as i32;
        for x in 0..width {
            let source_x = (x * frame.width as usize / width) as i32;
            let color = frame.get(source_x, source_y).unwrap_or(mrt_game::BLACK);
            // softbuffer wants 0x00RRGGBB. The surface's alpha has already
            // been composited into the picture and means nothing to the
            // desktop.
            out[y * width + x] = color.0 & 0x00ff_ffff;
        }
    }
}

/// A window showing a framebuffer.
pub struct Window {
    event_loop: EventLoop<()>,
    app: App,
    last_frame: Instant,
    delta: f64,
}

impl Window {
    /// Open a window, or explain why not.
    ///
    /// "Explain" rather than "panic": an MRT program that asks for a window
    /// on a machine without one has to be able to catch that and carry on
    /// headless, which `examples/game_bounce.mrt` does. A panic would abort
    /// the process instead -- and this workspace builds release with
    /// `panic = "abort"`, so no amount of `catch_unwind` upstream could
    /// soften it.
    pub fn new(width: u32, height: u32, title: &str) -> Result<Window, String> {
        #[allow(unused_mut)]
        let mut builder = EventLoop::builder();

        // winit panics outright when an event loop is built off the main
        // thread. On macOS and Windows that requirement is real -- the OS
        // demands it. On X11 and Wayland it is only a portability warning,
        // and MRT's interpreter is not guaranteed to be on the main thread:
        // an embedding may run it anywhere, and `cargo test` certainly does.
        // Opting in here is what turns a process abort into an ordinary
        // window on the platform where nothing is actually wrong.
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use winit::platform::wayland::EventLoopBuilderExtWayland;
            use winit::platform::x11::EventLoopBuilderExtX11;
            EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
            EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
        }

        let event_loop = builder
            .build()
            .map_err(|e| format!("no window system: {e}"))?;
        event_loop.set_control_flow(ControlFlow::Poll);
        let mut window = Window {
            event_loop,
            app: App {
                window: None,
                surface: None,
                input: Input::default(),
                open: true,
                width,
                height,
                title: title.to_string(),
                created: false,
            },
            last_frame: Instant::now(),
            delta: 0.0,
        };

        // Pump until the window exists. `resumed` is where a window may be
        // created, and it arrives as an event rather than on demand, so
        // there is nothing to wait on except the queue itself.
        for _ in 0..100 {
            window.pump();
            if window.app.created {
                break;
            }
        }
        if !window.app.created || window.app.window.is_none() {
            return Err("could not open a window; is there a display?".to_string());
        }
        Ok(window)
    }

    fn pump(&mut self) {
        // A zero timeout: drain whatever is waiting and come straight back,
        // rather than sleeping until something happens. The program on the
        // other side has a frame to draw.
        self.event_loop
            .pump_app_events(Some(Duration::ZERO), &mut self.app);
    }

    /// Whether the window is still there, pumping events on the way past.
    ///
    /// Pumping here and not only in `present` is deliberate: it is what keeps
    /// a `while (gameOpen())` loop responsive through a frame that decides to
    /// draw nothing at all.
    pub fn is_open(&mut self) -> bool {
        self.pump();
        self.app.open
    }

    /// Put a frame on the screen.
    pub fn present(&mut self, frame: &Surface) -> Result<(), String> {
        self.pump();

        // Held keys survive a frame; "pressed this frame" does not, by
        // definition. Cleared after the pump so that everything which arrived
        // during this frame is still visible to the code that asked.
        let now = Instant::now();
        self.delta = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;

        let (Some(window), Some(surface)) = (&self.app.window, &mut self.app.surface) else {
            return Err("the window is gone".to_string());
        };

        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            // A minimised window has no pixels. Not an error: the game keeps
            // running, there is just nowhere to put the frame.
            self.app.input.pressed.clear();
            return Ok(());
        };
        surface
            .resize(width, height)
            .map_err(|e| format!("could not size the window: {e}"))?;
        let mut buffer = surface
            .buffer_mut()
            .map_err(|e| format!("could not reach the window's pixels: {e}"))?;

        scale_into(
            frame,
            &mut buffer,
            width.get() as usize,
            height.get() as usize,
        );

        window.pre_present_notify();
        buffer
            .present()
            .map_err(|e| format!("could not show the frame: {e}"))?;
        self.app.input.pressed.clear();
        Ok(())
    }

    /// Seconds since the previous `present`.
    ///
    /// The number a game multiplies movement by, so that something crossing
    /// the screen takes the same time on a fast machine and a slow one.
    pub fn delta(&self) -> f64 {
        self.delta
    }

    pub fn key_down(&self, name: &str) -> bool {
        self.app.input.held.iter().any(|held| held == name)
    }

    pub fn key_pressed(&self, name: &str) -> bool {
        self.app.input.pressed.iter().any(|key| key == name)
    }

    pub fn pointer(&self) -> (f64, f64) {
        self.app.input.pointer
    }

    pub fn pointer_down(&self) -> bool {
        self.app.input.pointer_down
    }

    /// Ask the window to close, as `gameClose()` does.
    pub fn close(&mut self) {
        self.app.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_named_the_way_a_program_would_guess() {
        assert_eq!(
            key_name(&Key::Named(NamedKey::ArrowUp)).as_deref(),
            Some("Up")
        );
        assert_eq!(
            key_name(&Key::Named(NamedKey::Space)).as_deref(),
            Some("Space")
        );
        assert_eq!(
            key_name(&Key::Named(NamedKey::F35)),
            None,
            "unnamed keys are dropped"
        );
    }

    #[test]
    fn a_frame_shown_at_its_own_size_is_untouched() {
        let mut frame = Surface::new(3, 2, mrt_game::BLACK);
        frame.set(0, 0, mrt_game::Color::rgb(255, 0, 0));
        frame.set(2, 1, mrt_game::Color::rgb(0, 0, 255));
        let mut out = vec![0u32; 6];
        scale_into(&frame, &mut out, 3, 2);
        assert_eq!(out[0], 0x00ff_0000, "red, with the alpha byte dropped");
        assert_eq!(out[5], 0x0000_00ff);
        assert_eq!(out[1], 0, "black stays black");
    }

    #[test]
    fn a_frame_in_a_bigger_window_is_stretched_not_cropped() {
        // The failure this rules out is a window showing the top-left corner
        // of the picture, which looks like the game drawing in the wrong
        // place rather than like a scaling bug.
        let mut frame = Surface::new(2, 2, mrt_game::BLACK);
        frame.set(1, 1, mrt_game::Color::rgb(255, 255, 255));
        let mut out = vec![0u32; 16];
        scale_into(&frame, &mut out, 4, 4);
        assert_eq!(out[0], 0, "the dark corner covers a quarter");
        assert_eq!(out[15], 0x00ff_ffff, "and the light one reaches the end");
        assert_eq!(out[10], 0x00ff_ffff, "the light quarter starts halfway");
        assert_eq!(out[9], 0, "but not before");
    }

    #[test]
    fn scaling_reaches_the_last_row_of_a_tall_window() {
        // The multiply-before-divide earns its place here: scaling by a
        // pre-computed ratio loses a fraction per row and lands short.
        let mut frame = Surface::new(1, 3, mrt_game::BLACK);
        frame.set(0, 2, mrt_game::Color::rgb(255, 255, 255));
        let mut out = vec![0u32; 100];
        scale_into(&frame, &mut out, 1, 100);
        assert_eq!(out[99], 0x00ff_ffff, "the bottom row shows the bottom row");
        assert_eq!(out[0], 0);
    }

    #[test]
    fn a_window_with_no_pixels_is_not_an_error() {
        // A minimised window reports zero, and a frame still gets drawn.
        let frame = Surface::new(2, 2, mrt_game::BLACK);
        let mut out: Vec<u32> = Vec::new();
        scale_into(&frame, &mut out, 0, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn a_letter_is_upper_cased_whatever_shift_was_doing() {
        // A game asking about "W" means the physical key, not the character
        // that would have been typed.
        assert_eq!(key_name(&Key::Character("w".into())).as_deref(), Some("W"));
        assert_eq!(key_name(&Key::Character("W".into())).as_deref(), Some("W"));
    }
}
