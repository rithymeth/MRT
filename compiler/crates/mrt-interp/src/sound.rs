//! The MRT-facing side of audio.
//!
//! Sounds follow sprites exactly: a sound is an **id**, a plain number, held
//! by the engine. The reasoning is the same one — a second of stereo audio is
//! 88,200 samples, and as MRT values that is not a slow design but an
//! unusable one — and the consistency is the point. A program that has
//! learned how surfaces work already knows how sounds work.
//!
//! Nothing here plays anything. `mrt-audio` is arithmetic on samples, and
//! whether a mix is *correct* is a question about numbers; a device is a
//! separate crate's problem, the way a window is separate from a framebuffer.
//! What a program can do today is make sounds, combine them, and write a
//! `.wav` — the audio equivalent of saving a PNG, and the thing that stays
//! useful once a device exists, because a test can compare samples and
//! cannot listen.

use std::rc::Rc;
use std::sync::Arc;

use mrt_audio::synth::{Envelope, Wave};
use mrt_audio::Sound;

use crate::error::{type_error, value_error, Signal};
use crate::value::{ObjKey, ObjMap, Value};

/// Every sound a program has made, by 1-based id.
#[derive(Default)]
pub struct Bank {
    /// `Arc` because playing a sound shares it with the audio thread rather
    /// than copying it: the same effect fired twenty times a second would
    /// otherwise allocate a second of audio per shot, in a game loop.
    sounds: Vec<Option<Arc<Sound>>>,
    /// The speaker, once something has asked to play.
    ///
    /// Opened on the first `soundPlay` rather than at startup, the way the
    /// window is: a program that only makes sounds and writes a `.wav` runs
    /// on a machine with no sound card, which includes this repository's own
    /// test suite.
    #[cfg(feature = "audio")]
    speaker: Option<mrt_speaker::Speaker>,
    /// Why opening the speaker failed, if it did.
    ///
    /// Remembered so it is tried *once*. A game calls soundPlay on every
    /// bounce, and on a machine with no sound card each call would otherwise
    /// reopen the device -- which on Linux means ALSA writing several lines
    /// to the terminal, sixty times a second.
    #[cfg(feature = "audio")]
    speaker_failed: Option<String>,
}

fn key(name: &str) -> ObjKey {
    ObjKey::Str(Rc::from(name))
}

impl Bank {
    fn add(&mut self, sound: Sound) -> Value {
        self.sounds.push(Some(Arc::new(sound)));
        Value::Number(self.sounds.len() as f64)
    }

    /// The sound an id names, or why it does not.
    ///
    /// A freed id is told apart from one that never existed, for the reason
    /// surfaces do it: the first is a use-after-free in the program's own
    /// logic, and calling it "no such sound" sends its author looking for a
    /// typo instead.
    fn get(&self, id: usize, who: &str) -> Result<&Arc<Sound>, Signal> {
        if id == 0 {
            return Err(value_error(format!("{who}() needs a sound; 0 is not one.")));
        }
        match self.sounds.get(id - 1) {
            Some(Some(sound)) => Ok(sound),
            Some(None) => Err(value_error(format!("{who}(): sound {id} was freed."))),
            None => Err(value_error(format!("{who}(): there is no sound {id}."))),
        }
    }

    pub fn free(&mut self, id: usize) -> Result<Value, Signal> {
        self.get(id, "soundFree")?;
        self.sounds[id - 1] = None;
        Ok(Value::Null)
    }
}

fn wave_of(name: &str) -> Result<Wave, Signal> {
    match name {
        "square" => Ok(Wave::Square),
        "sine" => Ok(Wave::Sine),
        "saw" => Ok(Wave::Saw),
        "triangle" => Ok(Wave::Triangle),
        "noise" => Ok(Wave::Noise),
        other => Err(value_error(format!(
            "unknown wave {other:?}; expected \"square\", \"sine\", \"saw\", \"triangle\" or \"noise\"."
        ))),
    }
}

