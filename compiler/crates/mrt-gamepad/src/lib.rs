//! Gamepads: the third place in this workspace that talks to an operating
//! system, alongside the window and the speaker.
//!
//! -- Why this is a crate of its own --
//!
//! The same reason the window and the speaker are. A controller needs
//! platform FFI and there is no zero-dependency path to it, so the
//! dependency lives at a leaf and `mrt-game` -- where the logic that
//! actually plays a game lives -- stays hermetic and instant to test.
//!
//! -- Polled, not driven by a callback --
//!
//! The same shape `mrt-window` takes, for the same reason: a program asks
//! "is this button down" the same way it already asks "is W down", and gets
//! the same two kinds of answer -- held right now, and pressed since the
//! last frame. Nothing here owns a thread the interpreter would have to be
//! re-entered from; [`Gamepads::poll`] drains whatever has happened before
//! every query, and [`Gamepads::end_frame`] is what actually starts a fresh
//! frame for "pressed" -- called once, from `gamePresent`, the same point
//! that already ends a frame for the keyboard.
//!
//! -- Names, not raw indices --
//!
//! `gilrs` already gives every button and axis a semantic name (`South`,
//! `LeftStickX`, ...), aimed at the same "which finger, not which wire on
//! this specific pad" abstraction MRT's own `"W"` / `"Left"` key names use.
//! This crate translates those into the Xbox-style names most controllers
//! and most players already think in -- `"A"`, `"LeftStick"`, `"LeftX"` --
//! since asking an MRT program to know what `South` means on a gamepad
//! would be asking it to know more about `gilrs` than about the game.

use std::collections::HashSet;

use gilrs::{Axis as GAxis, Button as GButton, EventType, Gilrs};

/// One connected gamepad's state, as of the last [`Gamepads::poll`].
#[derive(Default)]
struct Pad {
    held: HashSet<&'static str>,
    pressed: HashSet<&'static str>,
}

/// Every connected gamepad, polled once a frame.
pub struct Gamepads {
    gilrs: Gilrs,
    // Indexed the same way `count()` and every other method report a pad:
    // 0-based, in the order `gilrs` first saw each one connect. A `gilrs`
    // `GamepadId` is not that -- it is stable per physical device but not
    // contiguous or ordered -- so this is the translation from "the second
    // pad a player plugged in" to the id gilrs actually uses.
    order: Vec<gilrs::GamepadId>,
    pads: Vec<Pad>,
}

/// A button name to the `gilrs` button it maps to, or `None` for a name this
/// crate does not recognise.
///
/// Xbox-style names, because that is what most controllers and most players
/// already think in -- `gilrs`'s own `South`/`East`/`North`/`West` name the
/// physical position of a button that only has a letter on an Xbox pad by
/// convention, not by hardware.
fn button_named(name: &str) -> Option<GButton> {
    Some(match name {
        "A" => GButton::South,
        "B" => GButton::East,
        "X" => GButton::West,
        "Y" => GButton::North,
        "LeftBumper" => GButton::LeftTrigger,
        "RightBumper" => GButton::RightTrigger,
        "LeftTrigger" => GButton::LeftTrigger2,
        "RightTrigger" => GButton::RightTrigger2,
        "LeftStick" => GButton::LeftThumb,
        "RightStick" => GButton::RightThumb,
        "Back" => GButton::Select,
        "Start" => GButton::Start,
        "Guide" => GButton::Mode,
        "DPadUp" => GButton::DPadUp,
        "DPadDown" => GButton::DPadDown,
        "DPadLeft" => GButton::DPadLeft,
        "DPadRight" => GButton::DPadRight,
        _ => return None,
    })
}

