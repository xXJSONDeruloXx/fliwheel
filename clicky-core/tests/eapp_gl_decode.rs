use clicky_core::sys::eapp::{
    blend_src_over, decode_fixed_16_16, first_frame, framebuffer_hash, framebuffer_to_ppm,
    rasterize_quad, register, sample_nearest, stack_word, texture_upload_candidates,
    words_from_snapshot, GlImportRecord, GlTraceFixture, Rgba8, Texture, TextureFormat,
    TextureUploadCandidate,
};

fn load_fixture() -> GlTraceFixture {
    serde_json::from_str(include_str!("fixtures/eapp/tetris_gl_trace.json"))
        .expect("valid trace fixture json")
}

fn seq_record<'a>(
    frame: &'a clicky_core::sys::eapp::GlFrameRecord,
    seq: u64,
) -> &'a GlImportRecord {
    frame
        .records
        .iter()
        .find(|record| record.seq_in_frame == seq)
        .unwrap_or_else(|| panic!("missing seq_in_frame {}", seq))
}

fn words_as_positions_xyzw(words: &[u32]) -> Vec<(f32, f32, f32, f32)> {
    words
        .chunks_exact(4)
        .map(|chunk| {
            (
                decode_fixed_16_16(chunk[0]),
                decode_fixed_16_16(chunk[1]),
                decode_fixed_16_16(chunk[2]),
                decode_fixed_16_16(chunk[3]),
            )
        })
        .collect()
}

fn words_as_pairs(words: &[u32]) -> Vec<(f32, f32)> {
    words
        .chunks_exact(2)
        .map(|chunk| (decode_fixed_16_16(chunk[0]), decode_fixed_16_16(chunk[1])))
        .collect()
}

fn make_raw_rgb565(width: usize, height: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity(width * height * 2);
    for y in 0..height {
        for x in 0..width {
            let r = ((x as u16) * 31 / (width as u16 - 1)) & 0x1f;
            let g = ((y as u16) * 63 / (height as u16 - 1)) & 0x3f;
            let b = (((x + y) as u16) * 31 / ((width + height - 2) as u16)) & 0x1f;
            let px = (r << 11) | (g << 5) | b;
            raw.extend_from_slice(&px.to_le_bytes());
        }
    }
    raw
}

fn make_raw_rgba5551(width: usize, height: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity(width * height * 2);
    for y in 0..height {
        for x in 0..width {
            let r = ((x as u16) * 31 / (width as u16 - 1)) & 0x1f;
            let g = ((y as u16) * 31 / (height as u16 - 1)) & 0x1f;
            let b = (((x + y) as u16) * 31 / ((width + height - 2) as u16)) & 0x1f;
            let a = if ((x / 4) + (y / 4)) % 2 == 0 { 1 } else { 0 };
            let px = (r << 11) | (g << 6) | (b << 1) | a;
            raw.extend_from_slice(&px.to_le_bytes());
        }
    }
    raw
}

fn make_raw_rgba4444(width: usize, height: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity(width * height * 2);
    for y in 0..height {
        for x in 0..width {
            let r = ((x as u16) * 15 / (width as u16 - 1)) & 0x0f;
            let g = ((y as u16) * 15 / (height as u16 - 1)) & 0x0f;
            let b = (((x + y) as u16) * 15 / ((width + height - 2) as u16)) & 0x0f;
            let a = (((x ^ y) as u16) * 15 / ((width.max(height) - 1) as u16)) & 0x0f;
            let px = (r << 12) | (g << 8) | (b << 4) | a;
            raw.extend_from_slice(&px.to_le_bytes());
        }
    }
    raw
}

fn make_raw_a8(width: usize, height: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let alpha = if width == 1 && height == 1 {
                128
            } else {
                ((x * 255 / (width - 1)) ^ (y * 255 / (height - 1))) as u8
            };
            raw.push(alpha);
        }
    }
    raw
}

fn make_texture(format: TextureFormat, width: usize, height: usize, raw: Vec<u8>) -> Texture {
    Texture::from_bytes(&raw, width, height, format, Rgba8::rgba(255, 255, 255, 255))
}

fn replay_frame4() -> (Vec<Rgba8>, Vec<DrawReplay>) {
    let fixture = load_fixture();
    let frame4 = first_frame(&fixture, 4).expect("steady-state frame");

    let draws = vec![
        DrawReplay::from_frame(
            frame4,
            DrawPlan {
                seqs_169: &[3, 4, 5],
                seq_159: 6,
                seq_pos: 7,
                seq_uv: 10,
                seq_aux: Some(11),
                proposed_texture: ProposedTexture::resolved(
                    "screenBG_565.pix",
                    320,
                    240,
                    TextureFormat::Rgb565,
                    0.93,
                ),
                texture: make_texture(TextureFormat::Rgb565, 320, 240, make_raw_rgb565(320, 240)),
            },
        ),
        DrawReplay::from_frame(
            frame4,
            DrawPlan {
                seqs_169: &[18, 19],
                seq_159: 20,
                seq_pos: 21,
                seq_uv: 24,
                seq_aux: None,
                proposed_texture: ProposedTexture::resolved(
                    "tetrisLogo_4444.pix",
                    250,
                    162,
                    TextureFormat::Rgba4444,
                    0.84,
                ),
                texture: make_texture(
                    TextureFormat::Rgba4444,
                    250,
                    162,
                    make_raw_rgba4444(250, 162),
                ),
            },
        ),
        DrawReplay::from_frame(
            frame4,
            DrawPlan {
                seqs_169: &[29, 30],
                seq_159: 31,
                seq_pos: 32,
                seq_uv: 35,
                seq_aux: None,
                proposed_texture: ProposedTexture::resolved(
                    "eaLogo_5551.pix",
                    50,
                    50,
                    TextureFormat::Rgba5551,
                    0.87,
                ),
                texture: make_texture(TextureFormat::Rgba5551, 50, 50, make_raw_rgba5551(50, 50)),
            },
        ),
        DrawReplay::from_frame(
            frame4,
            DrawPlan {
                seqs_169: &[40],
                seq_159: 41,
                seq_pos: 42,
                seq_uv: 44,
                seq_aux: None,
                proposed_texture: ProposedTexture::unresolved(
                    "generated placeholder",
                    "handle 3 / full-screen overlay",
                    TextureFormat::A8,
                    0.28,
                ),
                texture: make_texture(TextureFormat::A8, 1, 1, make_raw_a8(1, 1)),
            },
        ),
    ];

    let mut fb = vec![Rgba8::rgba(0, 0, 0, 0); 320 * 240];
    for draw in &draws {
        draw.rasterize(&mut fb);
    }
    (fb, draws)
}

#[derive(Debug, Clone)]
struct DrawPlan {
    seqs_169: &'static [u64],
    seq_159: u64,
    seq_pos: u64,
    seq_uv: u64,
    seq_aux: Option<u64>,
    proposed_texture: ProposedTexture,
    texture: Texture,
}

#[derive(Debug, Clone)]
struct DrawReplay {
    ordinal159_handle: u32,
    state_ptr: u32,
    translation: (f32, f32),
    local_positions: [(f32, f32); 4],
    uv_or_aux: Vec<(f32, f32)>,
    aux_array: Option<Vec<(f32, f32)>>,
    screen_bounds: (f32, f32, f32, f32),
    proposed_texture: ProposedTexture,
    texture: Texture,
    coverage: u64,
}

