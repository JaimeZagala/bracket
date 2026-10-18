//! Bracket's icon, drawn by code: a blue rounded square with white brackets and a caret between
//! them, `[|]`. Keeping it as code means no binary file in the repository and a reviewable
//! design.
//!
//! Each pixel averages 4×4 samples, so edges are smooth at every size.

/// Colour of a point in unit coordinates (0..1 on both axes), with alpha; `None` if outside.
fn colour_at(x: f32, y: f32) -> Option<[f32; 4]> {
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    const CARET: [f32; 4] = [0.65, 0.82, 1.0, 1.0];

    if !in_rounded_square(x, y, 0.04, 0.2) {
        return None;
    }
    let stroke = 0.09;
    let (top, bottom) = (0.24, 0.76);
    let serif = 0.15;
    // Left bracket; the right one is its mirror image.
    let bracket = |x: f32| {
        let (left, right) = (0.22, 0.22 + stroke);
        let bar = (left..right).contains(&x) && (top..bottom).contains(&y);
        let serifs = (left..left + serif).contains(&x)
            && ((top..top + stroke).contains(&y) || (bottom - stroke..bottom).contains(&y));
        bar || serifs
    };
    if bracket(x) || bracket(1.0 - x) {
        return Some(WHITE);
    }
    let caret_half = stroke * 0.32;
    if (0.5 - caret_half..0.5 + caret_half).contains(&x) && (0.33..0.67).contains(&y) {
        return Some(CARET);
    }
    // A slight vertical gradient: #3B82F6 at the top to #1D4ED8 at the bottom.
    let mix = |a: f32, b: f32| a + (b - a) * y;
    Some([
        mix(0x3B as f32, 0x1D as f32) / 255.0,
        mix(0x82 as f32, 0x4E as f32) / 255.0,
        mix(0xF6 as f32, 0xD8 as f32) / 255.0,
        1.0,
    ])
}

fn in_rounded_square(x: f32, y: f32, margin: f32, radius: f32) -> bool {
    let (low, high) = (margin, 1.0 - margin);
    if !(low..high).contains(&x) || !(low..high).contains(&y) {
        return false;
    }
    let cx = x.clamp(low + radius, high - radius);
    let cy = y.clamp(low + radius, high - radius);
    (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
}

/// The icon at `size`×`size` pixels, as RGBA rows from the top.
pub fn rgba(size: u32) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            // Premultiplied sum of the samples.
            let mut sum = [0.0f32; 4];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = (px as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size as f32;
                    if let Some([r, g, b, a]) = colour_at(x, y) {
                        sum[0] += r * a;
                        sum[1] += g * a;
                        sum[2] += b * a;
                        sum[3] += a;
                    }
                }
            }
            let count = (SAMPLES * SAMPLES) as f32;
            let alpha = sum[3] / count;
            let channel = |v: f32| {
                if alpha > 0.0 {
                    ((v / count / alpha) * 255.0).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                }
            };
            out.extend([
                channel(sum[0]),
                channel(sum[1]),
                channel(sum[2]),
                (alpha * 255.0).round() as u8,
            ]);
        }
    }
    out
}

/// A `.ico` file with the icon at each size. Sizes below 64 are 32-bit bitmaps (what older parts
/// of Windows read best); larger ones are PNG, much smaller.
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&size| {
            let pixels = rgba(size);
            if size >= 64 {
                png(size, &pixels)
            } else {
                bitmap(size, &pixels)
            }
        })
        .collect();

    let mut out = Vec::new();
    out.extend(0u16.to_le_bytes()); // reserved
    out.extend(1u16.to_le_bytes()); // icon
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&size, image) in sizes.iter().zip(&images) {
        let side = if size >= 256 { 0 } else { size as u8 };
        out.extend([side, side, 0, 0]);
        out.extend(1u16.to_le_bytes()); // colour planes
        out.extend(32u16.to_le_bytes()); // bits per pixel
        out.extend((image.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        out.extend(image);
    }
    out
}

/// A 32-bit icon bitmap: header, BGRA rows from the bottom, then an empty AND mask.
fn bitmap(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4;
    let mut out = Vec::new();
    out.extend(40u32.to_le_bytes()); // header size
    out.extend((size as i32).to_le_bytes());
    out.extend((size as i32 * 2).to_le_bytes()); // colour image plus mask
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // no compression
    out.extend((size * size * 4 + mask_row * size).to_le_bytes());
    out.extend([0u8; 16]); // resolution and palette: unused
    for row in (0..size as usize).rev() {
        for pixel in rgba[row * size as usize * 4..(row + 1) * size as usize * 4].chunks(4) {
            out.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    out.extend(std::iter::repeat_n(0u8, (mask_row * size) as usize));
    out
}

/// A PNG file (RGBA, 8 bits per channel).
fn png(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgba.len() + size as usize);
    for row in rgba.chunks(size as usize * 4) {
        raw.push(0); // no filter
        raw.extend(row);
    }
    let mut header = Vec::new();
    header.extend(size.to_be_bytes());
    header.extend(size.to_be_bytes());
    header.extend([8, 6, 0, 0, 0]); // 8 bits, RGBA, deflate, no filter, no interlace

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut out, b"IHDR", &header);
    chunk(
        &mut out,
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 10),
    );
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(kind);
    out.extend(data);
    let crc = crc32(kind.iter().chain(data));
    out.extend(crc.to_be_bytes());
}

/// CRC-32 as PNG uses it (polynomial 0xEDB88320).
fn crc32<'a>(bytes: impl Iterator<Item = &'a u8>) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
