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

    // -- Real PNGs, from an encoder that is not this one -----------------
    //
    // `encode_rgba` only ever writes stored blocks and filter 0, so a test
    // built from its own output would exercise neither Huffman decoding nor
    // four of the five row filters. These fixtures were produced by Python:
    // an independent encoder, making its own choices.

    const RGBA_ALL_FILTERS: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 5, 8, 6,
        0, 0, 0, 98, 173, 77, 219, 0, 0, 0, 76, 73, 68, 65, 84, 120, 218, 99, 96, 248, 207, 240,
        95, 227, 33, 195, 255, 128, 195, 12, 255, 43, 150, 50, 252, 103, 100, 255, 207, 240, 85,
        227, 145, 56, 3, 12, 51, 177, 51, 48, 124, 99, 103, 16, 7, 98, 61, 32, 118, 253, 198, 204,
        215, 192, 144, 35, 241, 81, 239, 183, 196, 71, 43, 32, 118, 253, 205, 2, 85, 193, 0, 84, 1,
        196, 174, 12, 0, 55, 193, 27, 106, 2, 247, 43, 113, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
        96, 130,
    ];

    const RGB: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 3, 0, 0, 0, 2, 8, 2,
        0, 0, 0, 18, 22, 241, 77, 0, 0, 0, 23, 73, 68, 65, 84, 120, 218, 99, 248, 207, 192, 192, 0,
        193, 92, 34, 114, 26, 70, 54, 110, 1, 81, 0, 51, 89, 4, 192, 92, 155, 225, 140, 0, 0, 0, 0,
        73, 69, 78, 68, 174, 66, 96, 130,
    ];

    const PALETTE: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 3,
        0, 0, 0, 69, 104, 253, 22, 0, 0, 0, 9, 80, 76, 84, 69, 255, 0, 0, 0, 255, 0, 0, 0, 255, 45,
        74, 205, 138, 0, 0, 0, 2, 116, 82, 78, 83, 255, 128, 8, 15, 179, 106, 0, 0, 0, 14, 73, 68,
        65, 84, 120, 218, 99, 96, 96, 100, 96, 98, 0, 0, 0, 14, 0, 4, 219, 224, 50, 142, 0, 0, 0,
        0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    const GREY_ALPHA: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1, 8, 4,
        0, 0, 0, 94, 43, 183, 1, 0, 0, 0, 13, 73, 68, 65, 84, 120, 218, 99, 56, 241, 63, 197, 1, 0,
        7, 42, 2, 108, 240, 191, 79, 148, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    const RGB16: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1, 16,
        2, 0, 0, 0, 43, 208, 52, 158, 0, 0, 0, 21, 73, 68, 65, 84, 120, 218, 99, 88, 181, 91, 80,
        201, 216, 229, 255, 127, 6, 134, 134, 6, 0, 33, 211, 5, 14, 117, 253, 58, 81, 0, 0, 0, 0,
        73, 69, 78, 68, 174, 66, 96, 130,
    ];

    const PALETTE_1BIT: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 1, 1, 3,
        0, 0, 0, 217, 206, 125, 0, 0, 0, 0, 6, 80, 76, 84, 69, 0, 0, 0, 255, 255, 255, 165, 217,
        159, 221, 0, 0, 0, 10, 73, 68, 65, 84, 120, 218, 99, 216, 8, 0, 0, 179, 0, 178, 140, 26,
        43, 71, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    /// The pixel at (x, y) as (r, g, b, a).
    fn pixel(image: &Decoded, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let at = ((y * image.width + x) * 4) as usize;
        (
            image.pixels[at],
            image.pixels[at + 1],
            image.pixels[at + 2],
            image.pixels[at + 3],
        )
    }

    #[test]
    fn every_row_filter_reconstructs_the_same_picture() {
        // One image whose five rows each use a different filter, so a
        // predictor that is subtly wrong shows up as one wrong row rather
        // than as a file that fails to decode.
        let image = decode(RGBA_ALL_FILTERS).expect("decodes");
        assert_eq!((image.width, image.height), (4, 5));
        for y in 0..5u32 {
            for x in 0..4u32 {
                let expected = (
                    (x * 40 + y * 7) as u8,
                    (255 - x * 30) as u8,
                    ((x * y * 23) % 256) as u8,
                    (255 - y * 10) as u8,
                );
                assert_eq!(pixel(&image, x, y), expected, "at {x},{y} (filter {y})");
            }
        }
    }

    #[test]
    fn rgb_gains_an_opaque_alpha() {
        let image = decode(RGB).expect("decodes");
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(pixel(&image, 0, 0), (255, 0, 0, 255));
        assert_eq!(pixel(&image, 2, 0), (0, 0, 255, 255));
        assert_eq!(pixel(&image, 1, 1), (40, 50, 60, 255));
    }

    #[test]
    fn a_palette_is_looked_up_and_its_transparency_applied() {
        let image = decode(PALETTE).expect("decodes");
        assert_eq!(pixel(&image, 0, 0), (255, 0, 0, 255), "entry 0, alpha 255");
        assert_eq!(pixel(&image, 1, 0), (0, 255, 0, 128), "entry 1, alpha 128");
        // tRNS is shorter than the palette, and entries past its end are
        // opaque rather than missing.
        assert_eq!(pixel(&image, 0, 1), (0, 0, 255, 255), "entry 2, no tRNS");
    }

    #[test]
    fn greyscale_with_alpha_becomes_three_equal_channels() {
        let image = decode(GREY_ALPHA).expect("decodes");
        assert_eq!(pixel(&image, 0, 0), (200, 200, 200, 255));
        assert_eq!(pixel(&image, 1, 0), (100, 100, 100, 64));
    }

    #[test]
    fn sixteen_bit_channels_keep_their_high_byte() {
        // The low byte is precision a framebuffer has nowhere to put.
        let image = decode(RGB16).expect("decodes");
        assert_eq!(pixel(&image, 0, 0), (0xAA, 0x11, 0x33, 255));
        assert_eq!(pixel(&image, 1, 0), (0xFF, 0x00, 0x80, 255));
    }

    #[test]
    fn a_one_bit_palette_unpacks_eight_pixels_from_one_byte() {
        // 0b10110001, most significant bit first.
        let image = decode(PALETTE_1BIT).expect("decodes");
        assert_eq!(image.width, 8);
        let lit: Vec<bool> = (0..8).map(|x| pixel(&image, x, 0).0 == 255).collect();
        assert_eq!(lit, [true, false, true, true, false, false, false, true]);
    }

    #[test]
    fn what_this_crate_writes_it_can_read_back() {
        // The round trip the two halves owe each other, over data with no
        // pattern for the encoder to lean on.
        let pixels: Vec<u8> = (0..16 * 9 * 4).map(|n| ((n * 37) % 251) as u8).collect();
        let file = encode_rgba(16, 9, &pixels);
        let image = decode(&file).expect("decodes its own output");
        assert_eq!((image.width, image.height), (16, 9));
        assert_eq!(image.pixels, pixels);
    }

    #[test]
    fn a_file_that_is_not_a_png_says_so() {
        assert!(decode(b"").is_err());
        assert!(decode(b"GIF89a and then some").is_err());
        // A valid signature with nothing behind it.
        assert!(decode(&SIGNATURE).is_err());
    }

    #[test]
    fn an_unsupported_png_is_named_rather_than_smeared() {
        // Interlacing is a different layout for the same data. Refusing is
        // honest; decoding one as if it were not interlaced is a smear.
        let mut interlaced = RGB.to_vec();
        // The interlace byte is the last of IHDR's 13, at offset 8+8+12.
        interlaced[28] = 1;
        let error = decode(&interlaced).unwrap_err();
        assert!(error.contains("interlaced"), "got {error:?}");
    }

    #[test]
    fn a_truncated_png_is_an_error_rather_than_a_panic() {
        for cut in [10, 20, 40, 60] {
            assert!(decode(&RGB[..cut]).is_err(), "truncated at {cut}");
        }
    }

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