impl DrawReplay {
    fn from_frame(frame: &clicky_core::sys::eapp::GlFrameRecord, plan: DrawPlan) -> Self {
        let mut tx = 0.0f32;
        let mut ty = 0.0f32;
        for seq in plan.seqs_169 {
            let record = seq_record(frame, *seq);
            tx += f32::from_bits(register(record, "r1").unwrap().value);
            ty += f32::from_bits(register(record, "r2").unwrap().value);
        }

        let record_159 = seq_record(frame, plan.seq_159);
        let ordinal159_handle = register(record_159, "r0").unwrap().value;
        let state_ptr = register(record_159, "r1").unwrap().value;

        let pos_words = stack_word(seq_record(frame, plan.seq_pos), 0x04)
            .and_then(|word| word.snapshot.as_ref())
            .expect("position snapshot");
        let local_positions = {
            let points = words_as_positions_xyzw(&words_from_snapshot(pos_words));
            [
                (points[0].0 + tx, points[0].1 + ty),
                (points[1].0 + tx, points[1].1 + ty),
                (points[2].0 + tx, points[2].1 + ty),
                (points[3].0 + tx, points[3].1 + ty),
            ]
        };

        let uv_words = stack_word(seq_record(frame, plan.seq_uv), 0x04)
            .and_then(|word| word.snapshot.as_ref())
            .expect("uv snapshot");
        let uv_or_aux = words_as_pairs(&words_from_snapshot(uv_words));
        let aux_array = plan.seq_aux.map(|seq| {
            let aux_words = stack_word(seq_record(frame, seq), 0x04)
                .and_then(|word| word.snapshot.as_ref())
                .expect("aux snapshot");
            words_as_pairs(&words_from_snapshot(aux_words))
        });

        let screen_bounds = local_positions.iter().fold(
            (
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ),
            |acc, (x, y)| (acc.0.min(*x), acc.1.min(*y), acc.2.max(*x), acc.3.max(*y)),
        );

        let mut fb = vec![Rgba8::rgba(0, 0, 0, 0); 320 * 240];
        let coverage = rasterize_quad(
            &mut fb,
            320,
            240,
            &plan.texture,
            &local_positions,
            &[uv_or_aux[0], uv_or_aux[1], uv_or_aux[2], uv_or_aux[3]],
        );

        Self {
            ordinal159_handle,
            state_ptr,
            translation: (tx, ty),
            local_positions,
            uv_or_aux,
            aux_array,
            screen_bounds,
            proposed_texture: plan.proposed_texture,
            texture: plan.texture,
            coverage,
        }
    }

    fn rasterize(&self, fb: &mut [Rgba8]) -> u64 {
        let positions = self.local_positions;
        let uvs = [
            self.uv_or_aux[0],
            self.uv_or_aux[1],
            self.uv_or_aux[2],
            self.uv_or_aux[3],
        ];
        rasterize_quad(fb, 320, 240, &self.texture, &positions, &uvs)
    }
}

#[derive(Debug, Clone)]
struct ProposedTexture {
    label: &'static str,
    kind: &'static str,
    width: Option<usize>,
    height: Option<usize>,
    format: TextureFormat,
    confidence: f32,
    unresolved_note: Option<&'static str>,
}

impl ProposedTexture {
    fn resolved(
        label: &'static str,
        width: usize,
        height: usize,
        format: TextureFormat,
        confidence: f32,
    ) -> Self {
        Self {
            label,
            kind: "candidate",
            width: Some(width),
            height: Some(height),
            format,
            confidence,
            unresolved_note: None,
        }
    }

    fn unresolved(
        label: &'static str,
        note: &'static str,
        format: TextureFormat,
        confidence: f32,
    ) -> Self {
        Self {
            label,
            kind: "unresolved",
            width: None,
            height: None,
            format,
            confidence,
            unresolved_note: Some(note),
        }
    }
}

#[test]
fn fixed_16_16_decodes_signed_values() {
    assert_eq!(decode_fixed_16_16(0x0001_0000), 1.0);
    assert_eq!(decode_fixed_16_16(0x0000_8000), 0.5);
    assert_eq!(decode_fixed_16_16(0xffff_8000), -0.5);
    assert_eq!(decode_fixed_16_16(0x00ef_8000), 239.5);
}

#[test]
fn decodes_frame4_draw_stream_and_associations() {
    let fixture = load_fixture();
    let frame4 = first_frame(&fixture, 4).expect("steady-state frame");

    let draws = [
        (
            6u64,
            320u32,
            240u32,
            TextureFormat::Rgb565,
            0.93f32,
            "screenBG_565.pix",
            false,
        ),
        (
            20u64,
            250u32,
            162u32,
            TextureFormat::Rgba4444,
            0.84f32,
            "tetrisLogo_4444.pix",
            false,
        ),
        (
            31u64,
            50u32,
            50u32,
            TextureFormat::Rgba5551,
            0.87f32,
            "eaLogo_5551.pix",
            false,
        ),
        (
            41u64,
            1u32,
            1u32,
            TextureFormat::A8,
            0.28f32,
            "generated placeholder",
            true,
        ),
    ];

    let summaries = replay_frame4().1;
    assert_eq!(summaries.len(), 4);

    for (idx, (summary, (seq_159, width, height, format, confidence, label, unresolved))) in
        summaries.iter().zip(draws.iter()).enumerate()
    {
        assert!(summary.ordinal159_handle > 0);
        if idx == 0 {
            assert!(summary.aux_array.is_some());
        } else {
            assert!(summary.aux_array.is_none());
        }
        assert!(summary.state_ptr > 0);
        assert_eq!(
            summary.proposed_texture.kind,
            if *unresolved {
                "unresolved"
            } else {
                "candidate"
            }
        );
        assert_eq!(summary.proposed_texture.format, *format);
        assert!((summary.proposed_texture.confidence - confidence).abs() < 0.001);
        assert_eq!(summary.proposed_texture.label, *label);
        if *unresolved {
            assert_eq!(summary.proposed_texture.width, None);
            assert_eq!(summary.proposed_texture.height, None);
            assert_eq!(
                summary.proposed_texture.unresolved_note,
                Some("handle 3 / full-screen overlay")
            );
        } else {
            assert_eq!(summary.proposed_texture.width, Some(*width as usize));
            assert_eq!(summary.proposed_texture.height, Some(*height as usize));
            assert_eq!(summary.proposed_texture.unresolved_note, None);
        }
        assert!(summary.coverage > 0);
        assert!(summary.translation.0.is_finite());
        assert!(summary.translation.1.is_finite());
        assert!(summary.screen_bounds.0.is_finite());
        assert!(summary.screen_bounds.1.is_finite());
        assert!(summary.screen_bounds.2.is_finite());
        assert!(summary.screen_bounds.3.is_finite());
        let record = seq_record(frame4, *seq_159);
        assert_eq!(
            summary.ordinal159_handle,
            register(record, "r0").unwrap().value
        );
    }
}

#[test]
fn sample_nearest_matches_floor_and_clamp_capture_coordinates() {
    let tex = Texture::from_bytes(
        &[
            0x00, 0xf8, // top-left red
            0xe0, 0x07, // top-right green
            0x1f, 0x00, // bottom-left blue
            0xff, 0xff, // bottom-right white
        ],
        2,
        2,
        TextureFormat::Rgb565,
        Rgba8::rgba(255, 255, 255, 255),
    );
    assert_eq!(sample_nearest(&tex, 0.5, 0.5), Rgba8::rgba(255, 0, 0, 255));
    assert_eq!(sample_nearest(&tex, 1.5, 0.5), Rgba8::rgba(0, 255, 0, 255));
    assert_eq!(sample_nearest(&tex, 0.5, 1.5), Rgba8::rgba(0, 0, 255, 255));
    assert_eq!(
        sample_nearest(&tex, 1.5, 1.5),
        Rgba8::rgba(255, 255, 255, 255)
    );
    assert_eq!(
        sample_nearest(&tex, -0.5, -0.5),
        Rgba8::rgba(255, 0, 0, 255)
    );
}

