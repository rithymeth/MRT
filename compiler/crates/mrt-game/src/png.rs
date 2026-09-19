//! A PNG encoder, in a page, with no dependencies.
//!
//! -- Why write one --
//!
//! A framebuffer nobody can look at is not a framebuffer. Something has to
//! turn pixels into a file, and the obvious answer is the `png` crate --
//! which, with its compression backend, is a dependency tree on a workspace
//! whose manifest says in its first line that it has none.
//!
//! The escape is that PNG does not require compression. Its data stream is
//! zlib, and zlib's deflate format has a **stored** block type: a length and
//! then the bytes, uncompressed. So a valid PNG is a header, the rows with a
//! filter byte each, wrapped in stored blocks, with two checksums. Every
//! decoder in the world reads it, because there is nothing unusual about it.
//!
//! The cost is honest and bounded: a file roughly the size of the raw pixels,
//! where a compressing encoder would manage a fraction of that. For frames a
//! person looks at, that is a fine trade for a hermetic build. If images ever
//! need to be small, this is the file to replace, and nothing else changes.

/// The PNG signature: the bytes every file starts with, and the reason a
/// truncated download is detected rather than half-decoded.
const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// The largest a single deflate stored block may declare.
const MAX_BLOCK: usize = 65535;

/// Encode 8-bit RGBA pixels, row-major, into a PNG file's bytes.
///
/// `pixels` is `width * height * 4` bytes. Alpha is kept rather than
/// flattened onto a background, so a saved frame can be composited later.
pub fn encode_rgba(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    assert_eq!(
        pixels.len(),
        width as usize * height as usize * 4,
        "pixel buffer does not match the stated size"
    );

    let mut out = Vec::from(SIGNATURE);

    // IHDR: colour type 6 is RGBA, bit depth 8. Compression, filter and
    // interlace methods each have exactly one legal value in the standard.
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);

    chunk(&mut out, b"IDAT", &zlib(&filtered(width, height, pixels)));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Prefix each row with its filter byte.
///
/// Filter 0 means "no filtering": the row is its own bytes. The filter byte
/// is not optional -- a decoder reads one per row whatever it says -- so this
/// step exists even when there is nothing to undo.
fn filtered(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let stride = width as usize * 4;
    let mut out = Vec::with_capacity((stride + 1) * height as usize);
    for row in 0..height as usize {
        out.push(0);
        out.extend_from_slice(&pixels[row * stride..(row + 1) * stride]);
    }
    out
}

