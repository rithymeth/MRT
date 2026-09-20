//! DEFLATE decompression, in a page, with no dependencies.
//!
//! -- Why this is needed when `png.rs` exists --
//!
//! `png.rs` *writes* PNGs using deflate's stored-block escape hatch: a
//! length, then the bytes. That is enough to produce a valid file and it
//! needs no compression at all.
//!
//! Reading is not symmetrical. A PNG that came from anywhere else --
//! Aseprite, GIMP, a phone -- is compressed with Huffman coding, and a
//! decoder that only understood stored blocks would reject every real image
//! in the world. So the whole format has to be here: stored blocks, the fixed
//! Huffman tables, and dynamic Huffman with its code lengths encoded in a
//! Huffman code of their own.
//!
//! The implementation follows the structure of zlib's own reference decoder,
//! which decodes canonically -- walking the code lengths one bit at a time --
//! rather than building a lookup table. It is slower per byte and very much
//! shorter, which is the right trade for a file read once at startup.

/// Reads bits least-significant first, as DEFLATE stores them.
///
/// The ordering is worth stating because it is the opposite of the *other*
/// ordering in the same file: deflate's bit fields are LSB-first, while the
/// Huffman codes packed into them are MSB-first. Mixing the two up produces a
/// decoder that works on stored blocks and fails on everything else.
struct Bits<'a> {
    bytes: &'a [u8],
    /// Which bit comes next, counted across the whole input.
    at: usize,
}

impl<'a> Bits<'a> {
    fn new(bytes: &'a [u8]) -> Bits<'a> {
        Bits { bytes, at: 0 }
    }

    fn bit(&mut self) -> Result<u32, String> {
        let byte = self
            .bytes
            .get(self.at / 8)
            .ok_or("the compressed data ended early")?;
        let bit = (byte >> (self.at % 8)) & 1;
        self.at += 1;
        Ok(bit as u32)
    }

    /// `count` bits as a number, first bit read being the least significant.
    fn take(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0;
        for n in 0..count {
            value |= self.bit()? << n;
        }
        Ok(value)
    }

    fn align(&mut self) {
        self.at = self.at.div_ceil(8) * 8;
    }

    fn byte_position(&self) -> usize {
        self.at / 8
    }
}

/// A canonical Huffman code, as its code lengths describe it.
struct Huffman {
    /// How many codes there are of each length, 1 to 15.
    counts: [u16; 16],
    /// The symbols, ordered by code length and then by symbol.
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Huffman {
        let mut counts = [0u16; 16];
        for &length in lengths {
            counts[length as usize] += 1;
        }
        // Length 0 means "this symbol is not in the code at all".
        counts[0] = 0;

        let mut offsets = [0u16; 16];
        for length in 1..15 {
            offsets[length + 1] = offsets[length] + counts[length];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                symbols[offsets[length as usize] as usize] = symbol as u16;
                offsets[length as usize] += 1;
            }
        }
        Huffman { counts, symbols }
    }