#[test]
fn rasterizer_supports_formats_alpha_clipping_and_winding() {
    let transparent = Texture::from_bytes(
        &[0x00],
        1,
        1,
        TextureFormat::A8,
        Rgba8::rgba(255, 0, 0, 255),
    );
    let opaque_red = Texture::from_bytes(
        &[0xff],
        1,
        1,
        TextureFormat::A8,
        Rgba8::rgba(255, 0, 0, 255),
    );
    let rgba4444 = Texture::from_bytes(
        &[0x08, 0xf0],
        1,
        1,
        TextureFormat::Rgba4444,
        Rgba8::rgba(255, 255, 255, 255),
    );
    let rgba5551 = Texture::from_bytes(
        &[0x01, 0xf8],
        1,
        1,
        TextureFormat::Rgba5551,
        Rgba8::rgba(255, 255, 255, 255),
    );
    let rgb565 = Texture::from_bytes(
        &[0x1f, 0x00],
        1,
        1,
        TextureFormat::Rgb565,
        Rgba8::rgba(255, 255, 255, 255),
    );

    assert_eq!(
        blend_src_over(Rgba8::rgba(10, 20, 30, 255), Rgba8::rgba(0, 0, 0, 0)),
        Rgba8::rgba(10, 20, 30, 255)
    );
    assert_eq!(
        blend_src_over(Rgba8::rgba(10, 20, 30, 255), Rgba8::rgba(200, 10, 50, 255)),
        Rgba8::rgba(200, 10, 50, 255)
    );

    let mut fb = vec![Rgba8::rgba(0, 0, 0, 0); 4 * 4];
    let quad = [(0.0, 0.0), (0.0, 2.0), (2.0, 2.0), (2.0, 0.0)];
    let uvs = [(0.5, 0.5), (0.5, 0.5), (0.5, 0.5), (0.5, 0.5)];
    let cov = rasterize_quad(&mut fb, 4, 4, &opaque_red, &quad, &uvs);
    assert_eq!(cov, 4);
    assert_eq!(fb[0], Rgba8::rgba(255, 0, 0, 255));

    let mut fb = vec![Rgba8::rgba(0, 0, 255, 255); 2 * 2];
    let quad = [(-1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (1.0, -1.0)];
    let uvs = [(0.5, 0.5), (0.5, 0.5), (0.5, 0.5), (0.5, 0.5)];
    let cov = rasterize_quad(&mut fb, 2, 2, &transparent, &quad, &uvs);
    assert_eq!(cov, 1);
    assert_eq!(fb[0], Rgba8::rgba(0, 0, 255, 255));

    let mut fb = vec![Rgba8::rgba(0, 0, 0, 0); 2 * 2];
    let quad = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)];
    let uvs = [(0.5, 0.5), (0.5, 0.5), (0.5, 0.5), (0.5, 0.5)];
    let cov_rgba4444 = rasterize_quad(&mut fb, 2, 2, &rgba4444, &quad, &uvs);
    assert_eq!(cov_rgba4444, 1);
    assert_eq!(rgba4444.pixels[0].a, 136);
    assert_eq!(fb[0], rgba4444.pixels[0]);

    let mut fb_blend = vec![Rgba8::rgba(0, 0, 255, 255); 2 * 2];
    let cov_rgba4444_blend = rasterize_quad(&mut fb_blend, 2, 2, &rgba4444, &quad, &uvs);
    assert_eq!(cov_rgba4444_blend, 1);
    assert_eq!(
        fb_blend[0],
        blend_src_over(Rgba8::rgba(0, 0, 255, 255), rgba4444.pixels[0])
    );

    let mut fb = vec![Rgba8::rgba(0, 0, 0, 0); 2 * 2];
    let cov_rgba5551 = rasterize_quad(&mut fb, 2, 2, &rgba5551, &quad, &uvs);
    assert_eq!(cov_rgba5551, 1);
    assert_eq!(fb[0].a, 255);

    let mut fb = vec![Rgba8::rgba(0, 0, 0, 0); 2 * 2];
    let cov_rgb565 = rasterize_quad(&mut fb, 2, 2, &rgb565, &quad, &uvs);
    assert_eq!(cov_rgb565, 1);
    assert_eq!(fb[0].a, 255);

    let a8_mask = Texture::from_bytes(
        &[128],
        1,
        1,
        TextureFormat::A8,
        Rgba8::rgba(40, 80, 160, 255),
    );
    let mut fb = vec![Rgba8::rgba(0, 0, 0, 0); 1];
    let cov_a8 = rasterize_quad(
        &mut fb,
        1,
        1,
        &a8_mask,
        &[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)],
        &uvs,
    );
    assert_eq!(cov_a8, 1);
    assert_eq!(fb[0], Rgba8::rgba(40, 80, 160, 128));

    let mut fb_a = vec![Rgba8::rgba(0, 0, 0, 0); 4 * 4];
    let mut fb_b = vec![Rgba8::rgba(0, 0, 0, 0); 4 * 4];
    let quad = [(0.0, 0.0), (0.0, 2.0), (2.0, 2.0), (2.0, 0.0)];
    let uvs = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)];
    let tex = Texture::from_bytes(
        &[
            0x00, 0xf8, // red
            0xe0, 0x07, // green
            0x1f, 0x00, // blue
            0xff, 0xff, // white
        ],
        2,
        2,
        TextureFormat::Rgb565,
        Rgba8::rgba(255, 255, 255, 255),
    );
    let cov_a = rasterize_quad(&mut fb_a, 4, 4, &tex, &quad, &uvs);
    let cov_b = rasterize_quad(
        &mut fb_b,
        4,
        4,
        &tex,
        &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)],
        &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
    );
    assert_eq!(cov_a, cov_b);
    assert_eq!(fb_a, fb_b);
}

#[test]
fn replay_frame4_produces_complete_artifact_and_hash() {
    let (fb, draws) = replay_frame4();
    assert_eq!(draws.len(), 4);

    let mut bytes = Vec::with_capacity(fb.len() * 4);
    for px in &fb {
        bytes.extend_from_slice(&[px.r, px.g, px.b, px.a]);
    }
    let hash = framebuffer_hash(&fb);
    if std::env::var_os("CLICKY_WRITE_TETRIS_FRAME4_PPM").is_some() {
        framebuffer_to_ppm(
            std::path::Path::new("/tmp/tetris_frame4_replay.ppm"),
            &fb,
            320,
            240,
        );
    }
    if std::env::var_os("CLICKY_PRINT_FRAME4_SUMMARY").is_some() {
        for (idx, draw) in draws.iter().enumerate() {
            println!(
                "draw{} handle={} cov={} bounds=({:.1},{:.1})-({:.1},{:.1}) tex={} kind={} format={:?}",
                idx + 1,
                draw.ordinal159_handle,
                draw.coverage,
                draw.screen_bounds.0,
                draw.screen_bounds.1,
                draw.screen_bounds.2,
                draw.screen_bounds.3,
                draw.proposed_texture.label,
                draw.proposed_texture.kind,
                draw.proposed_texture.format,
            );
        }
        println!("frame4_hash={:016x}", hash);
    }

    if std::env::var_os("CLICKY_WRITE_TETRIS_FRAME4_PPM").is_some() {
        let (_, base_draws) = replay_frame4();
        let draws_1_to_3_fb = render_draws(&base_draws, false);
        let all_draws_fb = render_draws(&base_draws, true);
        let draw4_alpha = replay_frame4_with_probe(Draw4ProbeMode::AlphaOnly);
        let draw4_alpha_fb = render_draws(&draw4_alpha[3..4], true);
        let draw4_opaque = replay_frame4_with_probe(Draw4ProbeMode::Opaque);
        let draw4_opaque_fb = render_draws(&draw4_opaque[3..4], true);

        write_frame4_ppm_if_requested("/tmp/tetris_frame4_draws_1_3.ppm", &draws_1_to_3_fb);
        write_frame4_ppm_if_requested("/tmp/tetris_frame4_all_draws.ppm", &all_draws_fb);
        write_frame4_ppm_if_requested("/tmp/tetris_frame4_draw4_alpha.ppm", &draw4_alpha_fb);
        write_frame4_ppm_if_requested("/tmp/tetris_frame4_draw4_opaque.ppm", &draw4_opaque_fb);
    }

    assert_eq!(hash, 0x3514_598d_ae7f_1fe2);
    assert_eq!(bytes.len(), 320 * 240 * 4);
}

#[derive(Debug, Copy, Clone)]
enum Draw4ProbeMode {
    AlphaOnly,
    Opaque,
}

fn draw4_probe_texture(mode: Draw4ProbeMode) -> (Texture, ProposedTexture) {
    match mode {
        Draw4ProbeMode::AlphaOnly => (
            make_texture(TextureFormat::A8, 1, 1, make_raw_a8(1, 1)),
            ProposedTexture::unresolved(
                "handle 3 / full-screen overlay",
                "identity/overlay probe; not a final asset mapping",
                TextureFormat::A8,
                0.28,
            ),
        ),
        Draw4ProbeMode::Opaque => (
            make_texture(TextureFormat::Rgba5551, 1, 1, vec![0xff, 0xff]),
            ProposedTexture::unresolved(
                "handle 3 / full-screen overlay",
                "identity/overlay probe; not a final asset mapping",
                TextureFormat::Rgba5551,
                0.28,
            ),
        ),
    }
}

fn replay_frame4_with_probe(mode: Draw4ProbeMode) -> Vec<DrawReplay> {
    let (_, mut draws) = replay_frame4();
    let (texture, proposed_texture) = draw4_probe_texture(mode);
    draws[3].texture = texture;
    draws[3].proposed_texture = proposed_texture;
    draws
}