/// Wrap bytes in a zlib stream of stored deflate blocks.
fn zlib(data: &[u8]) -> Vec<u8> {
    // 0x78 0x01: deflate with a 32K window, no preset dictionary. The pair is
    // chosen so the 16-bit value is a multiple of 31, which is the header's
    // own check.
    let mut out = vec![0x78, 0x01];

    // An empty input still needs one block, or the stream ends without ever
    // declaring itself final and a strict decoder rejects the file.
    let mut at = 0;
    loop {
        let len = (data.len() - at).min(MAX_BLOCK);
        let final_block = at + len == data.len();
        out.push(u8::from(final_block));
        out.extend_from_slice(&(len as u16).to_le_bytes());
        // NLEN is LEN's ones' complement: the redundancy deflate uses to
        // catch a stored block whose length was corrupted.
        out.extend_from_slice(&(!(len as u16)).to_le_bytes());
        out.extend_from_slice(&data[at..at + len]);
        at += len;
        if final_block {
            break;
        }
    }

    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Append one chunk: length, type, data, and a CRC over the type *and* data.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// Adler-32, as zlib specifies: two running sums modulo 65521.
fn adler32(data: &[u8]) -> u32 {
    let (mut low, mut high) = (1u32, 0u32);
    for byte in data {
        low = (low + *byte as u32) % 65521;
        high = (high + low) % 65521;
    }
    (high << 16) | low
}

/// CRC-32, as PNG specifies.
///
/// The table is built on the fly rather than written out as 256 constants: it
/// is the same computation either way, and the loop says where the numbers
/// come from where a table of literals would not.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            // 0xedb88320 is the standard polynomial, bit-reversed.
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The checksums are the part that cannot be eyeballed, so they are
    /// pinned against values the standards themselves publish.
    #[test]
    fn the_checksums_match_the_published_ones() {
        // zlib's specification uses "Wikipedia" as its worked Adler-32
        // example; CRC-32 of "123456789" is the check value every CRC
        // catalogue lists for this polynomial.
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b""), 1, "the empty sum is 1, not 0");
    }

    #[test]
    fn a_file_has_the_structure_a_decoder_looks_for() {
        let png = encode_rgba(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]);
        assert_eq!(&png[..8], &SIGNATURE);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..20], &2u32.to_be_bytes(), "width");
        assert_eq!(&png[20..24], &1u32.to_be_bytes(), "height");
        assert_eq!(png[24], 8, "bit depth");
        assert_eq!(png[25], 6, "colour type RGBA");
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }

    #[test]
    fn every_chunk_carries_a_crc_that_checks_out() {
        // Walk the file the way a decoder does, and verify each chunk rather
        // than trusting that writing one is the same as writing it right.
        let png = encode_rgba(3, 2, &[128; 24]);
        let mut at = 8;
        let mut kinds = Vec::new();
        while at < png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let end = at + 8 + len;
            let stated = u32::from_be_bytes(png[end..end + 4].try_into().unwrap());
            assert_eq!(crc32(&png[at + 4..end]), stated, "chunk at {at}");
            kinds.push(String::from_utf8(png[at + 4..at + 8].to_vec()).unwrap());
            at = end + 4;
        }
        assert_eq!(at, png.len(), "the chunks account for the whole file");
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
    }

    #[test]
    fn the_pixel_data_survives_the_round_trip() {
        // Undo the stored blocks and the filter bytes by hand: what comes
        // back out has to be exactly what went in, or the encoder is writing
        // a valid file of the wrong picture.
        let pixels: Vec<u8> = (0..4 * 3 * 4).map(|n| (n * 7 % 251) as u8).collect();
        let png = encode_rgba(4, 3, &pixels);

        let start = png
            .windows(4)
            .position(|w| w == b"IDAT")
            .expect("an IDAT chunk");
        let len = u32::from_be_bytes(png[start - 4..start].try_into().unwrap()) as usize;
        let stream = &png[start + 4..start + 4 + len];

        let mut raw = Vec::new();
        let mut at = 2; // past the zlib header
        loop {
            let last = stream[at] & 1 != 0;
            let block = u16::from_le_bytes(stream[at + 1..at + 3].try_into().unwrap()) as usize;
            let nlen = u16::from_le_bytes(stream[at + 3..at + 5].try_into().unwrap());
            assert_eq!(nlen, !(block as u16), "NLEN complements LEN");
            raw.extend_from_slice(&stream[at + 5..at + 5 + block]);
            at += 5 + block;
            if last {
                break;
            }
        }
        assert_eq!(
            u32::from_be_bytes(stream[at..at + 4].try_into().unwrap()),
            adler32(&raw)
        );

        let mut decoded = Vec::new();
        for row in raw.chunks(4 * 4 + 1) {
            assert_eq!(row[0], 0, "filter byte");
            decoded.extend_from_slice(&row[1..]);
        }
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn an_image_too_big_for_one_block_is_split_and_still_whole() {
        // 64x64 RGBA is 16448 bytes with filter bytes -- under the limit --
        // so the split only shows up on something larger. 200x100 is 80700.
        let png = encode_rgba(200, 100, &vec![9; 200 * 100 * 4]);
        let start = png.windows(4).position(|w| w == b"IDAT").unwrap();
        let len = u32::from_be_bytes(png[start - 4..start].try_into().unwrap()) as usize;
        let stream = &png[start + 4..start + 4 + len];

        let mut blocks = 0;
        let mut total = 0;
        let mut at = 2;
        loop {
            let last = stream[at] & 1 != 0;
            let block = u16::from_le_bytes(stream[at + 1..at + 3].try_into().unwrap()) as usize;
            blocks += 1;
            total += block;
            at += 5 + block;
            if last {
                break;
            }
        }
        assert!(blocks > 1, "expected more than one block, got {blocks}");
        assert_eq!(total, 100 * (200 * 4 + 1), "every row accounted for");
    }
}
