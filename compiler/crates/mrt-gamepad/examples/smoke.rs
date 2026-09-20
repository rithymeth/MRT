//! Opens the gamepad subsystem and reports what it can, with or without a
//! real controller plugged in.
//!
//! An example rather than a `#[test]` for the same reason `mrt-window`'s is:
//! this is what a person runs by hand to check a real device, and CI runs it
//! too, where it always finds zero gamepads and that has to be a pass, not
//! a failure -- opening successfully with nothing connected is the same
//! promise `mrt-speaker::Speaker::open` makes about a machine with no sound
//! card.
fn main() {
    let mut pads = match mrt_gamepad::Gamepads::new() {
        Ok(p) => p,
        Err(e) => {
            println!("FAIL open: {e}");
            std::process::exit(1);
        }
    };
    println!("opened");

    pads.poll();
    println!("{} connected", pads.count());
    println!(
        "an unknown pad reports nothing rather than erroring: {}",
        !pads.known(0) && !pads.button_down(0, "A") && pads.axis(0, "LeftX") == 0.0
    );
    println!(
        "an unknown button name is never held: {}",
        !pads.button_down(0, "Nonsense")
    );
    println!("ok");
}
