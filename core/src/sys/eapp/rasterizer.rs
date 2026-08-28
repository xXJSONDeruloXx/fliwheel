use std::path::Path;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TextureFormat {
    Rgb565,
    Rgba5551,
    Rgba4444,
    Rgba8888,
    LuminanceAlpha88,
    A8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Rgba8>,
}

impl Texture {
    pub fn from_bytes(
        raw: &[u8],
        width: usize,
        height: usize,
        format: TextureFormat,
        a8_tint: Rgba8,
    ) -> Self {
        let pixels = decode_texture_pixels(raw, width, height, format, a8_tint);
        Self {
            width,
            height,
            pixels,
        }
    }
}

/// A texture decoded from the title's manifest rather than uploaded by the
/// guest. The direct PR runner exposes these as pre-existing GL texture names
/// before the eApp starts issuing ordinary upload calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodedManifestTexture {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Rgba8>,
    pub format: TextureFormat,
}

/// Pull `Files[].Path` entries from an XML `Manifest.plist`, preserving the
/// document order used by the direct HLE runner. A full plist parser is not
/// needed for this narrow resource contract.
pub(crate) fn manifest_texture_paths(path: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut paths = Vec::new();
    let mut rest = text.as_str();
    while let Some(index) = rest.find("<key>Path</key>") {
        rest = &rest[index + 15..];
        let start = rest.find("<string>")?;
        let end = rest[start + 8..].find("</string>")?;
        paths.push(rest[start + 8..start + 8 + end].to_string());
        rest = &rest[start + 8 + end..];
    }
    (!paths.is_empty()).then_some(paths)
}

fn expand16(value: u16, rgb565: bool) -> Rgba8 {
    let (r, g, b) = if rgb565 {
        (
            ((value >> 11) & 0x1f) as u8,
            ((value >> 5) & 0x3f) as u8,
            (value & 0x1f) as u8,
        )
    } else {
        (
            ((value >> 10) & 0x1f) as u8,
            ((value >> 5) & 0x1f) as u8,
            (value & 0x1f) as u8,
        )
    };
    let green = if rgb565 { (g << 2) | (g >> 4) } else { (g << 3) | (g >> 2) };
    // The iPod resource formats use magenta as their transparent colour key.
    let alpha = if value == 0xf83e { 0 } else { 255 };
    Rgba8::rgba(
        (r << 3) | (r >> 2),
        green,
        (b << 3) | (b >> 2),
        alpha,
    )
}

/// Decode an uncompressed 16-bit TGA. Preserve the PR runner's row handling;
/// presentation flips, when needed, happen after rasterization.
fn decode_manifest_tga(data: &[u8]) -> Option<DecodedManifestTexture> {
    if data.len() < 18 || data[2] != 2 || data[16] != 16 {
        return None;
    }
    let width = u16::from_le_bytes([data[12], data[13]]) as usize;
    let height = u16::from_le_bytes([data[14], data[15]]) as usize;
    let top = data[17] & 0x20 != 0;
    if width == 0 || height == 0 || data.len() < 18 + width * height * 2 {
        return None;
    }
    let mut pixels = vec![Rgba8::rgba(0, 0, 0, 0); width * height];
    for y in 0..height {
        let source_y = if top { height - 1 - y } else { y };
        for x in 0..width {
            let offset = 18 + (source_y * width + x) * 2;
            pixels[y * width + x] = expand16(
                u16::from_le_bytes([data[offset], data[offset + 1]]),
                false,
            );
        }
    }
    Some(DecodedManifestTexture {
        width,
        height,
        pixels,
        // TGA's colour-keyed 5:5:5 payload is represented as decoded RGBA.
        format: TextureFormat::Rgba8888,
    })
}

/// Decode `.ipd`: width, height, type and RGB format, followed by RGB565
/// pixels. This is the image container used by Vortex and Cubis 2.
fn decode_manifest_ipd(data: &[u8]) -> Option<DecodedManifestTexture> {
    if data.len() < 16 {
        return None;
    }
    let width = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let height = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || data.len() < 16 + width * height * 2
    {
        return None;
    }
    let mut pixels = Vec::with_capacity(width * height);
    for index in 0..width * height {
        let offset = 16 + index * 2;
        pixels.push(expand16(
            u16::from_le_bytes([data[offset], data[offset + 1]]),
            true,
        ));
    }
    Some(DecodedManifestTexture {
        width,
        height,
        pixels,
        format: TextureFormat::Rgb565,
    })
}