/// A decoded image: 8-bit RGBA, row-major.
#[derive(Debug)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Read a PNG.
///
/// -- What is supported, and what is refused by name --
///
/// Every colour type at 8 bits a channel, palettes down to 1 bit, and 16-bit
/// channels reduced to 8. That is what image editors produce, and what a game
/// loads.
///
/// Interlaced images are refused rather than half-read. Adam7 is a different
/// layout for the same data -- seven passes at rising resolution -- and it is
/// rare enough in game art that supporting it would be code nobody exercised.
/// Saying so beats decoding one into a smear.
pub fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    if bytes.len() < 8 || bytes[..8] != SIGNATURE {
        return Err("not a PNG (the signature is wrong)".to_string());
    }

    let mut header: Option<Header> = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut transparency: Vec<u8> = Vec::new();
    let mut compressed: Vec<u8> = Vec::new();
    let mut at = 8;

    while at + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind = &bytes[at + 4..at + 8];
        let from = at + 8;
        let body = bytes
            .get(from..from + length)
            .ok_or("a chunk runs past the end of the file")?;

        match kind {
            b"IHDR" => header = Some(Header::read(body)?),
            b"PLTE" => {
                palette = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            // Transparency for a palette: one alpha per entry, and any entry
            // past the end is opaque.
            b"tRNS" => transparency = body.to_vec(),
            b"IDAT" => compressed.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        at = from + length + 4;
    }

    let header = header.ok_or("the PNG has no header chunk")?;
    if compressed.is_empty() {
        return Err("the PNG has no image data".to_string());
    }
    let raw = crate::inflate::zlib(&compressed)?;
    let scanlines = unfilter(&raw, &header)?;
    expand(&scanlines, &header, &palette, &transparency)
}