    /// Read one symbol.
    ///
    /// Canonical codes are ordered, so a code of a given length can be
    /// compared against the first code of that length without a table: read a
    /// bit, and if the value so far falls inside this length's range, the
    /// symbol is at that offset. Codes here are packed most-significant bit
    /// first, which is why the accumulator shifts left while `Bits` reads the
    /// surrounding fields the other way round.
    fn decode(&self, bits: &mut Bits) -> Result<u16, String> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for length in 1..16 {
            code |= bits.bit()? as i32;
            let count = self.counts[length] as i32;
            if code - first < count {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("a Huffman code ran past 15 bits".to_string())
    }
}

// The length and distance tables DEFLATE defines. Written out rather than
// computed: they are part of the format, not a derivation, and a reader
// checking them against the specification wants to see the numbers.
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// The order the code-length code's own lengths are stored in.
const LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Decompress a zlib stream: a two-byte header, deflate data, a checksum.
pub fn zlib(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 2 {
        return Err("the zlib stream is too short to have a header".to_string());
    }
    // The low nibble of the first byte is the compression method; 8 is
    // deflate and nothing else was ever defined.
    if data[0] & 0x0f != 8 {
        return Err(format!("not deflate data (method {})", data[0] & 0x0f));
    }
    if (u16::from(data[0]) * 256 + u16::from(data[1])) % 31 != 0 {
        return Err("the zlib header's check bits are wrong".to_string());
    }
    if data[1] & 0x20 != 0 {
        return Err("a preset dictionary is not supported".to_string());
    }
    inflate(&data[2..])
}

/// Decompress raw deflate data.
pub fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut bits = Bits::new(data);
    let mut out = Vec::new();
    loop {
        let last = bits.bit()? == 1;
        match bits.take(2)? {
            0 => stored(&mut bits, &mut out)?,
            1 => {
                let (literals, distances) = fixed_tables();
                block(&mut bits, &mut out, &literals, &distances)?;
            }
            2 => {
                let (literals, distances) = dynamic_tables(&mut bits)?;
                block(&mut bits, &mut out, &literals, &distances)?;
            }
            _ => return Err("reserved block type".to_string()),
        }
        if last {
            return Ok(out);
        }
    }
}

/// An uncompressed block: a length, its complement, then the bytes.
fn stored(bits: &mut Bits, out: &mut Vec<u8>) -> Result<(), String> {
    bits.align();
    let start = bits.byte_position();
    let header = bits
        .bytes
        .get(start..start + 4)
        .ok_or("a stored block's header ran past the end")?;
    let length = u16::from_le_bytes([header[0], header[1]]);
    let complement = u16::from_le_bytes([header[2], header[3]]);
    if length != !complement {
        return Err("a stored block's length does not match its complement".to_string());
    }
    let from = start + 4;
    let bytes = bits
        .bytes
        .get(from..from + length as usize)
        .ok_or("a stored block ran past the end")?;
    out.extend_from_slice(bytes);
    bits.at = (from + length as usize) * 8;
    Ok(())
}

