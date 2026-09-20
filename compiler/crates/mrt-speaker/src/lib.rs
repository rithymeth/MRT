//! The speaker: the one place in this workspace that talks to a sound card.
//!
//! -- Why this is a crate of its own --
//!
//! The same reason `mrt-window` is. Hardware needs platform FFI, and there is
//! no zero-dependency path to it, so the dependency lives at a leaf and
//! `mrt-audio` -- which holds the mixing, the synthesis and the file formats
//! -- stays hermetic and instant to test.
//!
//! The split is sharper here than it was for the window, because almost
//! nothing interesting is in this file. Deciding which voices are playing,
//! where each has got to, what happens when one loops, and how a sound
//! recorded at one rate plays at another is all `mrt-audio::mixer`, tested by
//! filling a buffer and reading the numbers back. What is left here is
//! handing that buffer to the operating system.
//!
//! -- What could not be verified, and should be said plainly --
//!
//! This was written on a machine with no sound device at all -- no `/dev/snd`
//! -- so the error path was exercised and playback was not. Every line that
//! runs once a device exists is unproven by testing; the arithmetic behind it
//! is not. When this first runs somewhere with a speaker, that is where to
//! look if something is wrong.
//!
//! -- A wart worth knowing about --
//!
//! On Linux with no sound card, ALSA's C library writes several lines to
//! stderr ("cannot find card '0'") *before* cpal can return an error. That
//! is libasound talking directly to the terminal, not this code, and there is
//! no way to stop it without calling `snd_lib_error_set_handler` through
//! unsafe FFI -- which this workspace forbids outright. So a program that
//! asks for sound on a machine without any catches a clean error, and the
//! terminal still shows ALSA's complaint above it.
//!
//! -- The lock on the audio thread --
//!
//! A sound card calls back on a thread of its own and does not wait. Sharing
//! the mixer with a mutex is what small engines do and is not ideal: a
//! callback that blocks is a callback that misses its deadline, and the
//! device stutters. So the callback **tries** the lock and fills silence if
//! it cannot take it, which turns an unbounded stall into one quiet block of
//! a few milliseconds. The proper answer is a lock-free queue of commands and
//! a mixer the audio thread owns outright; that is a bigger change than this
//! is worth until something is actually heard glitching.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use mrt_audio::mixer::{Mixer, Play};
use mrt_audio::Sound;

/// A sound card, playing whatever the mixer holds.
pub struct Speaker {
    /// Dropping a stream stops it, so this is held even though nothing reads
    /// it. Naming it `_stream` would say "unused"; it is doing the one thing
    /// that keeps sound coming out.
    stream: cpal::Stream,
    mixer: Arc<Mutex<Mixer>>,
    pub rate: u32,
    pub channels: u16,
}

impl Speaker {
    /// Open the default output, or explain why not.
    ///
    /// "Explain" rather than "panic", for the reason `mrt-window` gives: a
    /// program that asks for sound on a machine without any has to be able to
    /// carry on without it, and this workspace builds release with
    /// `panic = "abort"`, so nothing upstream could soften a panic here.
    pub fn open() -> Result<Speaker, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no sound device is available")?;
        let supported = device
            .default_output_config()
            .map_err(|e| format!("the sound device has no usable format: {e}"))?;

        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate;
        let channels = config.channels;

        // The device decides the rate, not the sounds: resampling each voice
        // as it plays is cheaper and simpler than asking the card for a rate
        // it may not have.
        let mixer = Arc::new(Mutex::new(Mixer::new(rate, channels)));
        let stream = build(&device, &config, format, mixer.clone())?;
        stream
            .play()
            .map_err(|e| format!("could not start playing: {e}"))?;

        Ok(Speaker {
            stream,
            mixer,
            rate,
            channels,
        })
    }

    /// Start a sound, and return the voice it is playing on.
    pub fn play(&self, sound: Arc<Sound>, how: Play) -> u64 {
        match self.mixer.lock() {
            Ok(mut mixer) => mixer.play(sound, how),
            // A poisoned lock means the audio thread panicked mid-callback.
            // Reporting no voice is better than panicking in turn: a game
            // that loses its sound should keep running.
            Err(_) => 0,
        }
    }

    pub fn stop(&self, voice: u64) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.stop(voice);
        }
    }

    /// Ramp a voice's volume to `to` over `seconds`, optionally stopping it
    /// once the fade completes. A poisoned lock or an unknown voice are both
    /// silently harmless, the same as `stop`.
    pub fn fade(&self, voice: u64, to: f32, seconds: f64, stop_at_end: bool) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.fade(voice, to, seconds, stop_at_end);
        }
    }

    /// Move a voice's stereo position live, without restarting it. A
    /// poisoned lock or an unknown voice are both silently harmless, the
    /// same as `fade`.
    pub fn set_pan(&self, voice: u64, pan: f32) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.set_pan(voice, pan);
        }
    }

    pub fn stop_all(&self) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.stop_all();
        }
    }

    pub fn playing(&self) -> usize {
        self.mixer.lock().map(|mixer| mixer.playing()).unwrap_or(0)
    }

    /// Stop the device. Playing again afterwards is silent.
    pub fn pause(&self) {
        let _ = self.stream.pause();
    }

    pub fn resume(&self) {
        let _ = self.stream.play();
    }
}