/// Decode a `.pix` file. The extension is unusual, but the payload is a
/// Windows BMP. Tetris and Cubis 2 use 16/32-bit colour and 8-bit coverage
/// atlases in this form.
fn decode_manifest_bmp(data: &[u8]) -> Option<DecodedManifestTexture> {
    if data.len() < 54 || &data[0..2] != b"BM" {
        return None;
    }
    let read_u16 = |offset: usize| {
        data.get(offset..offset + 2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
    };
    let read_u32 = |offset: usize| {
        data.get(offset..offset + 4)
            .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    let data_offset = read_u32(10)? as usize;
    let dib_size = read_u32(14)? as usize;
    if dib_size < 40 {
        return None;
    }
    let raw_width = read_u32(18)? as i32;
    let raw_height = read_u32(22)? as i32;
    let bits_per_pixel = read_u16(28)?;
    let compression = read_u32(30)?;
    if raw_width <= 0 || raw_height == 0 {
        return None;
    }
    let width = raw_width as usize;
    let height = raw_height.unsigned_abs() as usize;
    let top_down = raw_height < 0;
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return None;
    }

    // BI_BITFIELDS masks follow the 40-byte base header. BI_RGB uses the
    // standard masks for the depths shipped by the games.
    let (red_mask, green_mask, blue_mask, alpha_mask) = match (compression, bits_per_pixel) {
        (3, _) => (
            read_u32(54)?,
            read_u32(58)?,
            read_u32(62)?,
            read_u32(66).unwrap_or(0),
        ),
        (0, 32) => (0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000),
        (0, 16) => (0x7c00, 0x03e0, 0x001f, 0x8000),
        (0, 24) | (0, 8) => (0, 0, 0, 0),
        _ => return None,
    };
    let channel = |value: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 255;
        }
        let shift = mask.trailing_zeros();
        let bits = mask.count_ones();
        let sample = (value & mask) >> shift;
        let max = if bits == 32 { u32::MAX } else { (1u32 << bits) - 1 };
        if bits == 8 {
            sample as u8
        } else {
            ((sample * 255 + max / 2) / max) as u8
        }
    };

    let palette_entries = if bits_per_pixel <= 8 {
        let used = read_u32(46).unwrap_or(0) as usize;
        if used == 0 {
            1usize << bits_per_pixel
        } else {
            used
        }
    } else {
        0
    };
    let palette_offset = 14 + dib_size;
    // `_a8` files carry a grayscale palette with zero alpha bytes. In that
    // case the byte index is coverage, and the draw colour supplies RGB.
    let alpha_ramp = bits_per_pixel == 8
        && palette_entries >= 2
        && (0..palette_entries).all(|index| {
            let offset = palette_offset + index * 4;
            data.get(offset..offset + 4).is_some_and(|entry| {
                entry[0] == entry[1]
                    && entry[1] == entry[2]
                    && entry[0] as usize == index
                    && entry[3] == 0
            })
        });

    let row_bytes = (width * bits_per_pixel as usize).div_ceil(8);
    let row_stride = row_bytes.div_ceil(4) * 4;
    if data_offset + row_stride * height > data.len() {
        return None;
    }
    let mut pixels = vec![Rgba8::rgba(0, 0, 0, 0); width * height];
    for y in 0..height {
        let source_y = if top_down { y } else { height - 1 - y };
        let row = data_offset + source_y * row_stride;
        for x in 0..width {
            pixels[y * width + x] = match bits_per_pixel {
                8 => {
                    let index = data[row + x] as usize;
                    if alpha_ramp {
                        Rgba8::rgba(255, 255, 255, index as u8)
                    } else {
                        let offset = palette_offset + index * 4;
                        match data.get(offset..offset + 4) {
                            Some(entry) => Rgba8::rgba(entry[2], entry[1], entry[0], 255),
                            None => Rgba8::rgba(0, 0, 0, 0),
                        }
                    }
                }
                16 => {
                    let offset = row + x * 2;
                    let value = u16::from_le_bytes([data[offset], data[offset + 1]]) as u32;
                    Rgba8::rgba(
                        channel(value, red_mask),
                        channel(value, green_mask),
                        channel(value, blue_mask),
                        channel(value, alpha_mask),
                    )
                }
                24 => {
                    let offset = row + x * 3;
                    Rgba8::rgba(data[offset + 2], data[offset + 1], data[offset], 255)
                }
                32 => {
                    let offset = row + x * 4;
                    let value = u32::from_le_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                    Rgba8::rgba(
                        channel(value, red_mask),
                        channel(value, green_mask),
                        channel(value, blue_mask),
                        channel(value, alpha_mask),
                    )
                }
                _ => return None,
            };
        }
    }
    Some(DecodedManifestTexture {
        width,
        height,
        pixels,
        format: if alpha_ramp {
            TextureFormat::A8
        } else if bits_per_pixel == 16 {
            TextureFormat::Rgba5551
        } else {
            TextureFormat::Rgba8888
        },
    })
}