/// Read an envelope from an MRT object, filling in what it leaves out.
///
/// Optional because most sounds want the default shaping and saying so four
/// times per call would bury the two numbers that actually differ.
fn envelope_of(value: &Value, who: &str) -> Result<Envelope, Signal> {
    if matches!(value, Value::Null) {
        return Ok(Envelope::default());
    }
    let Value::Object(map) = value else {
        return Err(type_error(format!(
            "{who}() needs the envelope to be an object or null, not {}.",
            crate::value::type_name(value)
        )));
    };
    let map = map.borrow();
    let default = Envelope::default();
    let number = |name: &str, fallback: f64| -> Result<f64, Signal> {
        match map.get(&key(name)) {
            None => Ok(fallback),
            Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => Ok(*n),
            Some(other) => Err(value_error(format!(
                "{who}(): envelope.{name} must be a number that is not negative, not {}.",
                crate::value::stringify(other)
            ))),
        }
    };
    Ok(Envelope {
        attack: number("attack", default.attack)?,
        decay: number("decay", default.decay)?,
        sustain: number("sustain", default.sustain as f64)? as f32,
        release: number("release", default.release)?,
    })
}

/// Read `{volume, pan, speed, loop}`, filling in what it leaves out.
///
/// An object rather than positional arguments: `soundPlay(s, 0.8, -0.3, 1.05,
/// false)` says nothing at the call site about which number is which, and a
/// game fires these constantly.
pub fn play_options(value: &Value, who: &str) -> Result<mrt_audio::mixer::Play, Signal> {
    let mut how = mrt_audio::mixer::Play::default();
    if matches!(value, Value::Null) {
        return Ok(how);
    }
    let Value::Object(map) = value else {
        return Err(type_error(format!(
            "{who}() needs the options to be an object or null, not {}.",
            crate::value::type_name(value)
        )));
    };
    let map = map.borrow();
    let number = |name: &str, fallback: f32, lowest: f64| -> Result<f32, Signal> {
        match map.get(&key(name)) {
            None => Ok(fallback),
            Some(Value::Number(n)) if n.is_finite() && *n >= lowest => Ok(*n as f32),
            Some(other) => Err(value_error(format!(
                "{who}(): {name} is {}.",
                crate::value::stringify(other)
            ))),
        }
    };
    how.volume = number("volume", how.volume, 0.0)?;
    // Pan is the one that may be negative: that is what left means.
    how.pan = number("pan", how.pan, -1.0)?;
    how.speed = number("speed", how.speed, 0.0)?;
    how.looping = matches!(map.get(&key("loop")), Some(Value::Bool(true)));
    Ok(how)
}

/// A number that makes sense as a duration or a frequency.
fn positive(value: &Value, who: &str, what: &str) -> Result<f64, Signal> {
    match value {
        Value::Number(n) if n.is_finite() && *n >= 0.0 => Ok(*n),
        Value::Number(n) => Err(value_error(format!(
            "{who}() needs {what} to be a number that is not negative, not {n}."
        ))),
        other => Err(type_error(format!(
            "{who}() needs {what} to be a number, not {}.",
            crate::value::type_name(other)
        ))),
    }
}

pub fn tone(
    bank: &mut Bank,
    wave: &str,
    hertz: &Value,
    seconds: &Value,
    envelope: &Value,
) -> Result<Value, Signal> {
    let sound = mrt_audio::synth::tone(
        wave_of(wave)?,
        positive(hertz, "soundTone", "the frequency")?,
        positive(seconds, "soundTone", "the length")?,
        envelope_of(envelope, "soundTone")?,
        mrt_audio::RATE,
    );
    Ok(bank.add(sound))
}