/// Build the output stream for whichever sample format the device wants.
///
/// Three near-identical arms rather than one generic function, because the
/// closure captures differently in each and the shared part -- `fill_from`
/// below -- is where the actual work is.
fn build(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
    mixer: Arc<Mutex<Mixer>>,
) -> Result<cpal::Stream, String> {
    let complain = |e: cpal::Error| format!("could not open the sound device: {e}");
    let on_error = |e: cpal::Error| eprintln!("mrt: the sound device stopped: {e}");

    match format {
        cpal::SampleFormat::F32 => device
            .build_output_stream(
                *config,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    fill_from(&mixer, out, |sample| sample);
                },
                on_error,
                None,
            )
            .map_err(complain),
        cpal::SampleFormat::I16 => device
            .build_output_stream(
                *config,
                move |out: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    // 32767, not 32768: the positive side of the range is one
                    // shorter, and the larger number wraps the loudest sample
                    // to full negative.
                    fill_from(&mixer, out, |sample| (sample * 32767.0) as i16);
                },
                on_error,
                None,
            )
            .map_err(complain),
        cpal::SampleFormat::U16 => device
            .build_output_stream(
                *config,
                move |out: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    // Unsigned output puts silence at the middle of the range
                    // rather than at zero.
                    fill_from(&mixer, out, |sample| {
                        ((sample * 32767.0) as i32 + 32768) as u16
                    });
                },
                on_error,
                None,
            )
            .map_err(complain),
        other => Err(format!("the sound device wants {other:?} samples")),
    }
}

/// Ask the mixer for a block and convert it to whatever the device takes.
///
/// The `try_lock` is the point: see the note at the top of this file. A block
/// of silence is a few milliseconds; a blocked audio callback is a stutter
/// across the whole device.
fn fill_from<T: Copy>(mixer: &Mutex<Mixer>, out: &mut [T], convert: impl Fn(f32) -> T) {
    let Ok(mut mixer) = mixer.try_lock() else {
        for slot in out.iter_mut() {
            *slot = convert(0.0);
        }
        return;
    };
    // A scratch buffer per callback rather than a kept one, because the
    // device may change the block size between calls and a stale length is a
    // buffer half full of the last block.
    let mut scratch = vec![0.0f32; out.len()];
    mixer.fill(&mut scratch);
    for (slot, sample) in out.iter_mut().zip(scratch) {
        *slot = convert(sample);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_with_no_device_is_an_error_rather_than_a_crash() {
        // The one part of this file testable on a machine with no sound card,
        // and the part that matters most for it: a game asking for audio
        // where there is none must be able to carry on.
        //
        // Where a device *does* exist this opens one, which is also fine --
        // the assertion is that neither outcome is a panic.
        match Speaker::open() {
            Ok(speaker) => {
                assert!(speaker.rate > 0, "a real device has a real rate");
                assert!(speaker.channels > 0);
                assert_eq!(speaker.playing(), 0, "nothing plays until asked");
            }
            Err(why) => {
                assert!(!why.is_empty(), "a refusal has to say something");
            }
        }
    }

    #[test]
    fn the_sample_conversions_land_where_they_should() {
        // The arithmetic inside the three callbacks, which cannot be reached
        // without a device but can be checked on its own.
        let to_i16 = |s: f32| (s * 32767.0) as i16;
        assert_eq!(to_i16(0.0), 0);
        assert_eq!(to_i16(1.0), 32767, "full scale, not a wrap to -32768");
        assert_eq!(to_i16(-1.0), -32767);

        let to_u16 = |s: f32| ((s * 32767.0) as i32 + 32768) as u16;
        assert_eq!(to_u16(0.0), 32768, "silence sits in the middle");
        assert_eq!(to_u16(1.0), 65535);
        assert_eq!(to_u16(-1.0), 1);
    }
}