/// Decode headerless RGB565 resources whose dimensions are unambiguous from
/// the byte count (a square or a 2:1 rectangle).
fn decode_manifest_raw_rgb565(data: &[u8]) -> Option<DecodedManifestTexture> {
    let pixels_count = data.len() / 2;
    if data.len() % 2 != 0 || pixels_count == 0 {
        return None;
    }
    let side = (pixels_count as f64).sqrt() as usize;
    let (width, height) = if side * side == pixels_count {
        (side, side)
    } else if pixels_count / 2 > 0
        && (pixels_count / 2) * 2 == pixels_count
        && side > 0
        && (side * 2) * (side / 2) == pixels_count
    {
        (side * 2, side / 2)
    } else {
        return None;
    };
    let mut pixels = Vec::with_capacity(width * height);
    for index in 0..width * height {
        let offset = index * 2;
        pixels.push(expand16(
            u16::from_le_bytes([data[offset], data[offset + 1]]),
            true,
        ));
    }
    Some(DecodedManifestTexture {
        width,
        height,
        pixels,
        format: TextureFormat::Rgb565,
    })
}

/// Decode one manifest-listed image using the same extension contract as the
/// PR direct runner. Unknown files are intentionally ignored.
pub(crate) fn decode_manifest_texture(path: &Path, data: &[u8]) -> Option<DecodedManifestTexture> {
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())?;
    match extension.as_str() {
        "tga" => decode_manifest_tga(data),
        "ipd" => decode_manifest_ipd(data),
        "bin" => decode_manifest_raw_rgb565(data),
        "pix" | "bmp" => decode_manifest_bmp(data),
        _ => None,
    }
}

