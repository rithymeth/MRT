//! Making sounds from nothing.
//!
//! -- Why a synthesiser and not just file loading --
//!
//! The same reason `mrt-game` draws shapes rather than only loading sprites:
//! a game that needs an artist before it can show anything is a game nobody
//! starts. A jump, a coin, a hurt noise and a menu blip are a few hundred
//! samples of a square wave with an envelope on them, and being able to write
//! them in the program means the scaffolded game can have sound without
//! shipping any assets at all.
//!
//! These are the waveforms early game hardware had, for the same reason: they
//! are the ones you can compute a sample at a time with no tables.

use crate::Sound;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wave {
    /// Hollow and buzzy -- the classic bleep.
    Square,
    /// Smooth. A pure tone with no harmonics at all.
    Sine,
    /// Bright and harsh, every harmonic present.
    Saw,
    /// Soft, between sine and square in character.
    Triangle,
    /// No pitch at all: explosions, footsteps, static.
    Noise,
}

/// How a sound's volume changes over its life.
///
/// Attack and release are what stop a note clicking. A waveform that starts
/// instantly at full volume begins with a vertical step, and a speaker asked
/// to jump like that makes an audible tick at the front of every sound --
/// which, across a game firing hundreds of them, is the difference between
/// effects and crackle.
#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    pub attack: f64,
    pub decay: f64,
    pub sustain: f32,
    pub release: f64,
}

impl Default for Envelope {
    fn default() -> Envelope {
        Envelope {
            attack: 0.005,
            decay: 0.03,
            sustain: 0.7,
            release: 0.08,
        }
    }
}

impl Envelope {
    /// The volume at a moment, given how long the whole sound lasts.
    fn at(&self, time: f64, length: f64) -> f32 {
        let release_starts = (length - self.release).max(0.0);
        if time < self.attack && self.attack > 0.0 {
            (time / self.attack) as f32
        } else if time < self.attack + self.decay && self.decay > 0.0 {
            let through = ((time - self.attack) / self.decay) as f32;
            1.0 - through * (1.0 - self.sustain)
        } else if time >= release_starts && self.release > 0.0 {
            let left = ((length - time) / self.release).clamp(0.0, 1.0) as f32;
            self.sustain * left
        } else {
            self.sustain
        }
    }
}

/// A deterministic noise source.
///
/// Seeded rather than random, because two runs of a program that made a
/// sound differently each time could not be tested at all -- and a game's
/// explosion does not need to be unique, only to sound like one.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        // xorshift32: three shifts, no tables, and a period long enough that
        // a few seconds of audio never repeats.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Generate a tone.
pub fn tone(wave: Wave, hertz: f64, seconds: f64, envelope: Envelope, rate: u32) -> Sound {
    let frames = (seconds * rate as f64).round().max(0.0) as usize;
    let mut samples = Vec::with_capacity(frames);
    let mut noise = Noise(0x2545_F491);

    for frame in 0..frames {
        let time = frame as f64 / rate as f64;
        // Where in the wave's cycle this moment falls, 0 to 1.
        let phase = (time * hertz).fract();
        let value = match wave {
            Wave::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Sine => (phase * std::f64::consts::TAU).sin() as f32,
            Wave::Saw => (phase * 2.0 - 1.0) as f32,
            Wave::Triangle => (1.0 - (phase * 4.0 - 2.0).abs()) as f32,
            Wave::Noise => noise.next(),
        };
        samples.push(value * envelope.at(time, seconds));
    }

    Sound {
        rate,
        channels: 1,
        samples,
    }
}

