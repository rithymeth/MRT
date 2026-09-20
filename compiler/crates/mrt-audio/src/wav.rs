//! Reading and writing `.wav`.
//!
//! WAV is the audio equivalent of the stored-block PNG in `mrt-game`: a
//! header and then the samples, with no compression to implement. That makes
//! it the one audio format worth supporting without a dependency, and it is
//! what every tool can open.
//!
//! Writing is 16-bit, which is what a file is for. Reading takes the formats
//! an editor actually produces -- 8, 16, 24 and 32-bit integers, and 32-bit
//! floats -- because a program that loads only its own output is not loading.

use crate::Sound;

/// Write 16-bit PCM.
pub fn encode(sound: &Sound) -> Vec<u8> {
    let channels = sound.channels.max(1);
    let rate = if sound.rate == 0 {
        crate::RATE
    } else {
        sound.rate
    };
    let bytes_per_frame = channels as u32 * 2;
    let data_length = sound.samples.len() as u32 * 2;

    let mut out = Vec::with_capacity(44 + data_length as usize);
    out.extend_from_slice(b"RIFF");
    // The size of everything after this field.
    out.extend_from_slice(&(36 + data_length).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // the fmt chunk's size
    out.extend_from_slice(&1u16.to_le_bytes()); // 1 is integer PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * bytes_per_frame).to_le_bytes());
    out.extend_from_slice(&(bytes_per_frame as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_length.to_le_bytes());

    for sample in &sound.samples {
        // Clamped here and nowhere earlier: mixing deliberately allows sums
        // past full scale so a caller can see and fix them, but a 16-bit
        // integer has no room for the overflow and wrapping it would turn a
        // loud sound into a burst of noise.
        let clamped = sample.clamp(-1.0, 1.0);
        // 32767 rather than 32768: the positive side of a signed 16-bit range
        // is one shorter than the negative one, and scaling by the larger
        // number wraps the loudest samples to full negative.
        out.extend_from_slice(&((clamped * 32767.0).round() as i16).to_le_bytes());
    }
    out
}

/// Read a WAV file.
pub fn decode(bytes: &[u8]) -> Result<Sound, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".to_string());
    }

    let mut format: Option<Format> = None;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let kind = &bytes[at..at + 4];
        let length = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let from = at + 8;
        let body = bytes
            .get(from..from + length)
            .ok_or("a chunk runs past the end of the file")?;

        match kind {
            b"fmt " => format = Some(Format::read(body)?),
            b"data" => {
                let format = format.ok_or("the data comes before the format")?;
                return format.samples(body);
            }
            _ => {}
        }
        // Chunks are padded to an even length, and the pad byte is not
        // counted in the size -- miss it and every later chunk is misread.
        at = from + length + (length % 2);
    }
    Err("the WAV file has no data".to_string())
}

struct Format {
    channels: u16,
    rate: u32,
    bits: u16,
    floating: bool,
}

impl Format {
    fn read(body: &[u8]) -> Result<Format, String> {
        if body.len() < 16 {
            return Err("the format chunk is too short".to_string());
        }
        let tag = u16::from_le_bytes(body[0..2].try_into().unwrap());
        let channels = u16::from_le_bytes(body[2..4].try_into().unwrap());
        let rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
        let bits = u16::from_le_bytes(body[14..16].try_into().unwrap());

        // 0xFFFE is "extensible", which carries the real format in a GUID at
        // the end of the chunk; its first two bytes are the ordinary tag.
        let tag = if tag == 0xFFFE {
            if body.len() < 26 {
                return Err("an extensible format chunk with no subformat".to_string());
            }
            u16::from_le_bytes(body[24..26].try_into().unwrap())
        } else {
            tag
        };

        let floating = match tag {
            1 => false,
            3 => true,
            other => return Err(format!("unsupported WAV format {other}; only PCM is read")),
        };
        if channels == 0 {
            return Err("a WAV with no channels".to_string());
        }
        if floating && bits != 32 {
            return Err(format!("{bits}-bit floating point is not a thing"));
        }
        if !floating && !matches!(bits, 8 | 16 | 24 | 32) {
            return Err(format!("unsupported sample size of {bits} bits"));
        }
        Ok(Format {
            channels,
            rate,
            bits,
            floating,
        })
    }