pub fn sweep(
    bank: &mut Bank,
    wave: &str,
    from: &Value,
    to: &Value,
    seconds: &Value,
    envelope: &Value,
) -> Result<Value, Signal> {
    let sound = mrt_audio::synth::sweep(
        wave_of(wave)?,
        positive(from, "soundSweep", "the starting frequency")?,
        positive(to, "soundSweep", "the ending frequency")?,
        positive(seconds, "soundSweep", "the length")?,
        envelope_of(envelope, "soundSweep")?,
        mrt_audio::RATE,
    );
    Ok(bank.add(sound))
}

pub fn load(bank: &mut Bank, path: &str) -> Result<Value, Signal> {
    let bytes = std::fs::read(path)
        .map_err(|e| value_error(format!("soundLoad(): could not read {path}: {e}")))?;
    let sound =
        mrt_audio::wav::decode(&bytes).map_err(|e| value_error(format!("soundLoad(): {e}")))?;
    Ok(bank.add(sound))
}

pub fn save(bank: &Bank, id: usize, path: &str) -> Result<Value, Signal> {
    let sound = bank.get(id, "soundSave")?;
    std::fs::write(path, mrt_audio::wav::encode(sound))
        .map_err(|e| value_error(format!("soundSave(): could not write {path}: {e}")))?;
    Ok(Value::Null)
}

/// `soundMix([{sound: id, volume: 0.5}, ...])`.
pub fn mix(bank: &mut Bank, parts: &Value) -> Result<Value, Signal> {
    let Value::Array(items) = parts else {
        return Err(type_error(format!(
            "soundMix() expects an array of parts, not {}.",
            crate::value::type_name(parts)
        )));
    };
    let mut collected = Vec::new();
    for (index, item) in items.borrow().iter().enumerate() {
        let Value::Object(map) = item else {
            return Err(type_error(format!(
                "soundMix(): part {index} is not an object with a sound and a volume."
            )));
        };
        let map = map.borrow();
        let Some(Value::Number(id)) = map.get(&key("sound")) else {
            return Err(value_error(format!(
                "soundMix(): part {index} has no 'sound'."
            )));
        };
        if *id < 0.0 || id.fract() != 0.0 {
            return Err(value_error(format!(
                "soundMix(): part {index} has {id} as a sound id."
            )));
        }
        let volume = match map.get(&key("volume")) {
            None => 1.0,
            Some(Value::Number(v)) if v.is_finite() => *v as f32,
            Some(_) => {
                return Err(value_error(format!(
                    "soundMix(): part {index} has a volume that is not a number."
                )))
            }
        };
        collected.push((bank.get(*id as usize, "soundMix")?.as_ref().clone(), volume));
    }
    Ok(bank.add(mrt_audio::mix(&collected)))
}

/// One sound after another.
pub fn then(bank: &mut Bank, first: usize, second: usize) -> Result<Value, Signal> {
    let joined = bank
        .get(first, "soundThen")?
        .then(bank.get(second, "soundThen")?);
    Ok(bank.add(joined))
}

/// A copy at a different volume, or scaled so its loudest sample sits there.
pub fn gain(bank: &mut Bank, id: usize, amount: f64, normalize: bool) -> Result<Value, Signal> {
    let sound = bank.get(
        id,
        if normalize {
            "soundNormalize"
        } else {
            "soundGain"
        },
    )?;
    let out = if normalize {
        sound.normalized(amount as f32)
    } else {
        sound.gain(amount as f32)
    };
    Ok(bank.add(out))
}

/// `{seconds, frames, channels, rate, peak}`.
pub fn info(bank: &Bank, id: usize) -> Result<Value, Signal> {
    let sound = bank.get(id, "soundInfo")?;
    let mut map = ObjMap::new();
    map.insert(key("seconds"), Value::Number(sound.seconds()));
    map.insert(key("frames"), Value::Number(sound.frames() as f64));
    map.insert(key("channels"), Value::Number(sound.channels as f64));
    map.insert(key("rate"), Value::Number(sound.rate as f64));
    // The number that says whether a mix will clip when it is written out.
    map.insert(key("peak"), Value::Number(sound.peak() as f64));
    Ok(Value::object(map))
}