/// A tone whose pitch slides from one frequency to another.
///
/// Every arcade sound that is not a flat beep is this: a coin slides up, a
/// hurt noise slides down, a laser slides down fast.
pub fn sweep(wave: Wave, from: f64, to: f64, seconds: f64, envelope: Envelope, rate: u32) -> Sound {
    let frames = (seconds * rate as f64).round().max(0.0) as usize;
    let mut samples = Vec::with_capacity(frames);
    let mut noise = Noise(0x9E37_79B9);
    // The phase is accumulated rather than recomputed from the time, because
    // a changing frequency means `time * hertz` jumps whenever hertz does --
    // and a jump in phase is a click.
    let mut phase = 0.0f64;

    for frame in 0..frames {
        let time = frame as f64 / rate as f64;
        let through = if seconds > 0.0 { time / seconds } else { 0.0 };
        let hertz = from + (to - from) * through;
        phase = (phase + hertz / rate as f64).fract();

        let value = match wave {
            Wave::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Sine => (phase * std::f64::consts::TAU).sin() as f32,
            Wave::Saw => (phase * 2.0 - 1.0) as f32,
            Wave::Triangle => (1.0 - (phase * 4.0 - 2.0).abs()) as f32,
            Wave::Noise => noise.next(),
        };
        samples.push(value * envelope.at(time, seconds));
    }

    Sound {
        rate,
        channels: 1,
        samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RATE;

    fn plain() -> Envelope {
        // No shaping, so a test sees the waveform itself.
        Envelope {
            attack: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: 0.0,
        }
    }

    #[test]
    fn a_tone_is_as_long_as_it_was_asked_to_be() {
        let sound = tone(Wave::Sine, 440.0, 0.25, Envelope::default(), RATE);
        assert_eq!(sound.frames(), 11_025);
        assert!((sound.seconds() - 0.25).abs() < 1e-6);
        assert_eq!(sound.channels, 1);
    }

    #[test]
    fn a_square_wave_spends_half_its_cycle_at_each_extreme() {
        // 100Hz at 1000 samples a second is ten samples a cycle: five up,
        // five down.
        let sound = tone(Wave::Square, 100.0, 0.02, plain(), 1000);
        assert_eq!(
            &sound.samples[..10],
            &[1.0, 1.0, 1.0, 1.0, 1.0, -1.0, -1.0, -1.0, -1.0, -1.0]
        );
    }

    #[test]
    fn a_sine_wave_completes_its_cycle_where_it_started() {
        // 250Hz at 1000 samples a second is exactly four samples a cycle, so
        // every quarter turn lands on a sample and the expected values are
        // exact rather than approximate. (At 100Hz the quarter falls at
        // sample 2.5 and the peak is between two samples -- which is correct
        // and makes for a much weaker test.)
        let sound = tone(Wave::Sine, 250.0, 0.008, plain(), 1000);
        assert!(sound.samples[0].abs() < 1e-6, "starts at zero");
        assert!((sound.samples[1] - 1.0).abs() < 1e-6, "up a quarter in");
        assert!(sound.samples[2].abs() < 1e-6, "back through zero at half");
        assert!(
            (sound.samples[3] + 1.0).abs() < 1e-6,
            "down at three quarters"
        );
        assert!(sound.samples[4].abs() < 1e-6, "and round again");
        assert!(sound.peak() <= 1.0 + 1e-6, "never leaves the range");
    }

    #[test]
    fn every_waveform_stays_inside_the_range_a_sample_has() {
        // A waveform that escapes [-1, 1] clips the moment it is written to a
        // file, and the arithmetic that produced it is usually wrong rather
        // than merely loud.
        for wave in [
            Wave::Square,
            Wave::Sine,
            Wave::Saw,
            Wave::Triangle,
            Wave::Noise,
        ] {
            let sound = tone(wave, 220.0, 0.05, plain(), RATE);
            assert!(
                sound.peak() <= 1.0 + 1e-6,
                "{wave:?} peaked at {}",
                sound.peak()
            );
            assert!(sound.peak() > 0.5, "{wave:?} is suspiciously quiet");
        }
    }

    #[test]
    fn an_envelope_starts_at_silence_and_ends_there() {
        // The whole point of attack and release: a sound that begins at full
        // volume clicks, and a game firing hundreds of them crackles.
        let sound = tone(Wave::Square, 440.0, 0.2, Envelope::default(), RATE);
        assert_eq!(sound.samples[0], 0.0, "silent at the very start");
        let last = sound.samples[sound.samples.len() - 1];
        assert!(
            last.abs() < 0.02,
            "and nearly silent at the end, got {last}"
        );
        assert!(sound.peak() > 0.9, "but full volume in between");
    }

    #[test]
    fn noise_is_the_same_noise_every_time() {
        // Seeded on purpose: a sound that differed run to run could not be
        // tested, and an explosion does not need to be unique.
        let a = tone(Wave::Noise, 0.0, 0.01, plain(), RATE);
        let b = tone(Wave::Noise, 0.0, 0.01, plain(), RATE);
        assert_eq!(a.samples, b.samples);
        // And it is actually noisy rather than a constant.
        let distinct = a.samples.iter().filter(|s| **s != a.samples[0]).count();
        assert!(distinct > 400, "only {distinct} samples differed");
    }

    #[test]
    fn a_sweep_changes_pitch_without_jumping_in_phase() {
        // Recomputing the phase from time * hertz makes it leap whenever the
        // frequency moves, and a leap in phase is an audible click. The test
        // for "no click" is that no two neighbouring samples are further
        // apart than one cycle of the highest frequency could take them.
        let sound = sweep(Wave::Sine, 200.0, 1800.0, 0.3, plain(), RATE);
        let biggest_step = sound
            .samples
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        let one_cycle = (std::f64::consts::TAU * 1800.0 / RATE as f64) as f32;
        assert!(
            biggest_step < one_cycle * 1.2,
            "a step of {biggest_step} is a click"
        );
    }

    #[test]
    fn a_sweep_really_does_change_pitch() {
        // Counting zero crossings in each half: the second half has to cross
        // far more often than the first, or the frequency never moved.
        let sound = sweep(Wave::Square, 100.0, 2000.0, 0.4, plain(), RATE);
        let crossings = |part: &[f32]| {
            part.windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count()
        };
        // Checked against the arithmetic rather than against a ratio picked
        // by eye. A square wave crosses zero twice a cycle, and over 0.2s the
        // frequency averages 575Hz in the first half and 1525Hz in the
        // second, so the counts must be about 230 and 610.
        let half = sound.samples.len() / 2;
        let early = crossings(&sound.samples[..half]);
        let late = crossings(&sound.samples[half..]);
        assert!(
            (early as i32 - 230).abs() <= 4,
            "first half crossed {early}"
        );
        assert!((late as i32 - 610).abs() <= 4, "second half crossed {late}");
    }

    #[test]
    fn a_sound_of_no_length_is_empty_rather_than_a_panic() {
        assert_eq!(tone(Wave::Sine, 440.0, 0.0, plain(), RATE).frames(), 0);
        assert_eq!(sweep(Wave::Sine, 1.0, 2.0, -1.0, plain(), RATE).frames(), 0);
    }
}