pub fn decode_texture_pixels(
    raw: &[u8],
    width: usize,
    height: usize,
    format: TextureFormat,
    a8_tint: Rgba8,
) -> Vec<Rgba8> {
    let expected = match format {
        TextureFormat::Rgb565 | TextureFormat::Rgba5551 | TextureFormat::Rgba4444 => {
            width * height * 2
        }
        TextureFormat::Rgba8888 => width * height * 4,
        TextureFormat::LuminanceAlpha88 => width * height * 2,
        TextureFormat::A8 => width * height,
    };
    assert_eq!(raw.len(), expected);
    match format {
        TextureFormat::Rgb565 => raw
            .chunks_exact(2)
            .map(|chunk| {
                let px = u16::from_le_bytes([chunk[0], chunk[1]]);
                let r = ((px >> 11) & 0x1f) as u8;
                let g = ((px >> 5) & 0x3f) as u8;
                let b = (px & 0x1f) as u8;
                Rgba8::rgba(
                    (r as u16 * 255 / 31) as u8,
                    (g as u16 * 255 / 63) as u8,
                    (b as u16 * 255 / 31) as u8,
                    255,
                )
            })
            .collect(),
        TextureFormat::Rgba5551 => raw
            .chunks_exact(2)
            .map(|chunk| {
                let px = u16::from_le_bytes([chunk[0], chunk[1]]);
                let r = ((px >> 11) & 0x1f) as u8;
                let g = ((px >> 6) & 0x1f) as u8;
                let b = ((px >> 1) & 0x1f) as u8;
                let a = (px & 0x1) as u8;
                Rgba8::rgba(
                    (r as u16 * 255 / 31) as u8,
                    (g as u16 * 255 / 31) as u8,
                    (b as u16 * 255 / 31) as u8,
                    if a != 0 { 255 } else { 0 },
                )
            })
            .collect(),
        TextureFormat::Rgba4444 => raw
            .chunks_exact(2)
            .map(|chunk| {
                let px = u16::from_le_bytes([chunk[0], chunk[1]]);
                let r = ((px >> 12) & 0x0f) as u8;
                let g = ((px >> 8) & 0x0f) as u8;
                let b = ((px >> 4) & 0x0f) as u8;
                let a = (px & 0x0f) as u8;
                Rgba8::rgba(
                    (r as u16 * 255 / 15) as u8,
                    (g as u16 * 255 / 15) as u8,
                    (b as u16 * 255 / 15) as u8,
                    (a as u16 * 255 / 15) as u8,
                )
            })
            .collect(),
        TextureFormat::Rgba8888 => raw
            .chunks_exact(4)
            .map(|chunk| Rgba8::rgba(chunk[0], chunk[1], chunk[2], chunk[3]))
            .collect(),
        TextureFormat::LuminanceAlpha88 => raw
            .chunks_exact(2)
            .map(|chunk| Rgba8::rgba(chunk[0], chunk[0], chunk[0], chunk[1]))
            .collect(),
        TextureFormat::A8 => raw
            .iter()
            .map(|&alpha| Rgba8::rgba(a8_tint.r, a8_tint.g, a8_tint.b, alpha))
            .collect(),
    }
}

pub fn sample_nearest(texture: &Texture, u: f32, v: f32) -> Rgba8 {
    let x = u
        .floor()
        .clamp(0.0, (texture.width.saturating_sub(1)) as f32) as usize;
    let y = v
        .floor()
        .clamp(0.0, (texture.height.saturating_sub(1)) as f32) as usize;
    texture.pixels[y * texture.width + x]
}

/// Sample at texel centres using the filtering behavior observed in the PR
/// direct runner. RGB is interpolated in premultiplied-alpha space so keyed
/// transparent texels do not bleed their hidden colour into scaled edges.
pub fn sample_bilinear_premultiplied(texture: &Texture, u: f32, v: f32) -> Rgba8 {
    if texture.width == 0 || texture.height == 0 {
        return Rgba8::rgba(0, 0, 0, 0);
    }
    let sx = (u - 0.5).clamp(0.0, (texture.width - 1) as f32);
    let sy = (v - 0.5).clamp(0.0, (texture.height - 1) as f32);
    let x0 = sx.floor() as usize;
    let y0 = sy.floor() as usize;
    let x1 = (x0 + 1).min(texture.width - 1);
    let y1 = (y0 + 1).min(texture.height - 1);
    let dx = sx - x0 as f32;
    let dy = sy - y0 as f32;
    let weights = [
        ((1.0 - dx) * (1.0 - dy), x0, y0),
        (dx * (1.0 - dy), x1, y0),
        ((1.0 - dx) * dy, x0, y1),
        (dx * dy, x1, y1),
    ];
    let mut alpha_sum = 0.0;
    let mut channel_sum = [0.0; 3];
    for (weight, x, y) in weights {
        let pixel = texture.pixels[y * texture.width + x];
        let alpha = pixel.a as f32;
        alpha_sum += weight * alpha;
        for (channel, sum) in channel_sum.iter_mut().enumerate() {
            let value = [pixel.r, pixel.g, pixel.b][channel] as f32;
            *sum += weight * alpha * value;
        }
    }
    let alpha = alpha_sum.round().clamp(0.0, 255.0) as u8;
    if alpha_sum <= 0.0 {
        return Rgba8::rgba(0, 0, 0, alpha);
    }
    let channel = |sum: f32| (sum / alpha_sum).round().clamp(0.0, 255.0) as u8;
    Rgba8::rgba(
        channel(channel_sum[0]),
        channel(channel_sum[1]),
        channel(channel_sum[2]),
        alpha,
    )
}

