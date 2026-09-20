//! Sound: samples, and what you can do to them.
//!
//! -- The same split as the graphics side --
//!
//! `mrt-game` holds the drawing and knows nothing about windows, because
//! everything a 2D game draws is arithmetic on an array and arithmetic can be
//! checked on a machine with no screen. Sound is the same shape of problem:
//! a mix is arithmetic on an array of samples, and whether it is *correct* is
//! a question about numbers, not about whether a speaker moved.
//!
//! So nothing here opens a device. What it does instead is produce sounds,
//! combine them, and write them to a `.wav` a person can play -- which is the
//! audio equivalent of saving a PNG, and stays useful after a device exists
//! because a test can compare samples and cannot listen.
//!
//! -- Samples --
//!
//! A sample is an `f32` in [-1, 1], and channels are interleaved: frame 0's
//! left, frame 0's right, frame 1's left, and so on. Floats rather than the
//! 16-bit integers a file stores, because mixing sums signals and sums clip:
//! doing the arithmetic in a format that cannot overflow and converting once
//! at the end is the difference between a loud mix and a distorted one.

pub mod synth;
pub mod wav;

/// A piece of audio.
#[derive(Clone, PartialEq)]
pub struct Sound {
    /// Frames per second. 44100 unless something says otherwise.
    pub rate: u32,
    pub channels: u16,
    /// Interleaved samples, in [-1, 1].
    pub samples: Vec<f32>,
}

impl std::fmt::Debug for Sound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Sound({} frames, {}ch, {}Hz)",
            self.frames(),
            self.channels,
            self.rate
        )
    }
}

/// What a sound is at unless something says otherwise.
pub const RATE: u32 = 44_100;

impl Sound {
    pub fn silence(rate: u32, channels: u16, frames: usize) -> Sound {
        Sound {
            rate,
            channels,
            samples: vec![0.0; frames * channels as usize],
        }
    }

    /// How many frames -- that is, moments in time, whatever the channel count.
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / self.channels as usize
        }
    }

    pub fn seconds(&self) -> f64 {
        if self.rate == 0 {
            0.0
        } else {
            self.frames() as f64 / self.rate as f64
        }
    }

    /// One frame's sample on one channel.
    pub fn at(&self, frame: usize, channel: u16) -> f32 {
        let index = frame * self.channels as usize + channel as usize;
        self.samples.get(index).copied().unwrap_or(0.0)
    }

    /// Scale every sample.
    pub fn gain(&self, amount: f32) -> Sound {
        Sound {
            rate: self.rate,
            channels: self.channels,
            samples: self.samples.iter().map(|s| s * amount).collect(),
        }
    }

    /// Change the sample rate, interpolating between neighbours.
    ///
    /// Linear rather than anything cleverer. A proper resampler filters first
    /// to keep frequencies above the new rate's limit from folding back down
    /// as a whine; linear interpolation does some of that by accident and not
    /// all of it. For game sound at close to the source rate the difference is
    /// inaudible, and saying so here beats implying a windowed sinc is hiding
    /// somewhere in this file.
    pub fn resample(&self, rate: u32) -> Sound {
        if rate == self.rate || self.rate == 0 || self.frames() == 0 {
            return Sound {
                rate,
                ..self.clone()
            };
        }
        let ratio = self.rate as f64 / rate as f64;
        let frames = (self.frames() as f64 / ratio).round() as usize;
        let mut samples = Vec::with_capacity(frames * self.channels as usize);
        for frame in 0..frames {
            let position = frame as f64 * ratio;
            let left = position.floor() as usize;
            let fraction = (position - left as f64) as f32;
            for channel in 0..self.channels {
                let a = self.at(left, channel);
                let b = self.at(left + 1, channel);
                samples.push(a + (b - a) * fraction);
            }
        }
        Sound {
            rate,
            channels: self.channels,
            samples,
        }
    }

    /// Fold to one channel, or spread to two.
    pub fn to_channels(&self, channels: u16) -> Sound {
        if channels == self.channels || channels == 0 || self.channels == 0 {
            return self.clone();
        }
        let frames = self.frames();
        let mut samples = Vec::with_capacity(frames * channels as usize);
        for frame in 0..frames {
            // Down to mono is the average of what was there, so a stereo
            // sound does not double in volume on the way down.
            let average: f32 =
                (0..self.channels).map(|c| self.at(frame, c)).sum::<f32>() / self.channels as f32;
            for channel in 0..channels {
                samples.push(if channels >= self.channels {
                    self.at(frame, channel.min(self.channels - 1))
                } else {
                    average
                });
            }
        }
        Sound {
            rate: self.rate,
            channels,
            samples,
        }
    }

    /// One sound after another.
    pub fn then(&self, next: &Sound) -> Sound {
        let next = next.resample(self.rate).to_channels(self.channels);
        let mut samples = self.samples.clone();
        samples.extend_from_slice(&next.samples);
        Sound {
            rate: self.rate,
            channels: self.channels,
            samples,
        }
    }

    /// The loudest sample, which is what tells you whether a mix will clip.
    pub fn peak(&self) -> f32 {
        self.samples
            .iter()
            .fold(0.0f32, |most, s| most.max(s.abs()))
    }

    /// Scale so the loudest sample sits at `target`.
    ///
    /// Silence is returned unchanged rather than divided by its own zero
    /// peak, which would be an infinity that spreads through everything it
    /// touches.
    pub fn normalized(&self, target: f32) -> Sound {
        let peak = self.peak();
        if peak == 0.0 {
            return self.clone();
        }
        self.gain(target / peak)
    }
}