struct Header {
    width: u32,
    height: u32,
    depth: u8,
    colour: u8,
}

impl Header {
    fn read(body: &[u8]) -> Result<Header, String> {
        if body.len() < 13 {
            return Err("the header chunk is too short".to_string());
        }
        let header = Header {
            width: u32::from_be_bytes(body[0..4].try_into().unwrap()),
            height: u32::from_be_bytes(body[4..8].try_into().unwrap()),
            depth: body[8],
            colour: body[9],
        };
        if header.width == 0 || header.height == 0 {
            return Err("the image has no pixels".to_string());
        }
        if body[12] != 0 {
            return Err("interlaced PNGs are not supported".to_string());
        }
        let ok = match header.colour {
            0 => matches!(header.depth, 1 | 2 | 4 | 8 | 16),
            3 => matches!(header.depth, 1 | 2 | 4 | 8),
            2 | 4 | 6 => matches!(header.depth, 8 | 16),
            other => return Err(format!("unknown colour type {other}")),
        };
        if !ok {
            return Err(format!(
                "colour type {} does not come at {} bits a channel",
                header.colour, header.depth
            ));
        }
        Ok(header)
    }

    /// Channels per pixel, before any palette lookup.
    fn channels(&self) -> usize {
        match self.colour {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            _ => 4,
        }
    }

    /// Bytes in one row, rounded up when a pixel is narrower than a byte.
    fn stride(&self) -> usize {
        (self.width as usize * self.channels() * self.depth as usize).div_ceil(8)
    }

    /// The filter's idea of "the pixel before this one", in bytes, never zero.
    fn filter_step(&self) -> usize {
        (self.channels() * self.depth as usize / 8).max(1)
    }
}

