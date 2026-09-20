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

## Roadmap

Done: sounds, synthesis, mixing, resampling, and WAV both ways.

Next: playback. That needs a device, and therefore a dependency, confined to
a leaf crate the way `mrt-window` confines the windowing one.
