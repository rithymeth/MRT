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
    looping: bool,
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
    pub fn play(&mut self, sound: Arc<Sound>, volume: f32, looping: bool) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        // A sound recorded at 22050 playing on a 44100 device advances half a
        // frame per output frame, which is what makes it come out at the
        // right pitch instead of an octave high.
        let step = if self.rate == 0 {
            1.0
        } else {
            sound.rate as f64 / self.rate as f64
        };
        self.voices.push(Voice {
            id,
            sound,
            position: 0.0,
            step,
            volume,
            looping,
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
            if frames == 0 {
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
                    *sample += (a + (b - a) * fraction) * voice.volume;
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
        mixer.play(constant(0.5, 4, 1000), 1.0, false);
        let mut out = vec![100.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.5; 4]);
    }

    #[test]
    fn two_voices_add_together() {
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(constant(0.25, 8, 1000), 1.0, false);
        mixer.play(constant(0.25, 8, 1000), 1.0, false);
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
        mixer.play(constant(1.0, 4, 1000), 0.25, false);
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
            mixer.play(constant(0.9, 4, 1000), 1.0, false);
        }
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![1.0; 4], "3.6 became 1.0 rather than distortion");
    }

    #[test]
    fn a_voice_that_runs_out_stops_and_is_forgotten() {
        let mut mixer = Mixer::new(1000, 1);
        mixer.play(constant(1.0, 2, 1000), 1.0, false);
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
        mixer.play(sound, 1.0, true);
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
        mixer.play(sound, 1.0, true);
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
        mixer.play(constant(1.0, 4, 1000), 1.0, false);
        let mut out = vec![0.0; 10];
        mixer.fill(&mut out);
        assert_eq!(&out[..7], &[1.0; 7], "still playing at frame 7");
        assert_eq!(mixer.playing(), 0, "and finished within ten");
    }

    #[test]
    fn a_mono_sound_reaches_both_speakers() {
        let mut mixer = Mixer::new(1000, 2);
        mixer.play(constant(0.5, 4, 1000), 1.0, false);
        let mut out = vec![0.0; 8];
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.5; 8], "both channels, not just the left");
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
            1.0,
            false,
        );
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![1.0, -1.0, 1.0, -1.0]);
    }

    #[test]
    fn stopping_a_voice_silences_only_that_one() {
        let mut mixer = Mixer::new(1000, 1);
        let first = mixer.play(constant(0.5, 8, 1000), 1.0, false);
        mixer.play(constant(0.25, 8, 1000), 1.0, false);
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
        mixer.play(constant(1.0, 1, 1000), 1.0, false); // finishes at once
        mixer.play(constant(0.5, 8, 1000), 1.0, false);
        mixer.play(constant(0.25, 8, 1000), 1.0, false);
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
            1.0,
            true,
        );
        let mut out = vec![0.0; 4];
        mixer.fill(&mut out);
        assert_eq!(out, vec![0.0; 4]);
    }
}