#[cfg(test)]
mod tests {
    use crate::{run, run_vm, SourceFile};

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

    #[test]
    fn sound_needs_no_screen() {
        // Audio is arithmetic on samples. A program making a jingle should
        // not have to open a framebuffer first.
        assert_eq!(
            main_of(
                r#"var s = soundTone("sine", 440, 0.5, null);
                   var info = soundInfo(s);
                   print(s, info.seconds, info.channels, info.rate);"#
            ),
            "1 0.5 1 44100"
        );
    }

    #[test]
    fn an_envelope_fills_in_what_it_leaves_out() {
        // Most sounds want the default shaping, and saying so four times per
        // call would bury the numbers that actually differ.
        assert_eq!(
            main_of(
                r#"var a = soundTone("square", 440, 0.2, null);
                   var b = soundTone("square", 440, 0.2, {attack: 0.05});
                   print(soundInfo(a).frames == soundInfo(b).frames);
                   print(soundInfo(a).peak > 0.9, soundInfo(b).peak > 0.9);"#
            ),
            "true\ntrue true"
        );
    }

    #[test]
    fn mixing_adds_and_says_when_it_would_clip() {
        // Nothing is clamped on the way in: clamping silently is distortion
        // the caller never asked for, and `peak` is how they find out.
        assert_eq!(
            main_of(
                r#"var a = soundTone("square", 440, 0.1, {attack: 0, decay: 0, sustain: 1, release: 0});
                   var stacked = soundMix([{sound: a, volume: 1}, {sound: a, volume: 1}]);
                   print(soundInfo(stacked).peak);
                   print(soundInfo(soundNormalize(stacked, 0.5)).peak);"#
            ),
            "2\n0.5"
        );
    }

    #[test]
    fn a_default_volume_is_full() {
        assert_eq!(
            main_of(
                r#"var a = soundTone("square", 440, 0.05, {attack: 0, decay: 0, sustain: 1, release: 0});
                   print(soundInfo(soundMix([{sound: a}])).peak);"#
            ),
            "1"
        );
    }

    #[test]
    fn one_sound_after_another_adds_their_lengths() {
        assert_eq!(
            main_of(
                r#"var a = soundTone("sine", 440, 0.2, null);
                   var b = soundTone("sine", 880, 0.3, null);
                   print(round(soundInfo(soundThen(a, b)).seconds * 100) / 100);"#
            ),
            "0.5"
        );
    }

