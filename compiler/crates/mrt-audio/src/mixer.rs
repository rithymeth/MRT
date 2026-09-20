//! Many sounds at once, filled into an output buffer a frame at a time.
//!
//! -- Why this is here and not in the device crate --
//!
//! A sound card asks for a buffer of samples, repeatedly, on a thread of its
//! own, and it does not wait. Everything interesting happens in answering
//! that question: which voices are still playing, where each one is up to,
//! what happens when one runs out or loops, and how a sound recorded at one
//! rate is played at another.
//!
//! None of that needs a sound card. It is arithmetic on arrays, so it lives
//! in the crate with no dependencies and gets tested by filling a buffer and
//! reading the numbers back -- the same split as `scale_into` in
//! `mrt-window`, for the same reason: what can be checked without hardware
//! should be.
//!
//! What is left for the device crate is handing this buffer to the operating
//! system, which is the part no test can answer anyway.

use std::sync::Arc;

use crate::Sound;

/// How to play a sound.
///
/// A struct rather than four positional arguments, because `play(sound, 0.8,
/// -0.3, 1.05, false)` at a call site says nothing about which number is
/// which -- and because the next thing anyone wants to add goes in here
/// without changing every caller.
#[derive(Clone, Copy, Debug)]
pub struct Play {
    pub volume: f32,
    /// Where it sits between the speakers: -1 left, 0 centre, 1 right.
    pub pan: f32,
    /// A pitch multiplier. 1.0 is as recorded; 1.06 is about a semitone up.
    ///
    /// It changes the length too, because this is resampling rather than
    /// true pitch shifting: playing a sound faster makes it higher *and*
    /// shorter, exactly as a record does. For game effects that is the
    /// wanted behaviour and not an approximation of something better.
    pub speed: f32,
    /// Cut everything above this many hertz, to make a sound distant or
    /// muffled. Zero means no filtering.
    ///
    /// Live rather than baked, because the same footstep is near in one
    /// moment and far in the next, and pre-filtering every distance would
    /// mean a sound per distance.
    pub cutoff: f64,
    pub looping: bool,
}

impl Default for Play {
    fn default() -> Play {
        Play {
            volume: 1.0,
            pan: 0.0,
            speed: 1.0,
            cutoff: 0.0,
            looping: false,
        }
    }
}

/// One playing sound.
struct Voice {
    id: u64,
    sound: Arc<Sound>,
    /// Where playback has reached, in frames of the *source*, fractional
    /// because the source and the output rarely share a sample rate.
    position: f64,
    /// How far the position moves per output frame.
    step: f64,
    volume: f32,
    /// Per-channel gain, already worked out from the pan.
    gains: [f32; 2],
    /// How far the filter moves towards its input each sample; 1.0 is off.
    alpha: f32,
    /// The filter's running value, per channel. Two of them, because one
    /// shared value would fold the stereo sides into each other.
    filtered: [f32; 2],
    looping: bool,
}

/// The gain for each speaker at a pan position, by the constant-power law.
///
/// `cos` and `sin` rather than a straight line between the speakers. Linear
/// panning sums to half the power in the middle, so a sound swept across the
/// stereo field audibly sags as it passes the centre -- the one artefact
/// everyone notices and nobody can name.
///
/// At the centre both gains are about 0.707, whose squares add to one.
fn pan_gains(pan: f32) -> [f32; 2] {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
    [angle.cos(), angle.sin()]
}

/// Everything currently playing, and the format they are being played at.
pub struct Mixer {
    voices: Vec<Voice>,
    next_id: u64,
    pub rate: u32,
    pub channels: u16,
}

impl Mixer {
    pub fn new(rate: u32, channels: u16) -> Mixer {
        Mixer {
            voices: Vec::new(),
            next_id: 1,
            rate: rate.max(1),
            channels: channels.max(1),
        }
    }