/// The tables for a fixed-Huffman block, which the format defines outright.
fn fixed_tables() -> (Huffman, Huffman) {
    let mut lengths = [0u8; 288];
    for (symbol, length) in lengths.iter_mut().enumerate() {
        *length = match symbol {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    (Huffman::new(&lengths), Huffman::new(&[5u8; 30]))
}

/// Read the tables a dynamic block carries in front of itself.
fn dynamic_tables(bits: &mut Bits) -> Result<(Huffman, Huffman), String> {
    let literal_count = bits.take(5)? as usize + 257;
    let distance_count = bits.take(5)? as usize + 1;
    let code_length_count = bits.take(4)? as usize + 4;

    // The code-length code's lengths, in the permuted order the format
    // stores them in so that the commonest entries come first and the tail
    // can be left out.
    let mut code_lengths = [0u8; 19];
    for &position in LENGTH_ORDER.iter().take(code_length_count) {
        code_lengths[position] = bits.take(3)? as u8;
    }
    let code_length_code = Huffman::new(&code_lengths);

    // Those lengths then decode the real tables' lengths, run-length encoded:
    // 16 repeats the previous length, 17 and 18 are runs of zero.
    let mut lengths = vec![0u8; literal_count + distance_count];
    let mut at = 0;
    while at < lengths.len() {
        let symbol = code_length_code.decode(bits)?;
        match symbol {
            0..=15 => {
                lengths[at] = symbol as u8;
                at += 1;
            }
            16 => {
                if at == 0 {
                    return Err("a repeat with nothing to repeat".to_string());
                }
                let previous = lengths[at - 1];
                let repeat = 3 + bits.take(2)? as usize;
                for _ in 0..repeat {
                    if at >= lengths.len() {
                        return Err("a repeat ran past the table".to_string());
                    }
                    lengths[at] = previous;
                    at += 1;
                }
            }
            17 | 18 => {
                let repeat = if symbol == 17 {
                    3 + bits.take(3)? as usize
                } else {
                    11 + bits.take(7)? as usize
                };
                if at + repeat > lengths.len() {
                    return Err("a run of zeros ran past the table".to_string());
                }
                at += repeat;
            }
            _ => return Err("a code length symbol above 18".to_string()),
        }
    }

    let (literals, distances) = lengths.split_at(literal_count);
    Ok((Huffman::new(literals), Huffman::new(distances)))
}

/// Decode one Huffman-coded block's symbols until its end marker.
fn block(
    bits: &mut Bits,
    out: &mut Vec<u8>,
    literals: &Huffman,
    distances: &Huffman,
) -> Result<(), String> {
    loop {
        let symbol = literals.decode(bits)?;
        match symbol {
            0..=255 => out.push(symbol as u8),
            256 => return Ok(()),
            257..=285 => {
                let index = symbol as usize - 257;
                let length =
                    LENGTH_BASE[index] as usize + bits.take(LENGTH_EXTRA[index] as u32)? as usize;

                let code = distances.decode(bits)? as usize;
                if code >= DISTANCE_BASE.len() {
                    return Err("a distance code above 29".to_string());
                }
                let distance =
                    DISTANCE_BASE[code] as usize + bits.take(DISTANCE_EXTRA[code] as u32)? as usize;
                if distance > out.len() {
                    return Err("a back-reference points before the start".to_string());
                }

                // Copied one byte at a time, on purpose: a run may overlap
                // its own source -- distance 1 and length 200 is how deflate
                // spells "repeat that byte 200 times" -- so the bytes being
                // written are part of what is being read.
                let from = out.len() - distance;
                for n in 0..length {
                    let byte = out[from + n];
                    out.push(byte);
                }
            }
            _ => return Err("a literal symbol above 285".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_block_round_trips() {
        // BFINAL=1, BTYPE=00, then LEN/NLEN and the bytes.
        let mut data = vec![0x01, 0x03, 0x00, 0xfc, 0xff];
        data.extend_from_slice(b"abc");
        assert_eq!(inflate(&data).unwrap(), b"abc");
    }

    #[test]
    fn a_zlib_header_is_checked_rather_than_skipped() {
        assert!(zlib(&[0x78]).is_err(), "too short");
        assert!(zlib(&[0x79, 0x01, 0x00]).is_err(), "bad check bits");
        assert!(zlib(&[0x07, 0x00]).is_err(), "not deflate");
    }

    #[test]
    fn truncated_input_is_an_error_rather_than_a_panic() {
        // A half-written file is a thing that happens, and it must report
        // rather than index past the end of a slice.
        assert!(inflate(&[0x01, 0x03, 0x00]).is_err());
        assert!(inflate(&[]).is_err());
        assert!(inflate(&[0xff, 0xff, 0xff]).is_err());
    }

    // -- Vectors from a real compressor ---------------------------------
    //
    // `png.rs` only ever writes stored blocks, so nothing this crate produces
    // exercises Huffman decoding at all. These came out of Python's zlib --
    // an independent implementation making its own choices about block types
    // and code lengths -- which is the only way to find out whether this
    // decoder reads what the rest of the world writes.

    /// 540 bytes compressed to 57 by zlib at level 9.
    const PROSE: &[u8] = &[
        120, 218, 43, 201, 72, 85, 40, 44, 205, 76, 206, 86, 72, 42, 202, 47, 207, 83, 72, 203,
        175, 80, 200, 42, 205, 45, 40, 86, 200, 47, 75, 45, 82, 40, 1, 74, 231, 36, 86, 85, 42,
        164, 228, 167, 235, 129, 121, 163, 138, 71, 140, 98, 0, 247, 36, 195, 85,
    ];

    /// 2 bytes compressed to 10 by zlib at level 9.
    const SHORT: &[u8] = &[120, 218, 203, 200, 4, 0, 1, 59, 0, 210];

    /// 500 bytes compressed to 14 by zlib at level 9.
    const RUN: &[u8] = &[120, 218, 115, 116, 28, 5, 35, 13, 0, 0, 66, 250, 126, 245];

    /// 400 bytes compressed to 411 by zlib at level 0.
    const NOISE: &[u8] = &[
        120, 1, 1, 144, 1, 111, 254, 165, 77, 202, 24, 37, 48, 187, 29, 109, 19, 44, 222, 214, 35,
        123, 46, 217, 30, 63, 114, 31, 203, 25, 113, 23, 68, 148, 214, 73, 60, 157, 92, 52, 96,
        190, 49, 32, 30, 105, 254, 218, 160, 238, 232, 185, 153, 127, 92, 124, 41, 153, 253, 175,
        229, 147, 37, 60, 214, 84, 175, 77, 250, 215, 20, 39, 160, 174, 179, 254, 233, 35, 47, 138,
        242, 33, 31, 158, 228, 145, 197, 177, 11, 236, 181, 86, 59, 252, 30, 111, 147, 66, 126,
        203, 200, 254, 41, 85, 229, 205, 142, 70, 220, 142, 212, 183, 194, 118, 77, 42, 90, 77,
        118, 119, 6, 248, 93, 134, 144, 2, 74, 214, 189, 163, 64, 27, 233, 200, 203, 204, 201, 53,
        246, 205, 31, 97, 34, 106, 225, 83, 56, 174, 26, 52, 0, 77, 51, 186, 13, 36, 106, 192, 76,
        129, 177, 186, 242, 62, 59, 249, 238, 245, 247, 159, 43, 73, 52, 175, 135, 245, 82, 11,
        105, 185, 75, 13, 152, 46, 133, 187, 85, 182, 114, 168, 114, 99, 122, 205, 116, 102, 252,
        182, 14, 14, 143, 241, 132, 99, 176, 228, 178, 186, 41, 112, 52, 116, 240, 100, 172, 104,
        247, 0, 245, 176, 43, 61, 198, 102, 244, 91, 222, 170, 44, 202, 237, 205, 43, 81, 87, 65,
        14, 77, 238, 74, 242, 179, 79, 67, 10, 7, 52, 71, 222, 99, 108, 14, 128, 108, 149, 123,
        166, 132, 214, 67, 31, 181, 234, 215, 66, 77, 9, 225, 93, 2, 76, 88, 72, 242, 61, 31, 166,
        247, 54, 29, 127, 97, 141, 21, 50, 231, 14, 32, 226, 166, 102, 141, 231, 244, 126, 132,
        103, 229, 70, 213, 62, 200, 226, 161, 37, 123, 219, 37, 108, 155, 62, 79, 187, 73, 129, 70,
        239, 112, 48, 203, 249, 83, 114, 82, 220, 206, 173, 215, 100, 182, 163, 47, 187, 9, 173,
        234, 225, 9, 196, 169, 151, 32, 57, 117, 53, 43, 135, 139, 20, 92, 138, 66, 216, 132, 207,
        76, 253, 167, 45, 142, 29, 93, 217, 37, 137, 8, 45, 133, 42, 113, 34, 135, 62, 232, 5, 173,
        213, 137, 66, 22, 122, 56, 82, 134, 25, 92, 103, 159, 156, 105, 148, 228, 91, 138, 177, 9,
        128, 18, 7, 9, 97, 243, 125, 228, 54, 221, 253, 170, 155, 195, 160,
    ];

    /// 512 bytes compressed to 282 by zlib at level 9.
    const EVERY_BYTE: &[u8] = &[
        120, 218, 99, 96, 100, 98, 102, 97, 101, 99, 231, 224, 228, 226, 230, 225, 229, 227, 23,
        16, 20, 18, 22, 17, 21, 19, 151, 144, 148, 146, 150, 145, 149, 147, 87, 80, 84, 82, 86, 81,
        85, 83, 215, 208, 212, 210, 214, 209, 213, 211, 55, 48, 52, 50, 54, 49, 53, 51, 183, 176,
        180, 178, 182, 177, 181, 179, 119, 112, 116, 114, 118, 113, 117, 115, 247, 240, 244, 242,
        246, 241, 245, 243, 15, 8, 12, 10, 14, 9, 13, 11, 143, 136, 140, 138, 142, 137, 141, 139,
        79, 72, 76, 74, 78, 73, 77, 75, 207, 200, 204, 202, 206, 201, 205, 203, 47, 40, 44, 42, 46,
        41, 45, 43, 175, 168, 172, 170, 174, 169, 173, 171, 111, 104, 108, 106, 110, 105, 109, 107,
        239, 232, 236, 234, 238, 233, 237, 235, 159, 48, 113, 210, 228, 41, 83, 167, 77, 159, 49,
        115, 214, 236, 57, 115, 231, 205, 95, 176, 112, 209, 226, 37, 75, 151, 45, 95, 177, 114,
        213, 234, 53, 107, 215, 173, 223, 176, 113, 211, 230, 45, 91, 183, 109, 223, 177, 115, 215,
        238, 61, 123, 247, 237, 63, 112, 240, 208, 225, 35, 71, 143, 29, 63, 113, 242, 212, 233,
        51, 103, 207, 157, 191, 112, 241, 210, 229, 43, 87, 175, 93, 191, 113, 243, 214, 237, 59,
        119, 239, 221, 127, 240, 240, 209, 227, 39, 79, 159, 61, 127, 241, 242, 213, 235, 55, 111,
        223, 189, 255, 240, 241, 211, 231, 47, 95, 191, 125, 255, 241, 243, 215, 239, 63, 127, 255,
        253, 103, 24, 225, 254, 7, 0, 227, 108, 255, 1,
    ];

    #[test]
    fn prose_uses_dynamic_huffman_and_back_references() {
        let expected = "the quick brown fox jumps over the lazy dog. ".repeat(12);
        assert_eq!(zlib(PROSE).unwrap(), expected.as_bytes());
    }

    #[test]
    fn a_tiny_input_uses_the_fixed_tables() {
        assert_eq!(zlib(SHORT).unwrap(), b"hi");
    }

    #[test]
    fn a_long_run_copies_from_bytes_it_is_still_writing() {
        // Distance 1 with a length of hundreds is how deflate spells "repeat
        // that byte": the source of the copy is inside the copy's own output,
        // which is why the inner loop goes one byte at a time.
        assert_eq!(zlib(RUN).unwrap(), vec![b'A'; 500]);
    }

    #[test]
    fn incompressible_data_comes_back_through_stored_blocks() {
        // Level 0 makes a real encoder emit stored blocks, and 400 bytes
        // needs more than one -- so this also checks that a block which is
        // not the last one is followed rather than treated as the end.
        let out = zlib(NOISE).unwrap();
        assert_eq!(out.len(), 400);
        assert_eq!(out[0], 165, "the first byte the generator produced");
        assert_eq!(out[399], 253, "and the last");
    }

    #[test]
    fn every_byte_value_survives() {
        // Literals above 143 use 9-bit codes in the fixed tables and their
        // own lengths in a dynamic one, so a decoder that only ever saw
        // ASCII would pass every other test and fail this.
        let expected: Vec<u8> = (0..=255u8).chain(0..=255u8).collect();
        assert_eq!(zlib(EVERY_BYTE).unwrap(), expected);
    }

    #[test]
    fn corrupting_a_vector_is_detected_rather_than_decoded() {
        // Not every flipped bit can be caught -- Huffman data is dense -- but
        // a decoder that never errors on damage is not checking anything.
        let mut broken = PROSE.to_vec();
        broken[0] ^= 0xff;
        assert!(zlib(&broken).is_err(), "a mangled header");
        let truncated = &PROSE[..PROSE.len() / 2];
        assert!(zlib(truncated).is_err(), "half a stream");
    }

    #[test]
    fn a_reserved_block_type_is_refused() {
        // BFINAL=1, BTYPE=11.
        assert!(inflate(&[0b111]).is_err());
    }
}
