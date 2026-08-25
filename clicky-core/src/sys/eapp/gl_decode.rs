use super::rasterizer::TextureFormat;
use super::{
    GlFileBacking, GlFrameRecord, GlImportRecord, GlMemorySnapshot, GlRegisterSnapshot,
    GlStackWordSnapshot, GlTraceFixture,
};

/// Bytes per pixel for each standalone renderer texture format.
///
/// Mirrors the payload sizing used by the local-asset replay path so the
/// live HLE upload handler can validate/copy guest pixel bytes with the same
/// accounting.
pub fn pix_payload_size(format: TextureFormat, width: usize, height: usize) -> usize {
    let bytes_per_pixel = match format {
        TextureFormat::Rgb565 | TextureFormat::Rgba5551 | TextureFormat::Rgba4444 => 2,
        TextureFormat::Rgba8888 => 4,
        TextureFormat::LuminanceAlpha88 => 2,
        TextureFormat::A8 => 1,
    };
    width * height * bytes_per_pixel
}

/// Map captured GL ES 1.1 upload enumerants to the standalone renderer's
/// `TextureFormat`. Constants:
///   GL_RGB=0x1907, GL_RGBA=0x1908, GL_ALPHA=0x1906,
///   GL_LUMINANCE_ALPHA=0x190a
///   GL_UNSIGNED_SHORT_5_6_5=0x8363, GL_UNSIGNED_SHORT_5_5_5_1=0x8034,
///   GL_UNSIGNED_SHORT_4_4_4_4=0x8033, GL_UNSIGNED_BYTE=0x1401
///
/// Returns `None` for unsupported combinations so callers can log+skip.
pub fn format_from_gl(internal_format: u32, pixel_type: u32) -> Option<TextureFormat> {
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

#[derive(Debug, Clone, PartialEq)]
pub struct TextureUploadCandidate {
    pub ordinal45_seq: u64,
    pub ordinal4_seq: u64,
    pub ordinal99_seq: u64,
    pub object_tag: u32,
    pub descriptor_ptr: u32,
    pub descriptor_snapshot: Option<GlMemorySnapshot>,
    pub prep_width: u32,
    pub prep_height: u32,
    pub target: u32,
    pub level: u32,
    pub internal_format: u32,
    pub width: u32,
    pub height: u32,
    pub border: u32,
    pub source_format: u32,
    pub pixel_type: u32,
    pub source_ptr: u32,
    pub source_snapshot: Option<GlMemorySnapshot>,
    pub source_file: Option<GlFileBacking>,
    pub ret45: u32,
    pub ret4: u32,
    pub ret99: u32,
}

pub fn decode_fixed_16_16(word: u32) -> f32 {
    (word as i32) as f32 / 65536.0
}

pub fn words_from_snapshot(snapshot: &GlMemorySnapshot) -> Vec<u32> {
    let bytes = bytes_from_snapshot(snapshot);
    bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

pub fn bytes_from_snapshot(snapshot: &GlMemorySnapshot) -> Vec<u8> {
    (0..snapshot.bytes_hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&snapshot.bytes_hex[i..i + 2], 16).ok())
        .collect()
}

pub fn float_words_from_snapshot(snapshot: &GlMemorySnapshot) -> Vec<f32> {
    words_from_snapshot(snapshot)
        .into_iter()
        .map(f32::from_bits)
        .collect()
}

pub fn fixed_words_from_snapshot(snapshot: &GlMemorySnapshot) -> Vec<f32> {
    words_from_snapshot(snapshot)
        .into_iter()
        .map(decode_fixed_16_16)
        .collect()
}

pub fn texture_upload_candidates(fixture: &GlTraceFixture) -> Vec<TextureUploadCandidate> {
    let mut out = Vec::new();
    for frame in &fixture.frames {
        let mut i = 0;
        while i + 2 < frame.records.len() {
            let a = &frame.records[i];
            let b = &frame.records[i + 1];
            let c = &frame.records[i + 2];
            if a.ordinal == 45 && b.ordinal == 4 && c.ordinal == 99 {
                if let Some(candidate) = decode_texture_triplet(a, b, c) {
                    out.push(candidate);
                }
                i += 3;
            } else {
                i += 1;
            }
        }
    }
    out
}

pub fn register<'a>(record: &'a GlImportRecord, name: &str) -> Option<&'a GlRegisterSnapshot> {
    record.registers.iter().find(|reg| reg.name == name)
}

pub fn stack_word<'a>(
    record: &'a GlImportRecord,
    offset: usize,
) -> Option<&'a GlStackWordSnapshot> {
    record.stack_words.iter().find(|word| word.offset == offset)
}

fn decode_texture_triplet(
    ord45: &GlImportRecord,
    ord4: &GlImportRecord,
    ord99: &GlImportRecord,
) -> Option<TextureUploadCandidate> {
    let r45_0 = register(ord45, "r0")?.value;
    let r45_1 = register(ord45, "r1")?;
    let r45_2 = register(ord45, "r2")?.value;
    let r45_3 = register(ord45, "r3")?.value;
    let r4_0 = register(ord4, "r0")?.value;
    let r99_0 = register(ord99, "r0")?.value;
    let r99_1 = register(ord99, "r1")?.value;
    let r99_2 = register(ord99, "r2")?.value;
    let r99_3 = register(ord99, "r3")?.value;
    let height = stack_word(ord99, 0x00)?.value;
    let border = stack_word(ord99, 0x04)?.value;
    let source_format = stack_word(ord99, 0x08)?.value;
    let pixel_type = stack_word(ord99, 0x0c)?.value;
    let source = stack_word(ord99, 0x10)?;
    Some(TextureUploadCandidate {
        ordinal45_seq: ord45.seq,
        ordinal4_seq: ord4.seq,
        ordinal99_seq: ord99.seq,
        object_tag: r45_0,
        descriptor_ptr: r45_1.value,
        descriptor_snapshot: r45_1.snapshot.clone(),
        prep_width: r45_2,
        prep_height: r45_3,
        target: r4_0.max(r99_0),
        level: r99_1,
        internal_format: r99_2,
        width: r99_3,
        height,
        border,
        source_format,
        pixel_type,
        source_ptr: source.value,
        source_snapshot: source.snapshot.clone(),
        source_file: source
            .snapshot
            .as_ref()
            .and_then(|snap| snap.file_backing.clone()),
        ret45: ord45.return_value,
        ret4: ord4.return_value,
        ret99: ord99.return_value,
    })
}

pub fn first_frame<'a>(fixture: &'a GlTraceFixture, first_frame: u64) -> Option<&'a GlFrameRecord> {
    fixture
        .frames
        .iter()
        .find(|frame| frame.first_frame == first_frame)
}