fn one_to_one_mapping(verts: &[(f32, f32, f32, f32); 3]) -> bool {
    let extent = |index: usize| {
        let (lo, hi): (f32, f32) = verts
            .iter()
            .map(|vertex| match index {
                0 => vertex.0,
                1 => vertex.1,
                2 => vertex.2,
                3 => vertex.3,
                _ => unreachable!("vertex extent index"),
            })
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), value| {
                (lo.min(value), hi.max(value))
            });
        hi - lo
    };
    let (position_w, position_h) = (extent(0), extent(1));
    let (uv_w, uv_h) = (extent(2), extent(3));
    position_w > 0.5
        && position_h > 0.5
        && (uv_w / position_w - 1.0).abs() < 0.02
        && (uv_h / position_h - 1.0).abs() < 0.02
}

pub fn blend_src_over(dst: Rgba8, src: Rgba8) -> Rgba8 {
    let sa = src.a as u32;
    let da = dst.a as u32;
    let inv_sa = 255 - sa;
    let out_a = sa + (da * inv_sa + 127) / 255;
    if out_a == 0 {
        return Rgba8::rgba(0, 0, 0, 0);
    }
    let blend = |src_c: u8, dst_c: u8| -> u8 {
        let src_p = src_c as u32 * sa;
        let dst_p = dst_c as u32 * da * inv_sa / 255;
        ((src_p + dst_p + out_a / 2) / out_a) as u8
    };
    Rgba8::rgba(
        blend(src.r, dst.r),
        blend(src.g, dst.g),
        blend(src.b, dst.b),
        out_a as u8,
    )
}

fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

fn is_top_left(ax: f32, ay: f32, bx: f32, by: f32) -> bool {
    let dy = by - ay;
    let dx = bx - ax;
    dy > 0.0 || (dy == 0.0 && dx < 0.0)
}

fn modulate(src: Rgba8, tint: Rgba8) -> Rgba8 {
    let mul = |a: u8, b: u8| -> u8 { ((a as u16 * b as u16 + 127) / 255) as u8 };
    Rgba8::rgba(
        mul(src.r, tint.r),
        mul(src.g, tint.g),
        mul(src.b, tint.b),
        mul(src.a, tint.a),
    )
}

pub fn rasterize_triangle_tinted(
    fb: &mut [Rgba8],
    fb_width: usize,
    fb_height: usize,
    tex: &Texture,
    verts: &[(f32, f32, f32, f32); 3],
    tint: Rgba8,
) -> u64 {
    rasterize_triangle_tinted_with_vertex_colors(fb, fb_width, fb_height, tex, verts, tint, None)
}

