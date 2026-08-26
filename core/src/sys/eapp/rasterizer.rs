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
    let mut v = *verts;
    if edge(v[0].0, v[0].1, v[1].0, v[1].1, v[2].0, v[2].1) < 0.0 {
        v.swap(1, 2);
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
            let src = modulate(sample_nearest(tex, u, vv), tint);
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
    rasterize_triangle_tinted(fb, fb_width, fb_height, tex, &tri0, tint)
        + rasterize_triangle_tinted(fb, fb_width, fb_height, tex, &tri1, tint)
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
}
