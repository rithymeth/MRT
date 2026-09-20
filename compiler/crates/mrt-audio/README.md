# MRT-Audio

Sound for MRT: samples, and what you can do to them.

Like `mrt-game`, it has **no dependencies and opens no device**. Everything a
game's audio needs before it reaches a speaker is arithmetic on an array of
samples — and whether a mix is *correct* is a question about numbers, which
can be answered on a build machine with no sound card, which is exactly what
this one is.

## What is here

`lib.rs` is `Sound`: a rate, a channel count, and interleaved `f32` samples in
[-1, 1]. Floats rather than the 16-bit integers a file stores, because mixing
sums signals and sums clip — doing the arithmetic in a format that cannot
overflow and converting once at the end is the difference between a loud mix
and a distorted one. Mixing, resampling, channel folding, gain and
normalisation live here.

`synth.rs` makes sounds from nothing: square, sine, saw, triangle and noise,
with an ADSR envelope and a pitch sweep. The same reasoning as drawing shapes
rather than only loading sprites — a game that needs an artist before it can
make a noise is a game nobody starts. A jump, a coin and a hurt noise are a
few hundred samples of a square wave with an envelope on them.

`wav.rs` reads and writes `.wav`. Writing is 16-bit; reading takes 8, 16, 24
and 32-bit integers and 32-bit floats, including the extensible header, because
a program that loads only its own output is not loading.

## Details that are easy to get wrong

- **8-bit WAV is unsigned**, with 128 as silence. Every other width is signed.
- **Scale by 32767, not 32768.** A signed 16-bit range is one shorter on the
  positive side, and the larger number wraps the loudest sample to full
  negative — inaudible on a sine, unmistakable on anything square.
- **Chunk sizes exclude their pad byte.** Miss it and every chunk after the
  first odd one is misread, so files with metadata fail and files without it
  work.
- **Sweeps accumulate phase** rather than recomputing it from `time * hertz`,
  which jumps whenever the frequency moves — and a jump in phase is a click.

`mixer.rs` answers the question a sound card asks: given these voices, fill
this buffer. Which are still playing, where each has got to, what happens when
one loops, how a sound recorded at one rate plays at another — all of it is
arithmetic, so all of it is here and tested by filling a buffer and reading
the numbers back. `mrt-speaker` only hands that buffer to the operating
system.

## Roadmap

Done: sounds, synthesis, mixing, resampling, voices, and WAV both ways.
Playback lives in `mrt-speaker`.

Voices carry a volume, a pan and a speed. Panning uses the constant-power law
rather than a straight line between the speakers, so a sound swept across the
field does not sag as it passes the centre. Speed is a resample, so it changes
the length as well as the pitch — which is what makes a handful of pitches on
one sample sound like a scale instead of a machine.

A one-pole low-pass filter is available both ways: baked into a sound with
`Sound::low_pass`, or applied live per voice. They share their arithmetic, so
a sound muffled once and a sound muffled while playing are the same sound.
Down 3dB at the cutoff, 6dB an octave after it — it dulls rather than removes,
which is the shape a game wants.

Next: a ring buffer so the audio thread owns its mixer outright, if anything
is ever heard glitching under the current lock.