    fn samples(&self, data: &[u8]) -> Result<Sound, String> {
        let width = self.bits as usize / 8;
        let samples = data
            .chunks_exact(width)
            .map(|bytes| {
                if self.floating {
                    f32::from_le_bytes(bytes.try_into().expect("four bytes"))
                } else {
                    match self.bits {
                        // 8-bit WAV is *unsigned*, with 128 as silence. Every
                        // other width is signed. Reading one as the other
                        // gives a sound that is inaudible and offset.
                        8 => (bytes[0] as f32 - 128.0) / 128.0,
                        16 => i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32768.0,
                        24 => {
                            // Sign-extended by putting the three bytes in the
                            // top of an i32 and shifting back down.
                            let value = i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) >> 8;
                            value as f32 / 8_388_608.0
                        }
                        _ => {
                            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32
                                / 2_147_483_648.0
                        }
                    }
                }
            })
            .collect();

        Ok(Sound {
            rate: if self.rate == 0 {
                crate::RATE
            } else {
                self.rate
            },
            channels: self.channels,
            samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * bits as u32 / 8).to_le_bytes());
        out.extend_from_slice(&(channels * bits / 8).to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn what_this_writes_it_reads_back() {
        let sound = Sound {
            rate: 22_050,
            channels: 2,
            samples: vec![0.0, 0.5, -0.5, 1.0, -1.0, 0.25],
        };
        let decoded = decode(&encode(&sound)).expect("decodes");
        assert_eq!(decoded.rate, 22_050);
        assert_eq!(decoded.channels, 2);
        for (from, to) in sound.samples.iter().zip(&decoded.samples) {
            assert!((from - to).abs() < 1e-4, "{from} became {to}");
        }
    }

    #[test]
    fn the_loudest_sample_does_not_wrap_to_its_opposite() {
        // Scaling by 32768 rather than 32767 sends 1.0 to -32768, so the
        // loudest moment of a sound becomes the quietest. It is inaudible on
        // a sine and unmistakable on anything square.
        let sound = Sound {
            rate: 8000,
            channels: 1,
            samples: vec![1.0, -1.0],
        };
        let bytes = encode(&sound);
        let data = &bytes[44..];
        assert_eq!(i16::from_le_bytes([data[0], data[1]]), 32767);
        assert_eq!(i16::from_le_bytes([data[2], data[3]]), -32767);
    }

    #[test]
    fn samples_past_full_scale_are_clamped_rather_than_wrapped() {
        // Mixing allows sums past 1.0 on purpose; a 16-bit integer has no
        // room for them, and wrapping turns a loud sound into a burst.
        let loud = Sound {
            rate: 8000,
            channels: 1,
            samples: vec![1.8, -2.5],
        };
        let bytes = encode(&loud);
        let data = &bytes[44..];
        assert_eq!(i16::from_le_bytes([data[0], data[1]]), 32767);
        assert_eq!(i16::from_le_bytes([data[2], data[3]]), -32767);
    }

    #[test]
    fn eight_bit_audio_is_unsigned_with_silence_at_128() {
        // The one width that is not signed. Reading it as signed gives a
        // sound that is quiet and offset rather than obviously wrong.
        let file = wav(1, 1, 8000, 8, &[128, 255, 0, 192]);
        let sound = decode(&file).expect("decodes");
        assert!(sound.samples[0].abs() < 1e-6, "128 is silence");
        assert!((sound.samples[1] - 0.9921875).abs() < 1e-6, "255 is loud");
        assert!((sound.samples[2] + 1.0).abs() < 1e-6, "0 is the other way");
        assert!((sound.samples[3] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn twenty_four_bit_samples_keep_their_sign() {
        // Three bytes have no integer type, so they are sign-extended by
        // hand; getting that wrong makes every negative sample enormous.
        let file = wav(1, 1, 8000, 24, &[0, 0, 0x40, 0, 0, 0xC0]);
        let sound = decode(&file).expect("decodes");
        assert!((sound.samples[0] - 0.5).abs() < 1e-5, "half scale up");
        assert!((sound.samples[1] + 0.5).abs() < 1e-5, "half scale down");
    }

    #[test]
    fn floating_point_wavs_are_read_as_they_are() {
        let mut data = Vec::new();
        for value in [0.25f32, -0.75] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        let sound = decode(&wav(3, 1, 44100, 32, &data)).expect("decodes");
        assert_eq!(sound.samples, vec![0.25, -0.75]);
    }

    #[test]
    fn an_extensible_header_is_followed_to_its_real_format() {
        // 0xFFFE carries the actual format in a GUID; a reader that stopped
        // at the tag would reject files plenty of editors write.
        let mut body = Vec::new();
        body.extend_from_slice(&0xFFFEu16.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes()); // channels
        body.extend_from_slice(&8000u32.to_le_bytes());
        body.extend_from_slice(&16000u32.to_le_bytes());
        body.extend_from_slice(&2u16.to_le_bytes());
        body.extend_from_slice(&16u16.to_le_bytes());
        body.extend_from_slice(&22u16.to_le_bytes()); // extension size
        body.extend_from_slice(&16u16.to_le_bytes()); // valid bits
        body.extend_from_slice(&0u32.to_le_bytes()); // channel mask
        body.extend_from_slice(&1u16.to_le_bytes()); // the real tag: PCM
        body.extend_from_slice(&[0; 14]);

        let mut file = Vec::new();
        file.extend_from_slice(b"RIFF");
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(b"WAVEfmt ");
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file.extend_from_slice(b"data");
        file.extend_from_slice(&4u32.to_le_bytes());
        file.extend_from_slice(&[0, 0, 0xFF, 0x7F]);

        let sound = decode(&file).expect("decodes");
        assert_eq!(sound.channels, 1);
        assert!((sound.samples[1] - 0.99997).abs() < 1e-4);
    }

    #[test]
    fn an_odd_chunk_is_padded_and_the_next_one_still_found() {
        // Chunk sizes exclude their pad byte. Missing it misreads every
        // chunk after the first odd one, which in practice means files with
        // a LIST or metadata chunk fail and files without it work.
        let mut file = Vec::new();
        file.extend_from_slice(b"RIFF");
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(b"WAVE");
        // An odd-length chunk nobody reads, plus its pad.
        file.extend_from_slice(b"LIST");
        file.extend_from_slice(&3u32.to_le_bytes());
        file.extend_from_slice(b"abc\0");
        let rest = wav(1, 1, 8000, 16, &[0, 0x40]);
        file.extend_from_slice(&rest[12..]);

        let sound = decode(&file).expect("decodes past the odd chunk");
        assert_eq!(sound.channels, 1);
        assert_eq!(sound.samples.len(), 1);
    }

    #[test]
    fn a_file_that_is_not_a_wav_says_so() {
        assert!(decode(b"").is_err());
        assert!(decode(b"not a wav at all").is_err());
        assert!(decode(&wav(2, 1, 8000, 16, &[0, 0])).is_err(), "ADPCM");
        assert!(decode(&wav(1, 1, 8000, 12, &[0, 0])).is_err(), "12-bit");
        assert!(
            decode(&wav(1, 0, 8000, 16, &[0, 0])).is_err(),
            "no channels"
        );
    }

    #[test]
    fn a_truncated_wav_is_an_error_rather_than_a_panic() {
        let file = wav(1, 1, 8000, 16, &[0, 0x40]);
        for cut in [4, 12, 20, 40] {
            assert!(decode(&file[..cut]).is_err(), "cut at {cut}");
        }
    }
}