fn render_draws(draws: &[DrawReplay], include_draw4: bool) -> Vec<Rgba8> {
    let mut fb = vec![Rgba8::rgba(0, 0, 0, 0); 320 * 240];
    for (idx, draw) in draws.iter().enumerate() {
        if !include_draw4 && idx == 3 {
            continue;
        }
        draw.rasterize(&mut fb);
    }
    fb
}

fn framebuffer_stats(
    fb: &[Rgba8],
    width: usize,
    height: usize,
) -> (usize, Option<(usize, usize, usize, usize)>, (u8, u8)) {
    let mut nonzero = 0usize;
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    let mut alpha_min = u8::MAX;
    let mut alpha_max = 0u8;

    for y in 0..height {
        for x in 0..width {
            let px = fb[y * width + x];
            if px.r != 0 || px.g != 0 || px.b != 0 || px.a != 0 {
                nonzero += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
            alpha_min = alpha_min.min(px.a);
            alpha_max = alpha_max.max(px.a);
        }
    }

    (
        nonzero,
        if nonzero == 0 {
            None
        } else {
            Some((min_x, min_y, max_x, max_y))
        },
        (alpha_min, alpha_max),
    )
}

fn diff_pixels(a: &[Rgba8], b: &[Rgba8]) -> usize {
    a.iter()
        .zip(b.iter())
        .filter(|(lhs, rhs)| lhs != rhs)
        .count()
}

fn write_frame4_ppm_if_requested(path: &str, fb: &[Rgba8]) {
    if std::env::var_os("CLICKY_WRITE_TETRIS_FRAME4_PPM").is_some() {
        framebuffer_to_ppm(std::path::Path::new(path), fb, 320, 240);
        println!("ppm_path={path} hash={:016x}", framebuffer_hash(fb));
    }
}

fn print_draw_summary(
    name: &str,
    fb: &[Rgba8],
    draws: &[DrawReplay],
    include_draw4: bool,
    draw_index_offset: usize,
) {
    let (nonzero, bbox, alpha_range) = framebuffer_stats(fb, 320, 240);
    println!("artifact={name}");
    println!("  hash={:016x}", framebuffer_hash(fb));
    println!("  nonzero_pixels={nonzero}");
    match bbox {
        Some((min_x, min_y, max_x, max_y)) => {
            println!("  bbox=({}, {})-({}, {})", min_x, min_y, max_x, max_y)
        }
        None => println!("  bbox=none"),
    }
    println!("  alpha_range=({}, {})", alpha_range.0, alpha_range.1);
    for (idx, draw) in draws.iter().enumerate() {
        if !include_draw4 && idx == 3 {
            continue;
        }
        println!(
            "  draw{} handle={} coverage={} bounds=({:.1},{:.1})-({:.1},{:.1}) tex={} kind={} format={:?}",
            idx + 1 + draw_index_offset,
            draw.ordinal159_handle,
            draw.coverage,
            draw.screen_bounds.0,
            draw.screen_bounds.1,
            draw.screen_bounds.2,
            draw.screen_bounds.3,
            draw.proposed_texture.label,
            draw.proposed_texture.kind,
            draw.proposed_texture.format,
        );
    }
}

#[test]
fn frame4_artifact_comparison_and_handle_mapping() {
    let fixture = load_fixture();
    let uploads = texture_upload_candidates(&fixture);
    let (_, base_draws) = replay_frame4();
    let draws_1_to_3 = render_draws(&base_draws, false);
    let all_draws = render_draws(&base_draws, true);

    let draw4_alpha = replay_frame4_with_probe(Draw4ProbeMode::AlphaOnly);
    let draw4_alpha_fb = render_draws(&draw4_alpha[3..4], true);
    let draw4_opaque = replay_frame4_with_probe(Draw4ProbeMode::Opaque);
    let draw4_opaque_fb = render_draws(&draw4_opaque[3..4], true);
    let draw4_only = render_draws(&base_draws[3..4], true);

    if std::env::var_os("CLICKY_PRINT_FRAME4_ARTIFACTS").is_some() {
        print_draw_summary("draws_1_to_3_only", &draws_1_to_3, &base_draws, false, 0);
        print_draw_summary(
            "all_draws_draw4_disabled",
            &draws_1_to_3,
            &base_draws,
            false,
            0,
        );
        print_draw_summary("all_draws_placeholder", &all_draws, &base_draws, true, 0);
        println!(
            "  overwrite_vs_draws_1_to_3={} diff_pixels={}",
            if diff_pixels(&draws_1_to_3, &all_draws) > 0 {
                "yes"
            } else {
                "no"
            },
            diff_pixels(&draws_1_to_3, &all_draws)
        );

        print_draw_summary(
            "draw4_only_placeholder",
            &draw4_only,
            &base_draws[3..4],
            true,
            3,
        );
        print_draw_summary(
            "draw4_only_alpha",
            &draw4_alpha_fb,
            &draw4_alpha[3..4],
            true,
            3,
        );
        print_draw_summary(
            "draw4_only_opaque",
            &draw4_opaque_fb,
            &draw4_opaque[3..4],
            true,
            3,
        );
    }

    write_frame4_ppm_if_requested("/tmp/tetris_frame4_draws_1_3.ppm", &draws_1_to_3);
    write_frame4_ppm_if_requested("/tmp/tetris_frame4_all_draws.ppm", &all_draws);
    write_frame4_ppm_if_requested("/tmp/tetris_frame4_draw4_alpha.ppm", &draw4_alpha_fb);
    write_frame4_ppm_if_requested("/tmp/tetris_frame4_draw4_opaque.ppm", &draw4_opaque_fb);

    // Conservative handle mapping from upload candidates to frame-4 ord159 draws.
    let mapping_rows = [
        (
            "screenBG_565.pix",
            19u32,
            "frame4 draw1",
            Some(0.93f32),
            "exact table write not captured; matched by size + fullscreen state blob",
        ),
        (
            "tetrisLogoT_4444.pix",
            14u32,
            "frame4 draw2",
            Some(0.84f32),
            "exact table write not captured; matched by size + state blob",
        ),
        (
            "eaLogo_5551.pix",
            27u32,
            "frame4 draw3",
            Some(0.87f32),
            "exact table write not captured; matched by size + state blob",
        ),
        (
            "no upload candidate",
            3u32,
            "frame4 draw4",
            Some(0.28f32),
            "no matching upload triplet; appears to be a generated fullscreen overlay/material blob",
        ),
    ];

    if std::env::var_os("CLICKY_PRINT_FRAME4_MAPPING").is_some() {
        println!("mapping_table");
        for row in &mapping_rows {
            println!(
                "  source_file={} handle={} draw={} confidence={:.2} missing={}",
                row.0,
                row.1,
                row.2,
                row.3.unwrap_or(0.0),
                row.4,
            );
        }
        println!("  uploads={}", uploads.len());
        for upload in uploads.iter().take(4) {
            println!(
                "  upload seqs={}→{}→{} file={} desc={:#x} object_tag={} target={:#x} fmt={:#x} type={:#x} src={:#x}",
                upload.ordinal45_seq,
                upload.ordinal4_seq,
                upload.ordinal99_seq,
                upload
                    .source_file
                    .as_ref()
                    .map(|f| f.path.as_str())
                    .unwrap_or("<unknown>"),
                upload.descriptor_ptr,
                upload.object_tag,
                upload.target,
                upload.internal_format,
                upload.pixel_type,
                upload.source_ptr,
            );
        }
    }

    assert!(diff_pixels(&draws_1_to_3, &all_draws) > 0);
    assert_eq!(draw4_only.len(), 320 * 240);
    assert_eq!(draw4_alpha_fb.len(), 320 * 240);
    assert_eq!(draw4_opaque_fb.len(), 320 * 240);
}

// --- Local asset-backed replay helpers -------------------------------------
//
// These helpers power the optional, opt-in real-asset frame-4 replay. They
// never embed commercial asset bytes; payloads are sliced from the user's own
// local .pix files using the captured upload metadata as the source of truth
// for dimensions, format, and payload offset.

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalAsset {
    filename: String,
    file_path: std::path::PathBuf,
    width: usize,
    height: usize,
    format: TextureFormat,
    payload_offset: usize,
    payload_size: usize,
}

fn pix_payload_size(format: TextureFormat, width: usize, height: usize) -> usize {
    let bytes_per_pixel = match format {
        TextureFormat::Rgb565 | TextureFormat::Rgba5551 | TextureFormat::Rgba4444 => 2,
        TextureFormat::Rgba8888 => 4,
        TextureFormat::LuminanceAlpha88 => 2,
        TextureFormat::A8 => 1,
    };
    width * height * bytes_per_pixel
}

/// Map the captured GL upload constants to the standalone renderer's
/// `TextureFormat`. Constants are standard GL ES 1.1 enumerants:
///   GL_RGB=0x1907, GL_RGBA=0x1908, GL_ALPHA=0x1906,
///   GL_LUMINANCE_ALPHA=0x190a
///   GL_UNSIGNED_SHORT_5_6_5=0x8363, GL_UNSIGNED_SHORT_5_5_5_1=0x8034,
///   GL_UNSIGNED_SHORT_4_4_4_4=0x8033, GL_UNSIGNED_BYTE=0x1401
fn format_from_gl(internal_format: u32, pixel_type: u32) -> Option<TextureFormat> {
    match (internal_format, pixel_type) {
        (0x1907, 0x8363) => Some(TextureFormat::Rgb565),
        (0x1908, 0x8034) => Some(TextureFormat::Rgba5551),
        (0x1908, 0x8033) => Some(TextureFormat::Rgba4444),
        (0x1908, 0x1401) => Some(TextureFormat::Rgba8888),
        (0x190a, 0x1401) => Some(TextureFormat::LuminanceAlpha88),
        (0x1906, 0x1401) => Some(TextureFormat::A8),
        _ => None,
    }
}

/// Slice the pixel payload out of a raw .pix file using the captured offset
/// and the upload-derived dimensions/format. Validates bounds so a truncated
/// or mismatched file produces a clear error instead of a panic.
fn extract_pix_payload(
    file_bytes: &[u8],
    payload_offset: usize,
    width: usize,
    height: usize,
    format: TextureFormat,
) -> Result<Vec<u8>, String> {
    let payload_size = pix_payload_size(format, width, height);
    let end = payload_offset.checked_add(payload_size).ok_or_else(|| {
        format!("payload size overflow: offset={payload_offset} size={payload_size}")
    })?;
    if end > file_bytes.len() {
        return Err(format!(
            "payload bounds exceed file: offset={payload_offset} size={payload_size} file_len={}",
            file_bytes.len()
        ));
    }
    Ok(file_bytes[payload_offset..end].to_vec())
}

/// Locate the first upload candidate whose captured source file matches
/// `filename`. The trace uploads each asset at least once during frame 2.
fn find_upload_for_file<'a>(
    uploads: &'a [TextureUploadCandidate],
    filename: &str,
) -> Option<&'a TextureUploadCandidate> {
    uploads.iter().find(|upload| {
        upload
            .source_file
            .as_ref()
            .map(|file| file.path == filename)
            .unwrap_or(false)
    })
}