/// The name this crate reports a `gilrs` button under, the inverse of
/// [`button_named`]. `None` for a button `gilrs` reports that has no Xbox
/// analogue (`C`, `Z`, and its own `Unknown`) -- reported as held by nothing
/// rather than under a name nobody asked for.
fn name_of(button: GButton) -> Option<&'static str> {
    Some(match button {
        GButton::South => "A",
        GButton::East => "B",
        GButton::West => "X",
        GButton::North => "Y",
        GButton::LeftTrigger => "LeftBumper",
        GButton::RightTrigger => "RightBumper",
        GButton::LeftTrigger2 => "LeftTrigger",
        GButton::RightTrigger2 => "RightTrigger",
        GButton::LeftThumb => "LeftStick",
        GButton::RightThumb => "RightStick",
        GButton::Select => "Back",
        GButton::Start => "Start",
        GButton::Mode => "Guide",
        GButton::DPadUp => "DPadUp",
        GButton::DPadDown => "DPadDown",
        GButton::DPadLeft => "DPadLeft",
        GButton::DPadRight => "DPadRight",
        _ => return None,
    })
}

/// An axis name to the `gilrs` axis it maps to.
fn axis_named(name: &str) -> Option<GAxis> {
    Some(match name {
        "LeftX" => GAxis::LeftStickX,
        "LeftY" => GAxis::LeftStickY,
        "RightX" => GAxis::RightStickX,
        "RightY" => GAxis::RightStickY,
        _ => return None,
    })
}

impl Gamepads {
    /// Open the gamepad subsystem, or explain why not.
    ///
    /// "Explain" rather than "panic", the same reason the window and the
    /// speaker do: a program that asks for a controller on a machine with
    /// none, or with no permission to read input devices, has to be able to
    /// catch that and carry on without one. No controller needs to be
    /// plugged in for this to succeed -- it can succeed with zero gamepads
    /// connected, the same way opening a speaker does not require a sound
    /// to be playing.
    pub fn new() -> Result<Gamepads, String> {
        let gilrs = Gilrs::new().map_err(|e| format!("could not read game controllers: {e}"))?;
        let order: Vec<_> = gilrs.gamepads().map(|(id, _)| id).collect();
        let pads = order.iter().map(|_| Pad::default()).collect();
        Ok(Gamepads { gilrs, order, pads })
    }