/// Rasterize a textured triangle while optionally interpolating a primary
/// colour supplied by the guest's GL colour array. The direct PR runner keeps
/// this colour separate from the constant colour register; Vortex uses it to
/// turn one greyscale ring atlas into the coloured attract-mode tiles.
pub fn rasterize_triangle_tinted_with_vertex_colors(
    fb: &mut [Rgba8],
    fb_width: usize,
    fb_height: usize,
    tex: &Texture,
    verts: &[(f32, f32, f32, f32); 3],
    tint: Rgba8,
    vertex_colors: Option<&[[f32; 4]; 3]>,
) -> u64 {
    let mut v = *verts;
    let mut colors = vertex_colors.copied();
    if edge(v[0].0, v[0].1, v[1].0, v[1].1, v[2].0, v[2].1) < 0.0 {
        v.swap(1, 2);
        if let Some(colors) = colors.as_mut() {
            colors.swap(1, 2);
        }
    }

    let min_x = v
        .iter()
        .map(|p| p.0)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as i32;
    let min_y = v
        .iter()
        .map(|p| p.1)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as i32;
    let max_x = v
        .iter()
        .map(|p| p.0)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min((fb_width - 1) as f32) as i32;
    let max_y = v
        .iter()
        .map(|p| p.1)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min((fb_height - 1) as f32) as i32;

    let area = edge(v[0].0, v[0].1, v[1].0, v[1].1, v[2].0, v[2].1);
    if area == 0.0 {
        return 0;
    }

    let tl01 = is_top_left(v[0].0, v[0].1, v[1].0, v[1].1);
    let tl12 = is_top_left(v[1].0, v[1].1, v[2].0, v[2].1);
    let tl20 = is_top_left(v[2].0, v[2].1, v[0].0, v[0].1);

    let mut coverage = 0u64;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let e0 = edge(v[1].0, v[1].1, v[2].0, v[2].1, px, py);
            let e1 = edge(v[2].0, v[2].1, v[0].0, v[0].1, px, py);
            let e2 = edge(v[0].0, v[0].1, v[1].0, v[1].1, px, py);
            let inside = (e0 > 0.0 || (e0 == 0.0 && tl12))
                && (e1 > 0.0 || (e1 == 0.0 && tl20))
                && (e2 > 0.0 || (e2 == 0.0 && tl01));
            if !inside {
                continue;
            }
            let inv_area = 1.0 / area;
            let w0 = e0 * inv_area;
            let w1 = e1 * inv_area;
            let w2 = e2 * inv_area;
            let u = v[0].2 * w0 + v[1].2 * w1 + v[2].2 * w2;
            let vv = v[0].3 * w0 + v[1].3 * w1 + v[2].3 * w2;
            let sample = if one_to_one_mapping(&v) {
                sample_nearest(tex, u, vv)
            } else {
                sample_bilinear_premultiplied(tex, u, vv)
            };
            let primary = colors.map(|colors| {
                let mut out = [0.0; 4];
                for component in 0..4 {
                    out[component] = (colors[0][component] * w0
                        + colors[1][component] * w1
                        + colors[2][component] * w2)
                        .clamp(0.0, 1.0);
                }
                out
            });
            let primary_modulated = primary.map(|primary| {
                Rgba8::rgba(
                    (primary[0] * 255.0).round() as u8,
                    (primary[1] * 255.0).round() as u8,
                    (primary[2] * 255.0).round() as u8,
                    (primary[3] * 255.0).round() as u8,
                )
            });
            let src = match primary_modulated {
                Some(primary) => modulate(modulate(sample, primary), tint),
                None => modulate(sample, tint),
            };
            let idx = y as usize * fb_width + x as usize;
            fb[idx] = blend_src_over(fb[idx], src);
            coverage += 1;
        }
    }

    coverage
}

pub fn rasterize_solid_quad(
    fb: &mut [Rgba8],
    fb_width: usize,
    fb_height: usize,
    color: Rgba8,
    positions: &[(f32, f32); 4],
) -> u64 {
    if color.a == 0 {
        return 0;
    }
    let min_x = positions
        .iter()
        .map(|p| p.0)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as i32;
    let min_y = positions
        .iter()
        .map(|p| p.1)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as i32;
    let max_x = positions
        .iter()
        .map(|p| p.0)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min((fb_width - 1) as f32) as i32;
    let max_y = positions
        .iter()
        .map(|p| p.1)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min((fb_height - 1) as f32) as i32;
    if min_x > max_x || min_y > max_y {
        return 0;
    }

    let mut coverage = 0u64;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let idx = y as usize * fb_width + x as usize;
            fb[idx] = blend_src_over(fb[idx], color);
            coverage += 1;
        }
    }
    coverage
}

pub fn rasterize_quad_tinted(
    fb: &mut [Rgba8],
    fb_width: usize,
    fb_height: usize,
    tex: &Texture,
    positions: &[(f32, f32); 4],
    uvs: &[(f32, f32); 4],
    tint: Rgba8,
) -> u64 {
    rasterize_quad_tinted_with_vertex_colors(fb, fb_width, fb_height, tex, positions, uvs, tint, None)
}

