//! Opens a real window, presents real frames, and reports what happened.
//!
//! An example rather than a `#[test]` because winit insists an event loop is
//! built on the main thread, and cargo runs tests on spawned ones. Driven by
//! `scripts/check-window.sh`, which supplies a virtual display.
fn main() {
    let mut window = match mrt_window::Window::new(320, 200, "MRT smoke") {
        Ok(w) => w,
        Err(e) => {
            println!("FAIL open: {e}");
            std::process::exit(1);
        }
    };
    println!("opened");

    let mut frame = mrt_game::Surface::new(320, 200, mrt_game::Color::rgb(18, 22, 38));
    for n in 0..5 {
        frame.fill_rect(20 * n, 40, 60, 60, mrt_game::Color::rgb(230, 80, 60));
        if let Err(e) = window.present(&frame) {
            println!("FAIL present: {e}");
            std::process::exit(1);
        }
        if !window.is_open() {
            println!("FAIL closed early");
            std::process::exit(1);
        }
    }
    println!("presented 5 frames");
    println!("delta is a real duration: {}", window.delta() >= 0.0);
    println!("no key is down: {}", !window.key_down("W"));
    println!("nothing typed either: {}", window.text_input().is_empty());

    if window.is_fullscreen() {
        println!("FAIL starts fullscreen");
        std::process::exit(1);
    }
    window.set_fullscreen(true);
    window.is_open(); // pump: fullscreen is an async request to the desktop
    if !window.is_fullscreen() {
        println!("FAIL did not become fullscreen");
        std::process::exit(1);
    }
    window.set_fullscreen(false);
    window.is_open();
    if window.is_fullscreen() {
        println!("FAIL did not return to windowed");
        std::process::exit(1);
    }
    println!("fullscreen toggles both ways");

    window.set_cursor_visible(false);
    window.set_cursor_visible(true);
    println!("cursor visibility calls did not panic");

    let locked = window.set_cursor_locked(true);
    println!("cursor lock reports its own success: {locked}");
    window.set_cursor_locked(false);
    println!("cursor unlock did not panic");

    window.close();
    println!("closes on request: {}", !window.is_open());
    println!("ok");
}