/// Load one local .pix asset, extracting the payload using the captured
/// upload metadata (dimensions, format, and payload offset). Prints
/// diagnostics for every resolved asset.
fn load_local_asset(
    asset_dir: &std::path::Path,
    uploads: &[TextureUploadCandidate],
    filename: &str,
) -> Result<(LocalAsset, Texture), String> {
    let upload = find_upload_for_file(uploads, filename)
        .ok_or_else(|| format!("no upload for {filename}"))?;
    let file_backing = upload
        .source_file
        .as_ref()
        .ok_or_else(|| format!("no file backing for {filename}"))?;
    let format = format_from_gl(upload.internal_format, upload.pixel_type).ok_or_else(|| {
        format!(
            "unsupported format for {filename}: internal={:#x} type={:#x}",
            upload.internal_format, upload.pixel_type
        )
    })?;
    let width = upload.width as usize;
    let height = upload.height as usize;
    // The captured offset is `source_ptr - file_base_addr`, i.e. the byte
    // offset within the .pix file where the guest told GL the pixels begin.
    // Do NOT assume a universal header length; trust the captured value.
    let payload_offset = file_backing.offset as usize;
    let payload_size = pix_payload_size(format, width, height);

    let file_path = asset_dir.join(&file_backing.path);
    let file_bytes =
        std::fs::read(&file_path).map_err(|e| format!("read {}: {e}", file_path.display()))?;

    if file_bytes.len() as u32 != file_backing.len {
        eprintln!(
            "warn: {filename} local_len={} differs from captured_len={}; using captured offset/size",
            file_bytes.len(),
            file_backing.len
        );
    }

    let payload = extract_pix_payload(&file_bytes, payload_offset, width, height, format)?;
    println!(
        "asset_resolved file={filename} path={} dimensions={width}x{height} format={format:?} payload_offset={payload_offset} payload_bytes={payload_size} file_bytes={}",
        file_path.display(),
        file_bytes.len(),
    );

    let texture = make_texture(format, width, height, payload);
    let asset = LocalAsset {
        filename: filename.to_string(),
        file_path,
        width,
        height,
        format,
        payload_offset,
        payload_size,
    };
    Ok((asset, texture))
}

/// Replace the generated placeholder textures for draws 1-3 with the supplied
/// local-asset textures, matching each draw by its captured dimensions/format.
///
/// This local replay path still does **not** prove a direct handle→asset link;
/// it uses the captured size/format metadata as the current best match.
fn apply_local_assets(draws: &mut [DrawReplay], assets: &[LocalAsset], textures: &[Texture]) {
    for draw in draws.iter_mut().take(3) {
        let (w, h) = (
            draw.proposed_texture.width.unwrap_or(0),
            draw.proposed_texture.height.unwrap_or(0),
        );
        if let Some(idx) = assets.iter().position(|asset| {
            asset.width == w && asset.height == h && asset.format == draw.proposed_texture.format
        }) {
            draw.texture = textures[idx].clone();
        }
    }
}

fn render_real_variant_and_write_ppm(
    name: &str,
    path: &str,
    draws: &[DrawReplay],
    include_draw4: bool,
) {
    let fb = render_draws(draws, include_draw4);
    framebuffer_to_ppm(std::path::Path::new(path), &fb, 320, 240);
    println!(
        "artifact={name} output={path} hash={:016x} nonzero={}",
        framebuffer_hash(&fb),
        framebuffer_stats(&fb, 320, 240).0,
    );
}

/// Opt-in local-asset frame-4 replay. Skipped entirely when
/// `CLICKY_TETRIS_ASSET_DIR` is absent so the generated-texture deterministic
/// tests remain the default path. Draw 4 (handle 3) is deliberately kept as an
/// experimental translucent overlay probe; it is NOT confirmed as a final
/// visual interpretation.
#[test]
fn replay_frame4_with_local_assets_when_requested() {
    let asset_dir = match std::env::var_os("CLICKY_TETRIS_ASSET_DIR") {
        Some(value) => std::path::PathBuf::from(value),
        None => return, // opt-in only
    };

    let fixture = load_fixture();
    let uploads = texture_upload_candidates(&fixture);

    let targets = [
        "screenBG_565.pix",
        "tetrisLogoT_4444.pix",
        "eaLogo_5551.pix",
    ];
    let mut assets = Vec::new();
    let mut textures = Vec::new();
    for filename in targets {
        let (asset, texture) = load_local_asset(&asset_dir, &uploads, filename)
            .unwrap_or_else(|err| panic!("load {}: {}", filename, err));
        assets.push(asset);
        textures.push(texture);
    }
    assert_eq!(assets.len(), 3);

    // draws 1-3 with real textures, draw 4 disabled (no overlay).
    let (_, mut draws_no_overlay) = replay_frame4();
    apply_local_assets(&mut draws_no_overlay, &assets, &textures);

    // all draws: draw 4 stays as the current best-fit translucent overlay probe.
    let (_, mut draws_all) = replay_frame4();
    apply_local_assets(&mut draws_all, &assets, &textures);

    // explicit alpha-only overlay probe variant for handle 3.
    let mut draws_alpha = replay_frame4_with_probe(Draw4ProbeMode::AlphaOnly);
    apply_local_assets(&mut draws_alpha, &assets, &textures);

    println!("overlay_probe_note kind=experimental handle=3 interpretation=unconfirmed");

    render_real_variant_and_write_ppm(
        "real_draws_1_3",
        "/tmp/tetris_frame4_real_draws_1_3.ppm",
        &draws_no_overlay,
        false,
    );
    render_real_variant_and_write_ppm(
        "real_no_overlay",
        "/tmp/tetris_frame4_real_no_overlay.ppm",
        &draws_no_overlay,
        false,
    );
    render_real_variant_and_write_ppm(
        "real_all_draws",
        "/tmp/tetris_frame4_real_all_draws.ppm",
        &draws_all,
        true,
    );
    render_real_variant_and_write_ppm(
        "real_overlay_alpha",
        "/tmp/tetris_frame4_real_overlay_alpha.ppm",
        &draws_alpha,
        true,
    );
}