/// Undo the per-row filters, leaving the raw scanline bytes.
///
/// Each row carries a filter byte saying how it was encoded against its
/// neighbours. This is where a decoder is usually wrong in a way that still
/// produces an image: the predictors refer to the *reconstructed* bytes to
/// the left and above, not the encoded ones, so getting it subtly wrong
/// smears colour down the picture rather than failing.
fn unfilter(raw: &[u8], header: &Header) -> Result<Vec<u8>, String> {
    let stride = header.stride();
    let step = header.filter_step();
    let height = header.height as usize;
    if raw.len() < (stride + 1) * height {
        return Err(format!(
            "the image data is short: {} bytes for {}x{}",
            raw.len(),
            header.width,
            header.height
        ));
    }

    let mut out = vec![0u8; stride * height];
    for row in 0..height {
        let filter = raw[row * (stride + 1)];
        let source = &raw[row * (stride + 1) + 1..row * (stride + 1) + 1 + stride];
        for column in 0..stride {
            let left = if column >= step {
                out[row * stride + column - step]
            } else {
                0
            };
            let above = if row > 0 {
                out[(row - 1) * stride + column]
            } else {
                0
            };
            let above_left = if row > 0 && column >= step {
                out[(row - 1) * stride + column - step]
            } else {
                0
            };
            let value = match filter {
                0 => source[column],
                1 => source[column].wrapping_add(left),
                2 => source[column].wrapping_add(above),
                3 => source[column].wrapping_add(((left as u16 + above as u16) / 2) as u8),
                4 => source[column].wrapping_add(paeth(left, above, above_left)),
                other => return Err(format!("unknown row filter {other}")),
            };
            out[row * stride + column] = value;
        }
    }
    Ok(out)
}

/// PNG's Paeth predictor: whichever neighbour the gradient points closest to.
fn paeth(left: u8, above: u8, above_left: u8) -> u8 {
    let estimate = left as i16 + above as i16 - above_left as i16;
    let to_left = (estimate - left as i16).abs();
    let to_above = (estimate - above as i16).abs();
    let to_corner = (estimate - above_left as i16).abs();
    if to_left <= to_above && to_left <= to_corner {
        left
    } else if to_above <= to_corner {
        above
    } else {
        above_left
    }
}

/// Turn unfiltered scanlines into 8-bit RGBA.
fn expand(
    scanlines: &[u8],
    header: &Header,
    palette: &[[u8; 3]],
    transparency: &[u8],
) -> Result<Decoded, String> {
    let stride = header.stride();
    let width = header.width as usize;
    let height = header.height as usize;
    let mut pixels = Vec::with_capacity(width * height * 4);

    for row in 0..height {
        let line = &scanlines[row * stride..(row + 1) * stride];
        for column in 0..width {
            // 16-bit channels keep their high byte. The low one is precision
            // a framebuffer has nowhere to put, and dropping it is what every
            // 8-bit pipeline does.
            let sample = |channel: usize| -> u8 {
                match header.depth {
                    16 => line[(column * header.channels() + channel) * 2],
                    8 => line[column * header.channels() + channel],
                    depth => {
                        // Narrow samples are packed several to a byte, most
                        // significant first.
                        let index = column * header.channels() + channel;
                        let per_byte = 8 / depth as usize;
                        let byte = line[index / per_byte];
                        let shift = 8 - depth as usize * (index % per_byte + 1);
                        (byte >> shift) & ((1 << depth) - 1)
                    }
                }
            };

            let rgba = match header.colour {
                0 => {
                    // Greyscale below 8 bits is scaled to fill the range, so
                    // 1-bit white is 255 rather than 1.
                    let value = sample(0);
                    let grey = match header.depth {
                        1 => value * 255,
                        2 => value * 85,
                        4 => value * 17,
                        _ => value,
                    };
                    [grey, grey, grey, 255]
                }
                2 => [sample(0), sample(1), sample(2), 255],
                3 => {
                    let index = sample(0) as usize;
                    let colour = palette
                        .get(index)
                        .ok_or_else(|| format!("palette entry {index} is missing"))?;
                    let alpha = transparency.get(index).copied().unwrap_or(255);
                    [colour[0], colour[1], colour[2], alpha]
                }
                4 => {
                    let grey = sample(0);
                    [grey, grey, grey, sample(1)]
                }
                _ => [sample(0), sample(1), sample(2), sample(3)],
            };
            pixels.extend_from_slice(&rgba);
        }
    }

    Ok(Decoded {
        width: header.width,
        height: header.height,
        pixels,
    })
}