    /// Bring every pad's held state up to date with what has happened since
    /// the last call, and add to (never clear) what has been pressed since
    /// the last [`end_frame`](Self::end_frame).
    ///
    /// Called before every query rather than once a frame on purpose: a
    /// program asking `button_down` and then `button_pressed` for two
    /// different buttons in the same frame must see the same answer either
    /// way asks first, which a `poll` that cleared `pressed` itself could
    /// not promise -- the first query would silently erase what the second
    /// one was about to ask for.
    pub fn poll(&mut self) {
        while let Some(event) = self.gilrs.next_event() {
            let index = match self.order.iter().position(|&id| id == event.id) {
                Some(index) => index,
                None => {
                    // A pad connected after `new()` opened. Appended, never
                    // inserted: an index already handed to a program must
                    // keep meaning the same physical pad for as long as
                    // that pad stays connected.
                    self.order.push(event.id);
                    self.pads.push(Pad::default());
                    self.order.len() - 1
                }
            };
            let Some(pad) = self.pads.get_mut(index) else {
                continue;
            };
            match event.event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(name) = name_of(button) {
                        pad.held.insert(name);
                        pad.pressed.insert(name);
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    if let Some(name) = name_of(button) {
                        pad.held.remove(name);
                    }
                }
                // Disconnecting does not drop the slot: the index is a
                // promise to whatever already asked about it, and a
                // disconnected pad reads as every button up and every axis
                // at rest rather than as a different pad taking its place.
                EventType::Disconnected => pad.held.clear(),
                _ => {}
            }
        }
    }

    /// Mark the end of a frame: what has been pressed since here is what the
    /// next frame's queries see, and everything before it is forgotten.
    ///
    /// Kept apart from `poll` so that a frame with several presses queried
    /// in any order all see the same set -- see `poll` for why merging the
    /// two would break that.
    pub fn end_frame(&mut self) {
        for pad in &mut self.pads {
            pad.pressed.clear();
        }
    }

    /// How many gamepads are connected right now.
    pub fn count(&self) -> usize {
        self.order
            .iter()
            .filter(|id| self.gilrs.connected_gamepad(**id).is_some())
            .count()
    }

    /// Whether `index` names a pad that has ever connected, connected or not
    /// -- the same "was this ever real" question `gameDraw` asks about a
    /// freed sprite, and for the same reason: told apart from an index that
    /// never named anything.
    pub fn known(&self, index: usize) -> bool {
        index < self.order.len()
    }

    pub fn button_exists(name: &str) -> bool {
        button_named(name).is_some()
    }

    pub fn axis_exists(name: &str) -> bool {
        axis_named(name).is_some()
    }

    /// Whether `name` is held on pad `index` right now. `false` for an
    /// unknown or disconnected pad rather than an error -- a controller
    /// unplugged mid-game should read as nothing held, not stop the game
    /// that was reading it.
    pub fn button_down(&self, index: usize, name: &str) -> bool {
        button_named(name).is_some()
            && self
                .pads
                .get(index)
                .is_some_and(|pad| pad.held.contains(name))
    }

    /// Whether `name` went down on pad `index` since the last [`poll`](Self::poll).
    pub fn button_pressed(&self, index: usize, name: &str) -> bool {
        button_named(name).is_some()
            && self
                .pads
                .get(index)
                .is_some_and(|pad| pad.pressed.contains(name))
    }

    /// `name`'s current value on pad `index`, from -1 to 1, or 0 for an
    /// unknown or disconnected pad -- centred, the same rest position a real
    /// stick returns to rather than a value that reads as pushed one way.
    pub fn axis(&self, index: usize, name: &str) -> f64 {
        let Some(axis) = axis_named(name) else {
            return 0.0;
        };
        let Some(&id) = self.order.get(index) else {
            return 0.0;
        };
        self.gilrs
            .connected_gamepad(id)
            .map(|pad| pad.value(axis) as f64)
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUTTON_NAMES: &[&str] = &[
        "A",
        "B",
        "X",
        "Y",
        "LeftBumper",
        "RightBumper",
        "LeftTrigger",
        "RightTrigger",
        "LeftStick",
        "RightStick",
        "Back",
        "Start",
        "Guide",
        "DPadUp",
        "DPadDown",
        "DPadLeft",
        "DPadRight",
    ];

    #[test]
    fn every_button_name_round_trips_through_gilrs_and_back() {
        for &name in BUTTON_NAMES {
            let button = button_named(name).unwrap_or_else(|| panic!("{name} is not mapped"));
            assert_eq!(
                name_of(button),
                Some(name),
                "{name} maps to a gilrs button that reports back as something else"
            );
        }
    }

    #[test]
    fn an_unrecognised_name_maps_to_nothing() {
        assert_eq!(button_named("Nonsense"), None);
        assert_eq!(axis_named("Nonsense"), None);
        assert!(!Gamepads::button_exists("Nonsense"));
        assert!(!Gamepads::axis_exists("Nonsense"));
    }

    #[test]
    fn every_documented_name_is_recognised() {
        for &name in BUTTON_NAMES {
            assert!(Gamepads::button_exists(name), "{name} should be known");
        }
        for name in ["LeftX", "LeftY", "RightX", "RightY"] {
            assert!(Gamepads::axis_exists(name), "{name} should be known");
        }
    }

    #[test]
    fn a_button_gilrs_reports_with_no_xbox_analogue_names_nothing() {
        // C, Z and gilrs's own Unknown exist on some pads but have no
        // Xbox-style name to report -- silently unmapped rather than a
        // panic, since a real controller sending one is not a bug.
        assert_eq!(name_of(GButton::C), None);
        assert_eq!(name_of(GButton::Z), None);
        assert_eq!(name_of(GButton::Unknown), None);
    }

    // Gilrs::new() only needs a working udev/evdev, not an actual
    // controller, so these run the same in CI as on a desk with a real pad
    // plugged in -- the same "succeeds with nothing connected" promise
    // mrt-speaker's Speaker::open makes about a machine with no sound card.
    #[test]
    fn opens_with_zero_gamepads_connected() {
        let pads = Gamepads::new().expect("gilrs should open with no controllers at all");
        assert_eq!(pads.count(), 0);
    }

    #[test]
    fn an_index_that_never_existed_reports_nothing_rather_than_erroring() {
        let mut pads = Gamepads::new().expect("opens");
        pads.poll();
        assert!(!pads.known(0));
        assert!(!pads.button_down(0, "A"));
        assert!(!pads.button_pressed(0, "A"));
        assert_eq!(pads.axis(0, "LeftX"), 0.0);
    }
}