#[test]
fn pix_payload_size_matches_format_dimensions() {
    assert_eq!(
        pix_payload_size(TextureFormat::Rgb565, 320, 240),
        320 * 240 * 2
    );
    assert_eq!(
        pix_payload_size(TextureFormat::Rgba5551, 50, 50),
        50 * 50 * 2
    );
    assert_eq!(
        pix_payload_size(TextureFormat::Rgba4444, 250, 162),
        250 * 162 * 2
    );
    assert_eq!(
        pix_payload_size(TextureFormat::Rgba8888, 40, 20),
        40 * 20 * 4
    );
    assert_eq!(
        pix_payload_size(TextureFormat::LuminanceAlpha88, 12, 34),
        12 * 34 * 2
    );
    assert_eq!(pix_payload_size(TextureFormat::A8, 784, 20), 784 * 20);
}

#[test]
fn format_from_gl_maps_captured_upload_constants() {
    assert_eq!(format_from_gl(0x1907, 0x8363), Some(TextureFormat::Rgb565));
    assert_eq!(
        format_from_gl(0x1908, 0x8034),
        Some(TextureFormat::Rgba5551)
    );
    assert_eq!(
        format_from_gl(0x1908, 0x8033),
        Some(TextureFormat::Rgba4444)
    );
    assert_eq!(
        format_from_gl(0x1908, 0x1401),
        Some(TextureFormat::Rgba8888)
    );
    assert_eq!(
        format_from_gl(0x190a, 0x1401),
        Some(TextureFormat::LuminanceAlpha88)
    );
    assert_eq!(format_from_gl(0x1906, 0x1401), Some(TextureFormat::A8));
    assert_eq!(format_from_gl(0xdead, 0xbeef), None);
}

#[test]
fn extract_pix_payload_slices_validated_region() {
    // Synthetic .pix: 12-byte fake header, 6 bytes of RGB565 pixels (3 pixels),
    // 4-byte trailer. Nothing here is a real asset.
    let mut file = Vec::new();
    file.extend_from_slice(&[0u8; 12]); // header
    file.extend_from_slice(&[0x00, 0xf8, 0xe0, 0x07, 0x1f, 0x00]); // 3 rgb565 pixels
    file.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]); // trailer

    let payload =
        extract_pix_payload(&file, 12, 3, 1, TextureFormat::Rgb565).expect("valid region");
    assert_eq!(payload, vec![0x00, 0xf8, 0xe0, 0x07, 0x1f, 0x00]);
}

#[test]
fn extract_pix_payload_rejects_short_files() {
    let file = vec![0u8; 70 + 10]; // header-sized but payload truncated
    let err =
        extract_pix_payload(&file, 70, 50, 50, TextureFormat::Rgba5551).expect_err("should reject");
    assert!(err.contains("payload bounds exceed file"), "{}", err);
    assert!(err.contains("file_len=80"));
}