/// Lay sounds on top of each other, each at its own volume.
///
/// The result is as long as the longest of them, at the first one's rate and
/// channel count -- everything else is converted to match, because summing
/// samples that mean different moments in time produces noise rather than a
/// mix.
///
/// Sums are **not** clamped here. Two sounds at full volume add to twice full
/// volume, and clamping would flatten the peaks into distortion silently;
/// `peak` reports it and `normalized` fixes it, which leaves the choice with
/// the caller instead of making it badly on their behalf.
pub fn mix(parts: &[(Sound, f32)]) -> Sound {
    let Some((first, _)) = parts.first() else {
        return Sound::silence(RATE, 1, 0);
    };
    let rate = first.rate;
    let channels = first.channels;

    let converted: Vec<(Sound, f32)> = parts
        .iter()
        .map(|(sound, volume)| (sound.resample(rate).to_channels(channels), *volume))
        .collect();
    let frames = converted
        .iter()
        .map(|(sound, _)| sound.frames())
        .max()
        .unwrap_or(0);

    let mut out = Sound::silence(rate, channels, frames);
    for (sound, volume) in &converted {
        for frame in 0..sound.frames() {
            for channel in 0..channels {
                let index = frame * channels as usize + channel as usize;
                out.samples[index] += sound.at(frame, channel) * volume;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(frames: usize) -> Sound {
        Sound {
            rate: 1000,
            channels: 1,
            samples: (0..frames).map(|n| n as f32 / frames as f32).collect(),
        }
    }

    #[test]
    fn a_sound_knows_how_long_it_is() {
        let sound = Sound::silence(8000, 2, 4000);
        assert_eq!(sound.frames(), 4000, "frames are moments, not samples");
        assert_eq!(sound.samples.len(), 8000, "two channels interleaved");
        assert!((sound.seconds() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn mixing_adds_rather_than_averages() {
        // Two identical sounds at full volume are twice as loud. Averaging
        // instead would make every added layer quieten the ones already
        // there, which is not what laying sounds on top of each other means.
        let one = Sound {
            rate: 1000,
            channels: 1,
            samples: vec![0.25, 0.5],
        };
        let mixed = mix(&[(one.clone(), 1.0), (one.clone(), 1.0)]);
        assert_eq!(mixed.samples, vec![0.5, 1.0]);
    }

    #[test]
    fn a_mix_is_as_long_as_its_longest_part() {
        let short = Sound {
            rate: 1000,
            channels: 1,
            samples: vec![1.0],
        };
        let long = Sound {
            rate: 1000,
            channels: 1,
            samples: vec![0.1, 0.2, 0.3],
        };
        let mixed = mix(&[(short, 1.0), (long, 1.0)]);
        assert_eq!(mixed.samples, vec![1.1, 0.2, 0.3], "the short one stops");
    }

    #[test]
    fn a_mix_is_not_clamped_but_reports_that_it_would_clip() {
        // Silently flattening the peaks is distortion the caller never asked
        // for; telling them is what lets them choose.
        let loud = Sound {
            rate: 1000,
            channels: 1,
            samples: vec![0.8, -0.8],
        };
        let mixed = mix(&[(loud.clone(), 1.0), (loud, 1.0)]);
        assert_eq!(mixed.samples, vec![1.6, -1.6]);
        assert!((mixed.peak() - 1.6).abs() < 1e-6);
        assert!((mixed.normalized(1.0).peak() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalizing_silence_does_not_divide_by_it() {
        let quiet = Sound::silence(1000, 1, 4);
        assert_eq!(quiet.normalized(1.0).samples, vec![0.0; 4]);
        assert_eq!(quiet.peak(), 0.0);
    }

    #[test]
    fn mixing_converts_parts_to_the_first_ones_format() {
        // Summing samples that mean different moments in time is noise, not a
        // mix, so the rate and channel count have to be reconciled first.
        let base = Sound {
            rate: 2000,
            channels: 2,
            samples: vec![0.0; 8],
        };
        let other = Sound {
            rate: 1000,
            channels: 1,
            samples: vec![1.0, 1.0],
        };
        let mixed = mix(&[(base, 1.0), (other, 1.0)]);
        assert_eq!(mixed.rate, 2000);
        assert_eq!(mixed.channels, 2);
    }

    #[test]
    fn resampling_keeps_the_sound_the_same_length_in_time() {
        let sound = ramp(100);
        let faster = sound.resample(2000);
        assert_eq!(faster.rate, 2000);
        assert_eq!(faster.frames(), 200, "twice the frames for twice the rate");
        assert!((faster.seconds() - sound.seconds()).abs() < 1e-3);
        // And the shape survives: the middle is still about halfway up.
        assert!((faster.at(100, 0) - 0.5).abs() < 0.02);
    }

    #[test]
    fn resampling_to_the_rate_it_already_has_changes_nothing() {
        let sound = ramp(50);
        assert_eq!(sound.resample(1000).samples, sound.samples);
    }

    #[test]
    fn stereo_folds_down_by_averaging_rather_than_summing() {
        // Summing would make every sound jump in volume the moment it was
        // played on a mono device.
        let stereo = Sound {
            rate: 1000,
            channels: 2,
            samples: vec![1.0, 0.0, 0.5, 0.5],
        };
        let mono = stereo.to_channels(1);
        assert_eq!(mono.samples, vec![0.5, 0.5]);
        assert_eq!(mono.channels, 1);
    }

    #[test]
    fn mono_spreads_to_both_speakers() {
        let mono = Sound {
            rate: 1000,
            channels: 1,
            samples: vec![0.3, -0.4],
        };
        assert_eq!(mono.to_channels(2).samples, vec![0.3, 0.3, -0.4, -0.4]);
    }

    #[test]
    fn one_sound_after_another_adds_their_lengths() {
        let a = ramp(10);
        let b = ramp(5);
        assert_eq!(a.then(&b).frames(), 15);
    }

    #[test]
    fn mixing_nothing_is_silence_rather_than_a_panic() {
        assert_eq!(mix(&[]).frames(), 0);
    }
}