/// Rasterize a textured quad with an optional four-vertex primary colour
/// array. The two triangles share the same colour values so interpolation is
/// continuous across the quad diagonal.
pub fn rasterize_quad_tinted_with_vertex_colors(
    fb: &mut [Rgba8],
    fb_width: usize,
    fb_height: usize,
    tex: &Texture,
    positions: &[(f32, f32); 4],
    uvs: &[(f32, f32); 4],
    tint: Rgba8,
    vertex_colors: Option<&[[f32; 4]; 4]>,
) -> u64 {
    let tri0 = [
        (positions[0].0, positions[0].1, uvs[0].0, uvs[0].1),
        (positions[1].0, positions[1].1, uvs[1].0, uvs[1].1),
        (positions[2].0, positions[2].1, uvs[2].0, uvs[2].1),
    ];
    let tri1 = [
        (positions[0].0, positions[0].1, uvs[0].0, uvs[0].1),
        (positions[2].0, positions[2].1, uvs[2].0, uvs[2].1),
        (positions[3].0, positions[3].1, uvs[3].0, uvs[3].1),
    ];
    let tri0_colors = vertex_colors.map(|colors| [colors[0], colors[1], colors[2]]);
    let tri1_colors = vertex_colors.map(|colors| [colors[0], colors[2], colors[3]]);
    let coverage0 = rasterize_triangle_tinted_with_vertex_colors(
        fb,
        fb_width,
        fb_height,
        tex,
        &tri0,
        tint,
        tri0_colors.as_ref(),
    );
    let coverage1 = rasterize_triangle_tinted_with_vertex_colors(
        fb,
        fb_width,
        fb_height,
        tex,
        &tri1,
        tint,
        tri1_colors.as_ref(),
    );
    coverage0 + coverage1
}

pub fn rasterize_triangle(
    fb: &mut [Rgba8],
    fb_width: usize,
    fb_height: usize,
    tex: &Texture,
    verts: &[(f32, f32, f32, f32); 3],
) -> u64 {
    rasterize_triangle_tinted(
        fb,
        fb_width,
        fb_height,
        tex,
        verts,
        Rgba8::rgba(255, 255, 255, 255),
    )
}

pub fn rasterize_quad(
    fb: &mut [Rgba8],
    fb_width: usize,
    fb_height: usize,
    tex: &Texture,
    positions: &[(f32, f32); 4],
    uvs: &[(f32, f32); 4],
) -> u64 {
    rasterize_quad_tinted(
        fb,
        fb_width,
        fb_height,
        tex,
        positions,
        uvs,
        Rgba8::rgba(255, 255, 255, 255),
    )
}