    #[test]
    fn a_sound_survives_being_written_and_read() {
        let path = std::env::temp_dir().join("mrt_sound_round_trip.wav");
        let escaped = path.to_string_lossy().replace('\\', "\\\\");
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            main_of(&format!(
                r#"var s = soundSweep("saw", 200, 900, 0.25, null);
                   soundSave(s, "{escaped}");
                   var back = soundLoad("{escaped}");
                   var a = soundInfo(s);
                   var b = soundInfo(back);
                   print(a.frames == b.frames, a.channels == b.channels, a.rate == b.rate);
                   print(round(b.peak * 10) / 10 == round(a.peak * 10) / 10);"#
            )),
            "true true true\ntrue"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unknown_wave_lists_the_ones_there_are() {
        let output = main_of(r#"soundTone("bagpipes", 440, 0.1, null);"#);
        assert!(
            output.contains("unknown wave \"bagpipes\""),
            "got {output:?}"
        );
        assert!(output.contains("\"square\""), "and names the real ones");
    }

    #[test]
    fn a_freed_sound_is_told_apart_from_one_that_never_existed() {
        assert_eq!(
            main_of(
                r#"var s = soundTone("sine", 440, 0.1, null);
                   soundFree(s);
                   soundInfo(s);"#
            ),
            "Runtime Error: soundInfo(): sound 1 was freed. [line 3]"
        );
        assert_eq!(
            main_of("soundInfo(9);"),
            "Runtime Error: soundInfo(): there is no sound 9. [line 1]"
        );
        assert_eq!(
            main_of("soundInfo(0);"),
            "Runtime Error: soundInfo() needs a sound; 0 is not one. [line 1]"
        );
    }

    #[test]
    fn a_malformed_mix_says_which_part_is_wrong() {
        assert_eq!(
            main_of("soundMix([{volume: 1}]);"),
            "Runtime Error: soundMix(): part 0 has no 'sound'. [line 1]"
        );
        assert_eq!(
            main_of("soundMix([5]);"),
            "Runtime Error: soundMix(): part 0 is not an object with a sound and a volume. [line 1]"
        );
        assert_eq!(
            main_of("soundMix(7);"),
            "Runtime Error: soundMix() expects an array of parts, not number. [line 1]"
        );
    }

    #[test]
    fn loading_something_that_is_not_a_wav_says_why() {
        let path = std::env::temp_dir().join("mrt_sound_not_a_wav.wav");
        std::fs::write(&path, b"nope").expect("writes");
        let escaped = path.to_string_lossy().replace('\\', "\\\\");
        let output = main_of(&format!(r#"soundLoad("{escaped}");"#));
        let _ = std::fs::remove_file(&path);
        assert!(output.contains("not a WAV file"), "got {output:?}");

        let missing = main_of(r#"soundLoad("/nope/nowhere.wav");"#);
        assert!(missing.contains("could not read"), "got {missing:?}");
    }

    /// Playback behaves differently depending on how the crate was built,
    /// so the tests do too. Both arrangements ship: `cargo test -p
    /// mrt-interp` runs without the feature, and a workspace build turns it
    /// on through the CLI.
    #[test]
    #[cfg(not(feature = "audio"))]
    fn a_build_without_audio_support_says_which_it_is() {
        // Not a quiet nothing: a soundPlay that silently did nothing looks
        // exactly like a sound that failed to load.
        assert_eq!(
            main_of(r#"var s = soundTone("sine", 440, 0.1, null); soundPlay(s);"#),
            "Runtime Error: soundPlay() needs a build with audio support; this one was built without it. [line 1]"
        );
    }

    #[test]
    #[cfg(feature = "audio")]
    fn with_no_sound_card_playing_is_an_error_a_program_can_catch() {
        // The property a game needs to run on a build server: asking for
        // sound where there is none must be catchable, not fatal. This is
        // the one part of playback testable without hardware -- and on a
        // machine that *has* a sound card it opens one, which is also fine.
        assert_eq!(
            main_of(
                r#"var s = soundTone("sine", 440, 0.05, null);
                   var played = false;
                   try { soundPlay(s); played = true; } catch (e) { print("caught", e.kind); }
                   print("carried on");"#
            )
            .lines()
            .last()
            .unwrap(),
            "carried on"
        );
    }

    #[test]
    #[cfg(feature = "audio")]
    fn a_speaker_that_will_not_open_is_only_tried_once() {
        // A game calls soundPlay on every bounce. Retrying the device each
        // time means, on Linux with no sound card, ALSA writing to the
        // terminal sixty times a second -- so the failure is remembered.
        //
        // Both outcomes are fine; what is checked is that a hundred calls
        // behave like the first and the program survives them.
        assert_eq!(
            main_of(
                r#"var s = soundTone("sine", 440, 0.02, null);
                   var failures = 0;
                   for (var i = 0; i < 100; i = i + 1) {
                       try { soundPlay(s); } catch (e) { failures = failures + 1; }
                   }
                   print(failures == 0 || failures == 100);"#
            ),
            "true",
            "all of them worked or none did, never a mixture"
        );
    }

    #[test]
    #[cfg(feature = "audio")]
    fn play_options_are_read_and_checked() {
        // The volume/pan/speed object, whether or not a device exists: a bad
        // option is refused before anything is opened.
        assert_eq!(
            main_of(r#"var s = soundTone("sine", 440, 0.02, null); soundPlay(s, {volume: -1});"#),
            "Runtime Error: soundPlay(): volume is -1. [line 1]"
        );
        assert_eq!(
            main_of(r#"var s = soundTone("sine", 440, 0.02, null); soundPlay(s, {pan: -2});"#),
            "Runtime Error: soundPlay(): pan is -2. [line 1]"
        );
        assert_eq!(
            main_of(r#"var s = soundTone("sine", 440, 0.02, null); soundPlay(s, 5);"#),
            "Runtime Error: soundPlay() needs the options to be an object or null, not number. [line 1]"
        );
    }

    #[test]
    #[cfg(feature = "audio")]
    fn stopping_when_nothing_plays_is_not_an_error() {
        // A game stopping its music on a screen that never started any is
        // ordinary, and should not need a guard around it.
        assert_eq!(main_of("soundStopAll(); print(soundPlaying());"), "0");
    }

    #[test]
    fn a_negative_length_is_refused_rather_than_rounded_away() {
        assert_eq!(
            main_of(r#"soundTone("sine", 440, -1, null);"#),
            "Runtime Error: soundTone() needs the length to be a number that is not negative, not -1. [line 1]"
        );
    }
}

/// Playing sounds out loud.
///
/// Every function exists twice, once against a real speaker and once as a
/// refusal for a build without the `audio` feature -- the same arrangement
/// the window half uses, and for the same reason: a `soundPlay` that quietly
/// did nothing would look like a sound that failed to load, and send its
/// author looking in the wrong place.
pub mod live {
    use super::*;

    #[cfg(not(feature = "audio"))]
    fn unsupported(who: &str) -> Signal {
        value_error(format!(
            "{who}() needs a build with audio support; this one was built without it."
        ))
    }

    /// Start a sound, opening the speaker if this is the first one.
    #[cfg(feature = "audio")]
    pub fn play(bank: &mut Bank, id: usize, how: mrt_audio::mixer::Play) -> Result<Value, Signal> {
        let sound = bank.get(id, "soundPlay")?.clone();
        if let Some(why) = &bank.speaker_failed {
            return Err(value_error(format!("soundPlay(): {why}")));
        }
        if bank.speaker.is_none() {
            match mrt_speaker::Speaker::open() {
                Ok(speaker) => bank.speaker = Some(speaker),
                Err(why) => {
                    bank.speaker_failed = Some(why.clone());
                    return Err(value_error(format!("soundPlay(): {why}")));
                }
            }
        }
        let speaker = bank.speaker.as_ref().expect("just opened");
        Ok(Value::Number(speaker.play(sound, how) as f64))
    }

    #[cfg(not(feature = "audio"))]
    pub fn play(bank: &mut Bank, id: usize, _how: mrt_audio::mixer::Play) -> Result<Value, Signal> {
        bank.get(id, "soundPlay")?;
        Err(unsupported("soundPlay"))
    }

    /// Stop one voice, or everything.
    #[cfg(feature = "audio")]
    pub fn stop(bank: &Bank, voice: Option<u64>) -> Result<Value, Signal> {
        if let Some(speaker) = bank.speaker.as_ref() {
            match voice {
                Some(voice) => speaker.stop(voice),
                None => speaker.stop_all(),
            }
        }
        // Stopping when nothing is playing is not an error: a game stopping
        // its music on a screen that never started any is ordinary.
        Ok(Value::Null)
    }

    #[cfg(not(feature = "audio"))]
    pub fn stop(_bank: &Bank, _voice: Option<u64>) -> Result<Value, Signal> {
        Err(unsupported("soundStop"))
    }

    /// How many voices are sounding right now.
    #[cfg(feature = "audio")]
    pub fn playing(bank: &Bank) -> Result<Value, Signal> {
        Ok(Value::Number(
            bank.speaker.as_ref().map(|s| s.playing()).unwrap_or(0) as f64,
        ))
    }

    #[cfg(not(feature = "audio"))]
    pub fn playing(_bank: &Bank) -> Result<Value, Signal> {
        Err(unsupported("soundPlaying"))
    }
}