    /// Start a sound. The id can be used to stop it again.
    ///
    /// Sounds are `Arc`ed rather than copied: the same effect fired twenty
    /// times in a second is twenty voices reading one buffer, and copying a
    /// second of audio per shot would allocate megabytes in the middle of a
    /// game loop.
    pub fn play(&mut self, sound: Arc<Sound>, how: Play) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        // A sound recorded at 22050 playing on a 44100 device advances half a
        // frame per output frame, which is what makes it come out at the
        // right pitch instead of an octave high. The speed multiplies that.
        let step = if self.rate == 0 {
            1.0
        } else {
            sound.rate as f64 / self.rate as f64 * how.speed.max(0.0) as f64
        };
        self.voices.push(Voice {
            id,
            sound,
            position: 0.0,
            step,
            volume: how.volume,
            gains: pan_gains(how.pan),
            alpha: crate::one_pole_alpha(how.cutoff, self.rate),
            filtered: [0.0; 2],
            looping: how.looping,
        });
        id
    }

    pub fn stop(&mut self, id: u64) {
        self.voices.retain(|voice| voice.id != id);
    }

    pub fn stop_all(&mut self) {
        self.voices.clear();
    }

    pub fn playing(&self) -> usize {
        self.voices.len()
    }

    /// Fill an interleaved output buffer, mixing every voice into it.
    ///
    /// The buffer is **overwritten**, not added to: an audio callback is
    /// handed whatever was in the buffer last time, and adding to it would
    /// play the previous block again underneath this one.
    ///
    /// Samples are clamped on the way out. Everywhere else in this crate a
    /// sum past full scale is left alone so a caller can see it, but nobody
    /// is watching here -- this runs on the device's thread, hundreds of
    /// times a second -- and what a speaker does with an out-of-range sample
    /// is worse than what clamping does.
    pub fn fill(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let channels = self.channels as usize;

        for voice in &mut self.voices {
            let frames = voice.sound.frames();
            // A sound with no samples, or one asked to play at no speed,
            // would sit at the same position for ever and never finish.
            if frames == 0 || voice.step <= 0.0 {
                continue;
            }
            for (frame, slot) in out.chunks_exact_mut(channels).enumerate() {
                let _ = frame;
                if voice.position >= frames as f64 {
                    if !voice.looping {
                        break;
                    }
                    // Wrapped with a remainder rather than reset to zero, so
                    // a loop does not gain a fraction of a frame every pass
                    // and drift out of time with itself.
                    voice.position %= frames as f64;
                }
                let index = voice.position as usize;
                let fraction = (voice.position - index as f64) as f32;

                for (channel, sample) in slot.iter_mut().enumerate() {
                    let source = (channel as u16).min(voice.sound.channels.saturating_sub(1));
                    let a = voice.sound.at(index, source);
                    let b = if index + 1 < frames {
                        voice.sound.at(index + 1, source)
                    } else if voice.looping {
                        voice.sound.at(0, source)
                    } else {
                        // Towards silence rather than holding the last
                        // sample, which would leave a click at the end.
                        0.0
                    };
                    let mut value = a + (b - a) * fraction;

                    // Filter the source, then place it: muffling is a
                    // property of the sound, panning of where it is.
                    if voice.alpha < 1.0 {
                        let slot = channel.min(1);
                        voice.filtered[slot] += voice.alpha * (value - voice.filtered[slot]);
                        value = voice.filtered[slot];
                    }

                    // Panning only means anything with somewhere to pan to:
                    // on a mono device both gains would apply to the one
                    // channel and quieten everything by a third.
                    let gain = if channels == 2 {
                        voice.gains[channel]
                    } else {
                        1.0
                    };
                    *sample += value * voice.volume * gain;
                }
                voice.position += voice.step;
            }
        }

        // Finished voices go after the pass, not during it: removing from the
        // list being iterated is how the voice after a finished one gets
        // skipped for a block.
        self.voices
            .retain(|voice| voice.looping || voice.position < voice.sound.frames() as f64);

        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(value: f32, frames: usize, rate: u32) -> Arc<Sound> {
        Arc::new(Sound {
            rate,
            channels: 1,
            samples: vec![value; frames],
        })
    }

    #[test]
    fn nothing_playing_fills_with_silence() {
        let mut mixer = Mixer::new(1000, 1);
        let mut out = vec![9.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.0; 4]);
    }

    #[test]
    fn the_buffer_is_overwritten_rather_than_added_to() {
        // An audio callback is handed whatever was in the buffer last time.
        // Adding to it plays the previous block again underneath this one --
        // which sounds like an echo that gets worse.
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(constant(0.5, 4, 1000), Play::default());
        let mut out = vec![100.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.5; 4]);
    }

    #[test]
    fn two_voices_add_together() {
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(constant(0.25, 8, 1000), Play::default());
        mixer.play(constant(0.25, 8, 1000), Play::default());
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.5; 4]);
        assert_eq!(mixer.playing(), 2, "both have half their length left");

        // And they carry on from where they stopped rather than restarting:
        // a callback is answered in blocks, and a voice that began again
        // every block would stutter instead of playing.
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.5; 4]);
        assert_eq!(mixer.playing(), 0, "eight frames delivered, so done");
    }

    #[test]
    fn volume_scales_a_voice() {
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(
            constant(1.0, 4, 1000),
            Play {
                volume: 0.25,
                ..Play::default()
            },
        );
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.25; 4]);
    }

    #[test]
    fn the_output_is_clamped_even_though_a_mix_is_not() {
        // Nobody is watching this thread, and what a speaker does with an
        // out-of-range sample is worse than what clamping does.
        let mut mixer = Mixer::new(1000, 1);
        for _ in 0..4 {
            mixer.play(constant(0.9, 4, 1000), Play::default());
        }
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![1.0; 4], "3.6 became 1.0 rather than distortion");
    }

    #[test]
    fn a_voice_that_runs_out_stops_and_is_forgotten() {
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(constant(1.0, 2, 1000), Play::default());
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(&out[..2], &[1.0, 1.0], "it played");
        assert_eq!(&out[2..], &[0.0, 0.0], "then silence, not repetition");
        assert_eq!(mixer.playing(), 0, "and it was dropped");
    }

    #[test]
    fn a_looping_voice_wraps_and_keeps_going() {
        let mut mixer = Mixer::new(1000, 1);
        let sound = Arc::new(Sound {
            rate: 1000,
            channels: 1,
            samples: vec![1.0, -1.0],
        });
        mixer.play(
            sound,
            Play {
                looping: true,
                ..Play::default()
            },
        );
        let mut out = vec![0.0; 6];
        mixer.fill(&mut out);
        assert_eq!(out, vec![1.0, -1.0, 1.0, -1.0, 1.0, -1.0]);
        assert_eq!(mixer.playing(), 1, "a loop never finishes on its own");
    }

    #[test]
    fn a_loop_does_not_drift_out_of_time_with_itself() {
        // Resetting the position to zero on each wrap loses whatever
        // fraction of a frame it had gone past, and the loop gains time --
        // inaudible once and unmistakable over a minute of music.
        let mut mixer = Mixer::new(3000, 1);
        let sound = constant(1.0, 7, 1000); // step of 1/3 per output frame
        mixer.play(
            sound,
            Play {
                looping: true,
                ..Play::default()
            },
        );
        let mut out = vec![0.0; 21 * 5];
        mixer.fill(&mut out);
        // 105 output frames at a third of a source frame each is 35 source
        // frames: exactly five times round a 7-frame sound.
        assert_eq!(mixer.playing(), 1);
    }

    #[test]
    fn a_sound_at_a_different_rate_plays_at_the_right_speed() {
        // Half the device's rate means half a source frame per output frame,
        // so a 4-frame sound lasts 8 output frames. Ignoring this is how a
        // sound ends up an octave high and twice as fast.
        let mut mixer = Mixer::new(2000, 1);
        mixer.play(constant(1.0, 4, 1000), Play::default());
        let mut out = vec![0.0; 10];
        mixer.fill(&mut out);
        assert_eq!(&out[..7], &[1.0; 7], "still playing at frame 7");
        assert_eq!(mixer.playing(), 0, "and finished within ten");
    }

    #[test]
    fn a_mono_sound_reaches_both_speakers() {
        let mut mixer = Mixer::new(1000, 2);
        mixer.play(constant(1.0, 4, 1000), Play::default());
        let mut out = vec![0.0; 8];
        mixer.fill(&mut out);
        // Centred is 0.707 a side, not 1.0 a side: the two together carry
        // the same power as one at full scale, which is what stops a sound
        // jumping in volume the moment it is panned away from the middle.
        for sample in &out {
            assert!((sample - 0.707).abs() < 0.001, "got {sample}");
        }
    }

    #[test]
    fn panning_keeps_the_power_constant_across_the_field() {
        // The point of the cos/sin law. A straight line between the speakers
        // sums to half the power in the middle, so a sound swept across the
        // stereo field sags as it passes centre -- the one panning artefact
        // everyone notices and nobody can name.
        for step in 0..=20 {
            let pan = -1.0 + step as f32 / 10.0;
            let [left, right] = pan_gains(pan);
            let power = left * left + right * right;
            assert!((power - 1.0).abs() < 1e-5, "pan {pan} carried {power}");
        }
        // And the ends really are hard left and hard right.
        let [left, right] = pan_gains(-1.0);
        assert!((left - 1.0).abs() < 1e-6 && right.abs() < 1e-6);
        let [left, right] = pan_gains(1.0);
        assert!(left.abs() < 1e-6 && (right - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_pan_past_the_speakers_stops_at_them() {
        assert_eq!(pan_gains(-9.0), pan_gains(-1.0));
        assert_eq!(pan_gains(9.0), pan_gains(1.0));
    }

    #[test]
    fn panning_a_voice_moves_it() {
        let mut mixer = Mixer::new(1000, 2);
        mixer.play(
            constant(1.0, 4, 1000),
            Play {
                pan: -1.0,
                ..Play::default()
            },
        );
        let mut out = vec![0.0; 8];
        mixer.fill(&mut out);
        assert!((out[0] - 1.0).abs() < 1e-5, "all of it on the left");
        assert!(out[1].abs() < 1e-5, "and none on the right");
    }

    #[test]
    fn panning_does_nothing_on_a_device_with_one_speaker() {
        // Both gains would otherwise apply to the same channel and quieten
        // everything by a third on mono hardware.
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(
            constant(1.0, 4, 1000),
            Play {
                pan: 0.5,
                ..Play::default()
            },
        );
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![1.0; 4]);
    }

    #[test]
    fn speed_changes_the_pitch_and_the_length_together() {
        // This is resampling, not true pitch shifting: faster is higher and
        // shorter, exactly as a record is. A four-frame sound at double
        // speed is gone in two.
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(
            constant(1.0, 4, 1000),
            Play {
                speed: 2.0,
                ..Play::default()
            },
        );
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(&out[..2], &[1.0, 1.0], "two frames of sound");
        assert_eq!(mixer.playing(), 0, "then finished, at twice the rate");
    }

    #[test]
    fn a_cutoff_dulls_a_voice_as_it_plays() {
        // The live filter and the offline one share their arithmetic, so a
        // sound muffled once and a sound muffled while playing are the same
        // sound. Checked here by playing a square wave, whose corners are
        // entirely high frequency: filtered, it can no longer reach the
        // extremes on the first sample.
        let square = Arc::new(Sound {
            rate: 1000,
            channels: 1,
            samples: vec![1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0],
        });

        let mut plain = Mixer::new(1000, 1);
        plain.play(square.clone(), Play::default());
        let mut sharp = vec![0.0; 8];
        plain.fill(&mut sharp);

        let mut muffled = Mixer::new(1000, 1);
        muffled.play(
            square,
            Play {
                cutoff: 60.0,
                ..Play::default()
            },
        );
        let mut soft = vec![0.0; 8];
        muffled.fill(&mut soft);

        assert_eq!(sharp[0], 1.0, "unfiltered, it starts at full scale");
        assert!(soft[0] < 0.5, "filtered, it has to climb, got {}", soft[0]);
        let sharp_energy: f32 = sharp.iter().map(|s| s * s).sum();
        let soft_energy: f32 = soft.iter().map(|s| s * s).sum();
        assert!(soft_energy < sharp_energy * 0.5, "and it lost its edge");
    }

    #[test]
    fn a_cutoff_of_zero_leaves_a_voice_alone() {
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(constant(1.0, 4, 1000), Play::default());
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![1.0; 4], "no filter means no change at all");
    }

    #[test]
    fn a_speed_of_zero_is_dropped_rather_than_stuck() {
        // A voice that never advances would sit at frame zero for ever,
        // holding one sample down like a stuck key.
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(
            constant(1.0, 4, 1000),
            Play {
                speed: 0.0,
                ..Play::default()
            },
        );
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.0; 4], "silent rather than a held tone");
    }

    #[test]
    fn a_stereo_sound_keeps_its_sides() {
        let mut mixer = Mixer::new(1000, 2);
        mixer.play(
            Arc::new(Sound {
                rate: 1000,
                channels: 2,
                samples: vec![1.0, -1.0, 1.0, -1.0],
            }),
            Play::default(),
        );
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        // Centred, so each side keeps its own signal at the centre gain
        // rather than being folded into the other.
        assert!((out[0] - 0.707).abs() < 0.001);
        assert!((out[1] + 0.707).abs() < 0.001);
    }

    #[test]
    fn stopping_a_voice_silences_only_that_one() {
        let mut mixer = Mixer::new(1000, 1);
        let first = mixer.play(constant(0.5, 8, 1000), Play::default());
        mixer.play(constant(0.25, 8, 1000), Play::default());
        mixer.stop(first);
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.25; 4]);
        assert_eq!(mixer.playing(), 1);

        mixer.stop_all();
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.0; 4]);
    }

    #[test]
    fn stopping_an_id_that_is_not_playing_is_harmless() {
        let mut mixer = Mixer::new(1000, 1);
        mixer.stop(999);
        assert_eq!(mixer.playing(), 0);
    }

    #[test]
    fn every_voice_after_a_finished_one_still_plays() {
        // Removing from the list while iterating it skips the voice that
        // moved into the gap -- for one block, so it sounds like an
        // occasional dropout rather than a bug.
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(constant(1.0, 1, 1000), Play::default()); // finishes at once
        mixer.play(constant(0.5, 8, 1000), Play::default());
        mixer.play(constant(0.25, 8, 1000), Play::default());
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out[1], 0.75, "both survivors were heard");
        assert_eq!(mixer.playing(), 2);
    }

    #[test]
    fn an_empty_sound_does_not_wedge_the_mixer() {
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(
            Arc::new(Sound {
                rate: 1000,
                channels: 1,
                samples: vec![],
            }),
            Play {
                looping: true,
                ..Play::default()
            },
        );
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.0; 4]);
    }
}