pub fn framebuffer_hash(fb: &[Rgba8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for px in fb {
        for b in [px.r, px.g, px.b, px.a] {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

pub fn framebuffer_to_ppm(path: &std::path::Path, fb: &[Rgba8], width: usize, height: usize) {
    let mut out = Vec::with_capacity(width * height * 3 + 64);
    out.extend_from_slice(format!("P6\n{} {}\n255\n", width, height).as_bytes());
    for px in fb {
        out.push(px.r);
        out.push(px.g);
        out.push(px.b);
    }
    std::fs::write(path, out).expect("write ppm");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rgba8888_preserves_channel_order() {
        let tex = Texture::from_bytes(
            &[0x11, 0x22, 0x33, 0x44],
            1,
            1,
            TextureFormat::Rgba8888,
            Rgba8::rgba(255, 255, 255, 255),
        );
        assert_eq!(tex.pixels[0], Rgba8::rgba(0x11, 0x22, 0x33, 0x44));
    }

    #[test]
    fn rasterize_quad_tinted_modulates_rgb_and_alpha() {
        let tex = Texture::from_bytes(
            &[0xff],
            1,
            1,
            TextureFormat::A8,
            Rgba8::rgba(255, 255, 255, 255),
        );
        let mut fb = vec![Rgba8::rgba(0, 0, 0, 0)];
        let cov = rasterize_quad_tinted(
            &mut fb,
            1,
            1,
            &tex,
            &[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)],
            &[(0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)],
            Rgba8::rgba(64, 128, 255, 128),
        );
        assert_eq!(cov, 1);
        assert_eq!(fb[0], Rgba8::rgba(64, 128, 255, 128));
    }

    #[test]
    fn bilinear_sampling_weights_rgb_by_alpha() {
        let tex = Texture::from_bytes(
            &[255, 0, 0, 255, 0, 255, 0, 0],
            2,
            1,
            TextureFormat::Rgba8888,
            Rgba8::rgba(255, 255, 255, 255),
        );
        assert_eq!(
            sample_bilinear_premultiplied(&tex, 0.5, 0.5),
            Rgba8::rgba(255, 0, 0, 255)
        );
        assert_eq!(
            sample_bilinear_premultiplied(&tex, 1.25, 0.5),
            Rgba8::rgba(255, 0, 0, 64)
        );
    }

    #[test]
    fn scaled_quad_uses_bilinear_but_one_to_one_stays_nearest() {
        let tex = Texture::from_bytes(
            &[255, 0, 0, 255, 0, 0, 255, 255],
            2,
            1,
            TextureFormat::Rgba8888,
            Rgba8::rgba(255, 255, 255, 255),
        );
        let scaled = [
            (0.0, 0.0, 0.0, 0.0),
            (4.0, 0.0, 2.0, 0.0),
            (4.0, 1.0, 2.0, 1.0),
        ];
        let one_to_one = [
            (0.0, 0.0, 0.0, 0.0),
            (2.0, 0.0, 2.0, 0.0),
            (2.0, 1.0, 2.0, 1.0),
        ];
        assert!(!one_to_one_mapping(&scaled));
        assert!(one_to_one_mapping(&one_to_one));
        assert_eq!(sample_nearest(&tex, 0.75, 0.5), Rgba8::rgba(255, 0, 0, 255));
        assert_eq!(
            sample_bilinear_premultiplied(&tex, 0.75, 0.5),
            Rgba8::rgba(191, 0, 64, 255)
        );
    }

    #[test]
    fn manifest_ipd_decodes_rgb565_pixels() {
        let mut data = vec![0u8; 18];
        data[0..4].copy_from_slice(&1u32.to_le_bytes());
        data[4..8].copy_from_slice(&1u32.to_le_bytes());
        data[16..18].copy_from_slice(&0xf800u16.to_le_bytes());
        let decoded = decode_manifest_ipd(&data).expect("valid ipd");
        assert_eq!((decoded.width, decoded.height), (1, 1));
        assert_eq!(decoded.format, TextureFormat::Rgb565);
        assert_eq!(decoded.pixels[0], Rgba8::rgba(255, 0, 0, 255));
    }

    #[test]
    fn manifest_tga_applies_magenta_colour_key() {
        let mut data = vec![0u8; 20];
        data[2] = 2;
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16] = 16;
        data[18..20].copy_from_slice(&0xf83eu16.to_le_bytes());
        let decoded = decode_manifest_tga(&data).expect("valid tga");
        assert_eq!(decoded.pixels[0].a, 0);
    }

    #[test]
    fn manifest_bmp_recognizes_a8_coverage_palette() {
        let mut data = vec![0u8; 66];
        data[0..2].copy_from_slice(b"BM");
        data[10..14].copy_from_slice(&62u32.to_le_bytes());
        data[14..18].copy_from_slice(&40u32.to_le_bytes());
        data[18..22].copy_from_slice(&1i32.to_le_bytes());
        data[22..26].copy_from_slice(&1i32.to_le_bytes());
        data[26..28].copy_from_slice(&1u16.to_le_bytes());
        data[28..30].copy_from_slice(&8u16.to_le_bytes());
        data[46..50].copy_from_slice(&2u32.to_le_bytes());
        data[54..58].copy_from_slice(&[0, 0, 0, 0]);
        data[58..62].copy_from_slice(&[1, 1, 1, 0]);
        data[62] = 1;
        let decoded = decode_manifest_bmp(&data).expect("valid bmp");
        assert_eq!(decoded.format, TextureFormat::A8);
        assert_eq!(decoded.pixels[0], Rgba8::rgba(255, 255, 255, 1));
    }
}