#[test]
fn extract_pix_payload_handles_zero_dimensions() {
    let file = vec![0u8; 8];
    let payload =
        extract_pix_payload(&file, 4, 0, 0, TextureFormat::A8).expect("zero-sized payload");
    assert!(payload.is_empty());
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum OrientationVariant {
    Current,
    TextureVFlip,
    UvVFlip,
    FramebufferVFlip,
    HFlipControl,
    BothAxisControl,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ScreenSpaceVariant {
    Current,
    PresentationFlip,
    PerVertexYFlip,
    RectangleAwareYFlip,
}

fn flip_v_for_height(v: f32, height: usize) -> f32 {
    height as f32 - v
}

fn flip_texture_vertical(texture: &Texture) -> Texture {
    let mut pixels = texture.pixels.clone();
    for row in 0..(texture.height / 2) {
        let top = row * texture.width;
        let bottom = (texture.height - 1 - row) * texture.width;
        for col in 0..texture.width {
            pixels.swap(top + col, bottom + col);
        }
    }
    Texture {
        width: texture.width,
        height: texture.height,
        pixels,
    }
}

fn flip_texture_horizontal(texture: &Texture) -> Texture {
    let mut pixels = texture.pixels.clone();
    for row in 0..texture.height {
        let base = row * texture.width;
        for col in 0..(texture.width / 2) {
            pixels.swap(base + col, base + (texture.width - 1 - col));
        }
    }
    Texture {
        width: texture.width,
        height: texture.height,
        pixels,
    }
}

fn flip_texture_both_axes(texture: &Texture) -> Texture {
    flip_texture_horizontal(&flip_texture_vertical(texture))
}

fn flip_draw_uvs_vertical(draw: &mut DrawReplay) {
    let height = draw.texture.height;
    for (u, v) in &mut draw.uv_or_aux {
        *v = flip_v_for_height(*v, height);
        let _ = u;
    }
}

fn flip_framebuffer_vertical_in_place(fb: &mut [Rgba8], width: usize, height: usize) {
    let mut flipped = vec![Rgba8::rgba(0, 0, 0, 0); width * height];
    for y in 0..height {
        let src = &fb[y * width..(y + 1) * width];
        let dst_y = height - 1 - y;
        flipped[dst_y * width..(dst_y + 1) * width].copy_from_slice(src);
    }
    fb.copy_from_slice(&flipped);
}

fn bounds_for_positions(positions: &[(f32, f32); 4]) -> (f32, f32, f32, f32) {
    positions.iter().fold(
        (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ),
        |acc, (x, y)| (acc.0.min(*x), acc.1.min(*y), acc.2.max(*x), acc.3.max(*y)),
    )
}

fn format_positions(positions: &[(f32, f32); 4]) -> String {
    positions
        .iter()
        .map(|(x, y)| format!("({x:.1},{y:.1})"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn flip_positions_vertical_per_vertex(
    positions: &[(f32, f32); 4],
    framebuffer_height: usize,
) -> [(f32, f32); 4] {
    let height = framebuffer_height as f32;
    [
        (positions[0].0, height - positions[0].1),
        (positions[1].0, height - positions[1].1),
        (positions[2].0, height - positions[2].1),
        (positions[3].0, height - positions[3].1),
    ]
}

fn flip_positions_vertical_rectangle_aware(
    draw: &DrawReplay,
    framebuffer_height: usize,
) -> [(f32, f32); 4] {
    let delta = framebuffer_height as f32 - (draw.screen_bounds.1 + draw.screen_bounds.3);
    [
        (draw.local_positions[0].0, draw.local_positions[0].1 + delta),
        (draw.local_positions[1].0, draw.local_positions[1].1 + delta),
        (draw.local_positions[2].0, draw.local_positions[2].1 + delta),
        (draw.local_positions[3].0, draw.local_positions[3].1 + delta),
    ]
}

fn render_orientation_variant(draws: &[DrawReplay], variant: OrientationVariant) -> Vec<Rgba8> {
    let mut variant_draws = draws.to_vec();
    match variant {
        OrientationVariant::Current => {}
        OrientationVariant::TextureVFlip => {
            for draw in &mut variant_draws {
                draw.texture = flip_texture_vertical(&draw.texture);
            }
        }
        OrientationVariant::UvVFlip => {
            for draw in &mut variant_draws {
                flip_draw_uvs_vertical(draw);
            }
        }
        OrientationVariant::FramebufferVFlip => {}
        OrientationVariant::HFlipControl => {
            for draw in &mut variant_draws {
                draw.texture = flip_texture_horizontal(&draw.texture);
            }
        }
        OrientationVariant::BothAxisControl => {
            for draw in &mut variant_draws {
                draw.texture = flip_texture_both_axes(&draw.texture);
            }
        }
    }

    let mut fb = vec![Rgba8::rgba(0, 0, 0, 0); 320 * 240];
    for draw in &variant_draws {
        draw.rasterize(&mut fb);
    }
    if matches!(variant, OrientationVariant::FramebufferVFlip) {
        let mut flipped = vec![Rgba8::rgba(0, 0, 0, 0); 320 * 240];
        for y in 0..240 {
            let src = &fb[y * 320..(y + 1) * 320];
            let dst_y = 239 - y;
            flipped[dst_y * 320..(dst_y + 1) * 320].copy_from_slice(src);
        }
        fb = flipped;
    }
    fb
}

fn write_orientation_artifact(name: &str, path: &str, fb: &[Rgba8]) {
    framebuffer_to_ppm(std::path::Path::new(path), fb, 320, 240);
    println!(
        "orientation_artifact={name} output={path} hash={:016x}",
        framebuffer_hash(fb)
    );
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum TextureOrientationVariant {
    Current,
    TextureVFlip,
    UvVFlip,
}

fn render_texture_and_screen_variant(
    draws: &[DrawReplay],
    texture_variant: TextureOrientationVariant,
    screen_variant: ScreenSpaceVariant,
) -> Vec<Rgba8> {
    let mut variant_draws = draws.to_vec();
    match texture_variant {
        TextureOrientationVariant::Current => {}
        TextureOrientationVariant::TextureVFlip => {
            for draw in &mut variant_draws {
                draw.texture = flip_texture_vertical(&draw.texture);
            }
        }
        TextureOrientationVariant::UvVFlip => {
            for draw in &mut variant_draws {
                flip_draw_uvs_vertical(draw);
            }
        }
    }

    match screen_variant {
        ScreenSpaceVariant::Current | ScreenSpaceVariant::PresentationFlip => {}
        ScreenSpaceVariant::PerVertexYFlip => {
            for draw in &mut variant_draws {
                draw.local_positions =
                    flip_positions_vertical_per_vertex(&draw.local_positions, 240);
                draw.screen_bounds = bounds_for_positions(&draw.local_positions);
            }
        }
        ScreenSpaceVariant::RectangleAwareYFlip => {
            for draw in &mut variant_draws {
                draw.local_positions = flip_positions_vertical_rectangle_aware(draw, 240);
                draw.screen_bounds = bounds_for_positions(&draw.local_positions);
            }
        }
    }

    let mut fb = vec![Rgba8::rgba(0, 0, 0, 0); 320 * 240];
    for draw in &variant_draws {
        draw.rasterize(&mut fb);
    }
    if matches!(screen_variant, ScreenSpaceVariant::PresentationFlip) {
        flip_framebuffer_vertical_in_place(&mut fb, 320, 240);
    }
    fb
}

fn screen_bounds_after_vertical_flip(
    bounds: (f32, f32, f32, f32),
    framebuffer_height: usize,
) -> (f32, f32, f32, f32) {
    let height = framebuffer_height as f32;
    (bounds.0, height - bounds.3, bounds.2, height - bounds.1)
}

#[test]
fn orientation_helpers_respect_corner_markers_and_global_vertical_origin() {
    let tex = Texture::from_bytes(
        &[
            0x00, 0xf8, // top-left red
            0xe0, 0x07, // top-right green
            0x1f, 0x00, // bottom-left blue
            0xff, 0xff, // bottom-right white
        ],
        2,
        2,
        TextureFormat::Rgb565,
        Rgba8::rgba(255, 255, 255, 255),
    );
    let quad = [(0.0, 0.0), (0.0, 2.0), (2.0, 2.0), (2.0, 0.0)];
    let uvs = [(0.5, 0.5), (0.5, 1.5), (1.5, 1.5), (1.5, 0.5)];

    let mut current_fb = vec![Rgba8::rgba(0, 0, 0, 0); 4];
    let current_cov = rasterize_quad(&mut current_fb, 2, 2, &tex, &quad, &uvs);
    assert_eq!(current_cov, 4);
    assert_eq!(current_fb[0], Rgba8::rgba(255, 0, 0, 255));
    assert_eq!(current_fb[1], Rgba8::rgba(0, 255, 0, 255));
    assert_eq!(current_fb[2], Rgba8::rgba(0, 0, 255, 255));
    assert_eq!(current_fb[3], Rgba8::rgba(255, 255, 255, 255));

    let vflip_tex = flip_texture_vertical(&tex);
    let mut texture_vflip_fb = vec![Rgba8::rgba(0, 0, 0, 0); 4];
    rasterize_quad(&mut texture_vflip_fb, 2, 2, &vflip_tex, &quad, &uvs);
    assert_eq!(texture_vflip_fb[0], Rgba8::rgba(0, 0, 255, 255));
    assert_eq!(texture_vflip_fb[1], Rgba8::rgba(255, 255, 255, 255));
    assert_eq!(texture_vflip_fb[2], Rgba8::rgba(255, 0, 0, 255));
    assert_eq!(texture_vflip_fb[3], Rgba8::rgba(0, 255, 0, 255));

    let mut uv_vflip_fb = vec![Rgba8::rgba(0, 0, 0, 0); 4];
    let mut uv_flipped = uvs;
    for uv in &mut uv_flipped {
        uv.1 = flip_v_for_height(uv.1, tex.height);
    }
    rasterize_quad(&mut uv_vflip_fb, 2, 2, &tex, &quad, &uv_flipped);
    assert_eq!(uv_vflip_fb, texture_vflip_fb);

    let hflip_tex = flip_texture_horizontal(&tex);
    let mut hflip_fb = vec![Rgba8::rgba(0, 0, 0, 0); 4];
    rasterize_quad(&mut hflip_fb, 2, 2, &hflip_tex, &quad, &uvs);
    assert_eq!(hflip_fb[0], Rgba8::rgba(0, 255, 0, 255));
    assert_eq!(hflip_fb[1], Rgba8::rgba(255, 0, 0, 255));
    assert_eq!(hflip_fb[2], Rgba8::rgba(255, 255, 255, 255));
    assert_eq!(hflip_fb[3], Rgba8::rgba(0, 0, 255, 255));

    let both_tex = flip_texture_both_axes(&tex);
    let mut both_fb = vec![Rgba8::rgba(0, 0, 0, 0); 4];
    rasterize_quad(&mut both_fb, 2, 2, &both_tex, &quad, &uvs);
    assert_eq!(both_fb[0], Rgba8::rgba(255, 255, 255, 255));
    assert_eq!(both_fb[1], Rgba8::rgba(0, 0, 255, 255));
    assert_eq!(both_fb[2], Rgba8::rgba(0, 255, 0, 255));
    assert_eq!(both_fb[3], Rgba8::rgba(255, 0, 0, 255));

    let fb_vflip = {
        let mut out = vec![Rgba8::rgba(0, 0, 0, 0); 4];
        for y in 0..2 {
            let src = &current_fb[y * 2..(y + 1) * 2];
            let dst_y = 1 - y;
            out[dst_y * 2..(dst_y + 1) * 2].copy_from_slice(src);
        }
        out
    };
    assert_eq!(fb_vflip[0], current_fb[2]);
    assert_eq!(fb_vflip[1], current_fb[3]);
    assert_eq!(fb_vflip[2], current_fb[0]);
    assert_eq!(fb_vflip[3], current_fb[1]);

    assert_eq!(flip_v_for_height(49.5, 50), 0.5);
    assert_eq!(flip_v_for_height(-0.5, 50), 50.5);
}

#[test]
fn screen_space_origin_and_serialization_flips_are_separate_from_texture_orientation() {
    let mut current_fb = vec![Rgba8::rgba(0, 0, 0, 0); 4 * 6];
    current_fb[0] = Rgba8::rgba(255, 0, 0, 255);
    current_fb[1] = Rgba8::rgba(0, 255, 0, 255);
    current_fb[4 * 5] = Rgba8::rgba(0, 0, 255, 255);
    current_fb[4 * 5 + 1] = Rgba8::rgba(255, 255, 255, 255);

    let mut presentation_fb = current_fb.clone();
    flip_framebuffer_vertical_in_place(&mut presentation_fb, 4, 6);
    assert_eq!(presentation_fb[0], Rgba8::rgba(0, 0, 255, 255));
    assert_eq!(presentation_fb[1], Rgba8::rgba(255, 255, 255, 255));
    assert_eq!(presentation_fb[4 * 5], Rgba8::rgba(255, 0, 0, 255));
    assert_eq!(presentation_fb[4 * 5 + 1], Rgba8::rgba(0, 255, 0, 255));

    let quad = [(0.5, 0.5), (0.5, 2.5), (2.5, 2.5), (2.5, 0.5)];
    let draw = DrawReplay {
        ordinal159_handle: 1,
        state_ptr: 2,
        translation: (0.0, 0.0),
        local_positions: quad,
        uv_or_aux: vec![(0.5, 0.5); 4],
        aux_array: None,
        screen_bounds: bounds_for_positions(&quad),
        proposed_texture: ProposedTexture::unresolved(
            "screen-space probe",
            "screen-space probe",
            TextureFormat::Rgb565,
            1.0,
        ),
        texture: Texture::from_bytes(
            &[0x00, 0xf8],
            1,
            1,
            TextureFormat::Rgb565,
            Rgba8::rgba(255, 255, 255, 255),
        ),
        coverage: 0,
    };

    let per_vertex_positions = flip_positions_vertical_per_vertex(&draw.local_positions, 6);
    let rect_aware_positions = flip_positions_vertical_rectangle_aware(&draw, 6);
    assert_ne!(per_vertex_positions, rect_aware_positions);
    assert_eq!(
        bounds_for_positions(&per_vertex_positions),
        bounds_for_positions(&rect_aware_positions)
    );
    assert_eq!(
        screen_bounds_after_vertical_flip(draw.screen_bounds, 6),
        bounds_for_positions(&per_vertex_positions)
    );
}

#[test]
fn replay_frame4_real_asset_orientation_when_requested() {
    let asset_dir = match std::env::var_os("CLICKY_TETRIS_ASSET_DIR") {
        Some(value) => std::path::PathBuf::from(value),
        None => return,
    };

    let fixture = load_fixture();
    let uploads = texture_upload_candidates(&fixture);
    let targets = [
        "screenBG_565.pix",
        "tetrisLogoT_4444.pix",
        "eaLogo_5551.pix",
    ];

    let mut assets = Vec::new();
    let mut textures = Vec::new();
    for filename in targets {
        let (asset, texture) = load_local_asset(&asset_dir, &uploads, filename)
            .unwrap_or_else(|err| panic!("load {}: {}", filename, err));
        assets.push(asset);
        textures.push(texture);
    }

    let (_, mut current_draws) = replay_frame4();
    apply_local_assets(&mut current_draws, &assets, &textures);

    if std::env::var_os("CLICKY_PRINT_FRAME4_ORIENTATION_GEOMETRY").is_some() {
        println!("orientation_geometry screen_height=240 note=origin-convention-is-still-under-investigation");
        for (idx, draw) in current_draws.iter().enumerate() {
            let per_vertex_positions =
                flip_positions_vertical_per_vertex(&draw.local_positions, 240);
            let rect_aware_positions = flip_positions_vertical_rectangle_aware(draw, 240);
            let current_bounds = draw.screen_bounds;
            let presentation_bounds = screen_bounds_after_vertical_flip(current_bounds, 240);
            println!(
                "  draw{} handle={} translation=({:.1},{:.1}) current_positions=[{}] current_bounds=({:.1},{:.1})-({:.1},{:.1}) presentation_flip_bounds=({:.1},{:.1})-({:.1},{:.1}) per_vertex_positions=[{}] per_vertex_bounds=({:.1},{:.1})-({:.1},{:.1}) rect_aware_positions=[{}] rect_aware_bounds=({:.1},{:.1})-({:.1},{:.1})",
                idx + 1,
                draw.ordinal159_handle,
                draw.translation.0,
                draw.translation.1,
                format_positions(&draw.local_positions),
                current_bounds.0,
                current_bounds.1,
                current_bounds.2,
                current_bounds.3,
                presentation_bounds.0,
                presentation_bounds.1,
                presentation_bounds.2,
                presentation_bounds.3,
                format_positions(&per_vertex_positions),
                bounds_for_positions(&per_vertex_positions).0,
                bounds_for_positions(&per_vertex_positions).1,
                bounds_for_positions(&per_vertex_positions).2,
                bounds_for_positions(&per_vertex_positions).3,
                format_positions(&rect_aware_positions),
                bounds_for_positions(&rect_aware_positions).0,
                bounds_for_positions(&rect_aware_positions).1,
                bounds_for_positions(&rect_aware_positions).2,
                bounds_for_positions(&rect_aware_positions).3,
            );
        }
        println!("  texture_convention=current_row_order (not baked in)");
        println!(
            "  screen_space_convention=framebuffer_presentation_flip (not a vertex transform)"
        );
    }

    let current_fb = render_texture_and_screen_variant(
        &current_draws,
        TextureOrientationVariant::Current,
        ScreenSpaceVariant::Current,
    );
    write_orientation_artifact(
        "current",
        "/tmp/tetris_frame4_real_orientation_current.ppm",
        &current_fb,
    );

    let screen_origin_best_fb = render_texture_and_screen_variant(
        &current_draws,
        TextureOrientationVariant::Current,
        ScreenSpaceVariant::PresentationFlip,
    );
    write_orientation_artifact(
        "screen_origin_best",
        "/tmp/tetris_frame4_orientation_screen_origin_best.ppm",
        &screen_origin_best_fb,
    );

    let current_texture_screen_flip_fb = screen_origin_best_fb.clone();
    write_orientation_artifact(
        "no_texture_framebuffer_vflip",
        "/tmp/tetris_frame4_orientation_no_texture_framebuffer_vflip.ppm",
        &current_texture_screen_flip_fb,
    );

    let per_vertex_screen_fb = render_texture_and_screen_variant(
        &current_draws,
        TextureOrientationVariant::Current,
        ScreenSpaceVariant::PerVertexYFlip,
    );
    write_orientation_artifact(
        "per_vertex_screen_y_flip",
        "/tmp/tetris_frame4_orientation_per_vertex_screen_y_flip.ppm",
        &per_vertex_screen_fb,
    );

    let rect_aware_screen_fb = render_texture_and_screen_variant(
        &current_draws,
        TextureOrientationVariant::Current,
        ScreenSpaceVariant::RectangleAwareYFlip,
    );
    write_orientation_artifact(
        "rectangle_aware_screen_y_flip",
        "/tmp/tetris_frame4_orientation_rectangle_aware_screen_y_flip.ppm",
        &rect_aware_screen_fb,
    );

    let texture_vflip_fb = render_texture_and_screen_variant(
        &current_draws,
        TextureOrientationVariant::TextureVFlip,
        ScreenSpaceVariant::Current,
    );
    write_orientation_artifact(
        "texture_vflip_no_framebuffer",
        "/tmp/tetris_frame4_orientation_texture_vflip_no_framebuffer.ppm",
        &texture_vflip_fb,
    );

    let texture_vflip_framebuffer_fb = render_texture_and_screen_variant(
        &current_draws,
        TextureOrientationVariant::TextureVFlip,
        ScreenSpaceVariant::PresentationFlip,
    );
    write_orientation_artifact(
        "texture_vflip_framebuffer_vflip",
        "/tmp/tetris_frame4_orientation_texture_vflip_framebuffer_vflip.ppm",
        &texture_vflip_framebuffer_fb,
    );

    let uv_vflip_framebuffer_fb = render_texture_and_screen_variant(
        &current_draws,
        TextureOrientationVariant::UvVFlip,
        ScreenSpaceVariant::PresentationFlip,
    );
    write_orientation_artifact(
        "uv_vflip_framebuffer_vflip",
        "/tmp/tetris_frame4_orientation_uv_vflip_framebuffer_vflip.ppm",
        &uv_vflip_framebuffer_fb,
    );

    let hflip_control_fb =
        render_orientation_variant(&current_draws, OrientationVariant::HFlipControl);
    write_orientation_artifact(
        "hflip_control",
        "/tmp/tetris_frame4_real_orientation_hflip_control.ppm",
        &hflip_control_fb,
    );

    let both_axis_fb =
        render_orientation_variant(&current_draws, OrientationVariant::BothAxisControl);
    write_orientation_artifact(
        "both_axis_control",
        "/tmp/tetris_frame4_real_orientation_both_axis_control.ppm",
        &both_axis_fb,
    );

    assert_ne!(current_fb, screen_origin_best_fb);
    assert_ne!(current_fb, texture_vflip_fb);
    assert_ne!(current_fb, texture_vflip_framebuffer_fb);
    assert_ne!(current_fb, uv_vflip_framebuffer_fb);
    assert_ne!(texture_vflip_fb, texture_vflip_framebuffer_fb);
    assert_ne!(texture_vflip_framebuffer_fb, uv_vflip_framebuffer_fb);
}
