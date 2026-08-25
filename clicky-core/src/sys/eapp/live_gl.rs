//! Live OpenGLES HLE state for the experimental first-pass renderer.
//!
//! This module holds the *data* and *pure* helpers for the opt-in live GL HLE
//! path (`CLICKY_EXPERIMENTAL_GL_HLE=1`). All guest-memory access is performed
//! in `mod.rs` (where the bus lives); this module only reasons about decoded
//! state, texture selection, framebuffer presentation, and bounded diagnostics.
//!
//! Scope rules (see docs/EAPP_GL_TRACE_DECODER_REPORT.md):
//! - texture row order is preserved (no row inversion at decode time);
//! - captured UVs and guest geometry are preserved;
//! - the internal rasterizer framebuffer is kept in its native (unflipped) order;
//! - a vertical presentation flip is applied **only** when serializing/presenting;
//! - the flip is a diagnostic/presentation convenience, not a confirmed ABI rule.

use std::collections::{HashMap, HashSet};

use super::gl_decode::{format_from_gl, pix_payload_size};
use super::rasterizer::{
    framebuffer_hash, framebuffer_to_ppm, rasterize_quad_tinted, rasterize_solid_quad,
    rasterize_triangle_tinted, Rgba8, Texture, TextureFormat,
};

pub const FB_WIDTH: usize = 320;
pub const FB_HEIGHT: usize = 240;
pub const FB_PIXELS: usize = FB_WIDTH * FB_HEIGHT;

/// GL_FIXED (0x140c) enumerant confirmed by disassembly for the position/UV
/// arrays. Any other array format is preserved but not interpreted.
pub const GL_FIXED: u32 = 0x140c;

/// GL_UNSIGNED_SHORT (0x1403), observed as the index type for ordinal-38
/// `DrawElements` triangle strips in the Sims/Sudoku/Solitaire engine family.
pub const GL_UNSIGNED_SHORT: u32 = 0x1403;

/// Confirmed DrawArrays quad mode token observed at most ordinal-37 call sites.
pub const DRAW_MODE: u32 = 7;

/// Standard GL ES `GL_TRIANGLE_STRIP`, observed in Texas Hold'em as
/// `OpenGLES:37 mode=5 count=11`.
pub const DRAW_MODE_TRIANGLE_STRIP: u32 = 5;

/// The observed `mode=7` stream behaves like batched quads: count is always a
/// positive multiple of 4, and the existing Tetris path is the 1-quad case.
pub fn quad_group_count(mode: u32, first: usize, count: usize) -> Option<usize> {
    if mode != DRAW_MODE || first != 0 || count < 4 || count % 4 != 0 {
        None
    } else {
        Some(count / 4)
    }
}

/// A live texture upload captured at ordinal-99 call time. Pixel bytes are
/// copied immediately from guest memory; row order is preserved as uploaded.
#[derive(Debug, Clone)]
pub struct LiveGlUpload {
    pub index: usize,
    pub target: u32,
    pub width: usize,
    pub height: usize,
    pub source_format: u32,
    pub pixel_type: u32,
    pub source_ptr: u32,
    pub source_file: Option<String>,
    pub source_file_offset: Option<u32>,
    pub format: Option<TextureFormat>,
    pub texture: Option<Texture>,
    /// GL texture name this upload is bound to, decoded from the preceding
    /// ordinal-45 descriptor (Tetris/Holdem layout: descriptor word 1).
    /// `None` for uploads captured before ord45-tex-name tracking existed or
    /// for Mahjong resource uploads (which use `resource_uploads_by_handle`).
    pub tex_name: Option<u32>,
}

/// A vertex array definition recorded from ordinal-137. Unknown slots are
/// preserved verbatim without assigning unsupported semantic names.
#[derive(Debug, Clone, Default)]
pub struct LiveArrayDef {
    pub array_index: u32,
    pub component_count: u32,
    pub format: u32,
    pub stride: u32,
    pub guest_ptr: u32,
    pub valid: bool,
    pub material_epoch: u64,
}

/// One decoded ordinal-37 draw, recorded for diagnostics and comparison.
#[derive(Debug, Clone)]
pub struct LiveDrawRecord {
    pub draw_index: usize,
    pub handle: u32,
    pub state_ptr: u32,
    pub translation: (f32, f32),
    pub positions: [(f32, f32); 4],
    pub uvs: [(f32, f32); 4],
    pub has_uv: bool,
    pub solid_color: Option<Rgba8>,
    pub tint: Rgba8,
    pub used_generated_uvs: bool,
    pub position_array: Option<LiveArrayDef>,
    pub uv_array: Option<LiveArrayDef>,
    pub enabled_arrays: Vec<u32>,
    pub state_words: Vec<u32>,
    pub bounds: (f32, f32, f32, f32),
    pub coverage: u64,
    pub selected_upload: Option<usize>,
    pub inferred_dim: Option<(usize, usize)>,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BeginOutcome {
    Began,
    DoubleBegin,
}

#[derive(Debug, Clone)]
pub struct CompletedFrame {
    pub index: u64,
    pub draw_count: usize,
    pub skipped_draws: usize,
    pub internal_hash: u64,
    pub presented_hash: u64,
    pub handle_signature: Vec<u32>,
}

/// Persistent per-eapp live graphics state, sufficient for the observed
/// Tetris stream. Stored on `Eapp` only when the experimental flag is set.
pub struct LiveGlState {
    pub uploads: Vec<LiveGlUpload>,
    /// Material handle -> upload index for texture objects decoded from
    /// ordinal-45 resource descriptors. This is used only as evidence that a
    /// pre-bind UV array belongs to the material, not as a substitute for real
    /// UV coordinates.
    pub resource_uploads_by_handle: HashMap<u32, usize>,
    /// GL texture name captured from the most recent ordinal-45 descriptor
    /// (Tetris/Holdem layout, word 1). Consumed by the following ordinal-99
    /// `glTexImage2D` so its upload can be associated with the GL texture
    /// name later bound by ordinal 159 at draw time.
    pub pending_tex_name: Option<u32>,
    pub arrays: HashMap<u32, LiveArrayDef>,
    pub enabled_arrays: HashSet<u32>,
    pub current_handle: u32,
    pub current_state_ptr: u32,
    pub current_material_epoch: u64,
    pub translation: (f32, f32),
    /// Pointer-backed text materials issue one full base translation for the
    /// first glyph, then only per-glyph deltas before subsequent DrawArrays
    /// calls. Keep that accumulated text cursor separately so the generic
    /// per-draw translation reset used by normal sprites does not collapse the
    /// glyph run back to the origin.
    pub pointer_text_carry_handle: Option<u32>,
    pub pointer_text_carry: (f32, f32),
    pub framebuffer: Vec<Rgba8>,
    pub draws: Vec<LiveDrawRecord>,
    pub draw_count_in_frame: usize,
    pub candidate_frames: usize,
    pub captured_first_frame: bool,
    pub present_vflip: bool,
    /// If true, the current frame used NDC (0–1) positions that were scaled
    /// to pixel coords. NDC engine families render top-to-bottom, so the
    /// usual bottom-to-top vflip should be suppressed for these frames.
    pub ndc_frame: bool,
    pub gate_b: bool,
    pub continuous_capture: bool,
    pub last_frame_counter: u64,
    /// Draw-handle signature of the previous 4-draw frame, used to detect the
    /// steady-state frame (first consecutive repeat) for default-mode capture.
    pub prev_draw_handles: Option<Vec<u32>>,
    /// Tentative lifecycle observations around ordinals 157/158/165. We record
    /// the observed ordering but do not rename them present/begin/end.
    pub lifecycle_log: Vec<String>,
    /// Ordered (ordinal, handle) trace of GL calls in the current guest frame,
    /// used to determine the real frame lifecycle (begin/present) from evidence.
    pub ordinal_trace: Vec<(u32, u32)>,
    /// Bounded per-frame lifecycle summaries (first N frames) for diagnostics.
    pub lifecycle_reports: Vec<String>,
    pub lifecycle_report_budget: usize,
    /// Most recent presented framebuffer (post optional vflip), kept so Gate B
    /// can copy it to the desktop window independently of the internal buffer.
    pub presented: Option<Vec<Rgba8>>,
    // --- continuous frame assembly (double-buffered) ---
    /// Last fully-rendered internal frame (copied from `framebuffer` at
    /// present). The window never reads the active `framebuffer`.
    pub completed_buffer: Vec<Rgba8>,
    /// Host-facing presented buffer (completed + optional vflip).
    pub presented_buffer: Vec<Rgba8>,
    /// True between candidate begin (158) and present (157).
    pub frame_active: bool,
    /// True when a DMA overlay has been applied to the current frame.
    /// Causes complete_frame to use the framebuffer even with 0 GL draws.
    pub has_dma_overlay: bool,
    /// Game identifier for PPM dump filenames.
    pub game_id: String,
    /// Viewport dimensions from ordinal 153 (glViewport).
    /// Default 320×240 for iPod screen.
    pub viewport_w: u32,
    pub viewport_h: u32,
    /// Monotonic count of completed/presented frames.
    pub completed_frame_index: u64,
    /// Candidate frame-begin ordinal, derived from observed ordering (always
    /// precedes all draws). Neutral name; semantics not yet proven.
    pub candidate_begin_ordinal: u32,
    /// Candidate frame-present ordinal, derived from observed ordering (always
    /// follows all draws). Neutral name; semantics not yet proven.
    pub candidate_present_ordinal: u32,
    // --- per-frame diagnostics & anomaly detection ---
    pub skipped_draws_this_frame: usize,
    pub frame_anomalies: Vec<String>,
    pub diagnostics_budget: usize,
    // --- optional continuous frame dumping (CLICKY_GL_DUMP_FRAMES=N) ---
    pub dump_remaining: usize,
    pub dump_counter: usize,
    // --- consecutive-frame hash tracking ---
    pub first_presented_hash: Option<u64>,
    pub prev_presented_hash: Option<u64>,
    pub first_changed_frame: Option<u64>,
    pub unique_presented_hashes: HashSet<u64>,
    pub repeated_presented_count: u64,
    /// Per-frame scalar-formatter char sequences captured from the guest
    /// `text_push_char` callsite (e.g. `0x1801616c`). Keyed by the text_obj
    /// pointer passed as `r0`. Each call appends the char (`r1`) so that an
    /// ordered run like `HH:MM AM` becomes `['H','H',':','M','M','A','M']`.
    /// This is the general model for clickwheel-game runtime text pushers
    /// that compute chars in registers rather than writing a UTF-16 buffer.
    pub text_char_seqs: HashMap<u32, Vec<u32>>,
    /// Per-run consumption index into `text_char_seqs[text_obj]`. Advanced by
    /// one each time a draw consumes a recorded char. Reset on material bind
    /// and per-frame so each glyph run restarts at index 0.
    pub text_char_consumed: HashMap<u32, usize>,
}

impl LiveGlState {
    pub fn new(present_vflip: bool, gate_b: bool, continuous_capture: bool, game_id: String) -> Self {
        Self {
            uploads: Vec::new(),
            resource_uploads_by_handle: HashMap::new(),
            pending_tex_name: None,
            arrays: HashMap::new(),
            enabled_arrays: HashSet::new(),
            current_handle: 0,
            current_state_ptr: 0,
            current_material_epoch: 0,
            translation: (0.0, 0.0),
            pointer_text_carry_handle: None,
            pointer_text_carry: (0.0, 0.0),
            framebuffer: vec![Rgba8::rgba(0, 0, 0, 0); FB_PIXELS],
            draws: Vec::new(),
            draw_count_in_frame: 0,
            candidate_frames: 0,
            captured_first_frame: false,
            present_vflip,
            ndc_frame: false,
            gate_b,
            continuous_capture,
            last_frame_counter: 0,
            prev_draw_handles: None,
            lifecycle_log: Vec::new(),
            ordinal_trace: Vec::new(),
            lifecycle_reports: Vec::new(),
            lifecycle_report_budget: 120,
            completed_buffer: vec![Rgba8::rgba(0, 0, 0, 0); FB_PIXELS],
            presented_buffer: vec![Rgba8::rgba(0, 0, 0, 0); FB_PIXELS],
            frame_active: false,
            has_dma_overlay: false,
            game_id,
            viewport_w: 320,
            viewport_h: 240,
            completed_frame_index: 0,
            candidate_begin_ordinal: 158,
            candidate_present_ordinal: 157,
            skipped_draws_this_frame: 0,
            frame_anomalies: Vec::new(),
            diagnostics_budget: 120,
            dump_remaining: 0,
            dump_counter: 0,
            first_presented_hash: None,
            prev_presented_hash: None,
            first_changed_frame: None,
            unique_presented_hashes: HashSet::new(),
            repeated_presented_count: 0,
            presented: None,
            text_char_seqs: HashMap::new(),
            text_char_consumed: HashMap::new(),
        }
    }

    /// Reset per-frame accumulators. Uploads persist (they happen once at
    /// startup); arrays/enabled are cleared because they are redefined each
    /// frame by ordinal-137/40 calls.
    pub fn reset_for_frame(&mut self) {
        self.arrays.clear();
        self.enabled_arrays.clear();
        self.translation = (0.0, 0.0);
        self.pointer_text_carry_handle = None;
        self.pointer_text_carry = (0.0, 0.0);
        self.framebuffer = vec![Rgba8::rgba(0, 0, 0, 0); FB_PIXELS];
        self.draws.clear();
        self.draw_count_in_frame = 0;
        self.ordinal_trace.clear();
        self.ndc_frame = false;
        // Scalar-formatter char sequences are rebuilt by the guest each frame,
        // so drop the prior frame's recorded pushes+consumption.
        self.text_char_seqs.clear();
        self.text_char_consumed.clear();
    }

    /// Record one scalar-formatter char push captured at the guest
    /// `text_push_char` callsite (`r0=text_obj`, `r1=char`). The sequence is
    /// consumed in order by draws that bind this text_obj's handle. This is
    /// the general model for clickwheel-game runtime text pushers that pass
    /// chars in registers rather than writing a UTF-16 buffer.
    pub fn record_text_char_push(&mut self, text_obj: u32, char: u32) {
        self.text_char_seqs.entry(text_obj).or_default().push(char);
    }

    /// Take the next recorded char for `text_obj`, advancing the per-run
    /// consumption index. Returns `None` if no chars have been recorded for
    /// this text_obj or the run has already consumed all of them.
    pub fn take_text_char_for_draw(&mut self, text_obj: u32) -> Option<u32> {
        let seq = self.text_char_seqs.get(&text_obj)?;
        let idx = self.text_char_consumed.entry(text_obj).or_insert(0);
        if *idx >= seq.len() {
            return None;
        }
        let ch = seq[*idx];
        *idx += 1;
        Some(ch)
    }

    /// Reset the per-run consumption index for a text_obj on material bind,
    /// so a freshly-bound text run restarts its char consumption at index 0.
    pub fn reset_text_char_consumption(&mut self, text_obj: u32) {
        self.text_char_consumed.insert(text_obj, 0);
    }

    /// Diagnostic: format one line per text_obj showing the recorded push
    /// sequence (hex + ASCII) and how many were consumed by draws this frame.
    /// A mismatch (`pushed != consumed`) means the text_obj is reused across
    /// multiple text runs within the frame and a linear consumption counter
    /// mis-segments across run boundaries. Drained by `reset_for_frame`.
    pub fn take_text_char_diag(&mut self, frame: u64) -> Vec<String> {
        if self.text_char_seqs.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(self.text_char_seqs.len());
        // Sort by text_obj for stable log ordering across frames.
        let mut keys: Vec<u32> = self.text_char_seqs.keys().copied().collect();
        keys.sort_unstable();
        for text_obj in keys {
            let seq = self.text_seqs_consume_drain(&text_obj);
            let pushed = seq.len();
            let consumed = *self.text_char_consumed.get(&text_obj).unwrap_or(&0);
            let hex: Vec<String> = seq.iter().map(|c| format!("0x{:02x}", c)).collect();
            let ascii: String = seq
                .iter()
                .map(|&c| {
                    if (0x20..0x7f).contains(&c) {
                        c as u8 as char
                    } else {
                        '.'
                    }
                })
                .collect();
            let flag = if pushed != consumed { " MISMATCH" } else { "" };
            out.push(format!(
                "text_char_diag frame={} text_obj={:#010x} pushed={} consumed={} ascii=\"{}\" hex=[{}]{}",
                frame, text_obj, pushed, consumed, ascii, hex.join(","), flag
            ));
        }
        out
    }

    // Helper: borrow the seq for a text_obj immutably for formatting. (Cannot
    // be a simple closure borrow with the HashMap API, so this just clones the
    // hex/ascii representation without mutating.)
    fn text_seqs_consume_drain(&self, text_obj: &u32) -> Vec<u32> {
        self.text_char_seqs.get(text_obj).cloned().unwrap_or_default()
    }

    /// Format the current frame's ordinal trace into a compact one-line
    /// summary and drain it. Draw ordinals (37) are annotated with their
    /// 1-based draw index; surface/material ordinals (157/158/165/159) include
    /// their handle so begin/present ordering can be read directly.
    pub fn take_frame_trace_summary(
        &mut self,
        frame_index: u64,
        draw_count: usize,
    ) -> Option<String> {
        if self.ordinal_trace.is_empty() {
            return None;
        }
        let mut draw_idx = 0usize;
        let mut first_surface: Option<u32> = None;
        let mut last_surface: Option<u32> = None;
        let mut rendered = String::new();
        for (ord, handle) in self.ordinal_trace.drain(..) {
            if matches!(ord, 157 | 158 | 165) {
                if first_surface.is_none() {
                    first_surface = Some(ord);
                }
                last_surface = Some(ord);
            }
            if !rendered.is_empty() {
                rendered.push(',');
            }
            if ord == 37 {
                draw_idx += 1;
                rendered.push_str(&format!("37#{}", draw_idx));
            } else if matches!(ord, 157 | 158 | 165 | 159) {
                rendered.push_str(&format!("{}(h{:#x})", ord, handle));
            } else {
                rendered.push_str(&format!("{}", ord));
            }
        }
        Some(format!(
            "lifecycle frame={} draws={} first_surface={} last_surface={} trace=[{}]",
            frame_index,
            draw_count,
            first_surface
                .map(|o| o.to_string())
                .unwrap_or_else(|| "none".into()),
            last_surface
                .map(|o| o.to_string())
                .unwrap_or_else(|| "none".into()),
            rendered
        ))
    }

    /// Outcome of a candidate begin event (ordinal 158).
    pub fn begin_frame(&mut self) -> BeginOutcome {
        // Stale-state check: arrays should have been cleared by the boundary
        // reset. If not, the previous frame's array state leaked across.
        if !self.arrays.is_empty() {
            self.push_anomaly(format!(
                "stale_array_state_at_begin ordinal={} leaked_arrays={}",
                self.candidate_begin_ordinal,
                self.arrays.len()
            ));
        }
        self.skipped_draws_this_frame = 0;
        self.has_dma_overlay = false;
        if self.frame_active {
            // 158 received while a frame is already active → the previous
            // frame never received a 157 (incomplete / missing present).
            self.push_anomaly(format!(
                "incomplete_frame double_begin ordinal={} previous_not_presented draws={}",
                self.candidate_begin_ordinal,
                self.draws.len()
            ));
            BeginOutcome::DoubleBegin
        } else {
            self.frame_active = true;
            BeginOutcome::Began
        }
    }

    /// Finalize the active frame at the candidate present event (ordinal 157).
    /// Copies active → completed → presented (with optional vflip) and returns
    /// Composite the DMA-rendered RGB565 background with the live_gl
    /// framebuffer. PopCap engine writes its background into hardware DMA
    /// buffer at 0x1402_0000, then GL draws overlay text/sprites on top.
    /// This method takes the DMA background, then alpha-blends the current
    /// framebuffer (which has transparent background + opaque sprites) on top.
    pub fn overlay_dma_rgb565(&mut self, rgb565: &[u8]) {
        self.has_dma_overlay = true;
        if rgb565.len() < FB_WIDTH * FB_HEIGHT * 2 {
            return;
        }
        let mut nonzero_count = 0usize;
        for y in 0..FB_HEIGHT {
            // DMA framebuffer is in display orientation (top-to-bottom).
            // GL framebuffer is in OpenGL orientation (bottom-to-top, needs vflip).
            // When overlaying DMA data, always read top-to-bottom regardless
            // of vflip setting — the vflip is applied separately during
            // present() to the entire composited framebuffer.
            let src_y = y;
            for x in 0..FB_WIDTH {
                let src_idx = (src_y * FB_WIDTH + x) * 2;
                let raw = u16::from_le_bytes([rgb565[src_idx], rgb565[src_idx + 1]]);
                if raw != 0 {
                    nonzero_count += 1;
                }
                let r = ((raw >> 11) & 0x1F) as u8;
                let g = ((raw >> 5) & 0x3F) as u8;
                let b = (raw & 0x1F) as u8;
                let r8 = (r << 3) | (r >> 2);
                let g8 = (g << 2) | (g >> 4);
                let b8 = (b << 3) | (b >> 2);
                let dst = self.framebuffer[y * FB_WIDTH + x];
                let alpha = dst.a as u32;
                // Alpha blend: dst over DMA background
                let inv_a = 255 - alpha;
                let out_r = ((r8 as u32 * inv_a + dst.r as u32 * alpha) / 255) as u8;
                let out_g = ((g8 as u32 * inv_a + dst.g as u32 * alpha) / 255) as u8;
                let out_b = ((b8 as u32 * inv_a + dst.b as u32 * alpha) / 255) as u8;
                self.framebuffer[y * FB_WIDTH + x] = Rgba8::rgba(out_r, out_g, out_b, 255);
            }
        }
        if nonzero_count > 0 && self.completed_frame_index < 20 {
            log::info!(target: "EAPP_GL", "DMA overlay: {}/{} non-zero pixels", nonzero_count, FB_PIXELS);
        }
    }

    /// the completed-frame metadata. Returns None if no frame is active
    /// (present without begin). The active `framebuffer` is left untouched;
    /// it is cleared by the next boundary reset / begin.
    pub fn complete_frame(&mut self) -> Option<CompletedFrame> {
        if !self.frame_active {
            self.push_anomaly(format!(
                "present_without_active_frame ordinal={}",
                self.candidate_present_ordinal
            ));
            return None;
        }
        self.frame_active = false;
        let draw_count = self.draws.len();
        if draw_count == 0 && !self.has_dma_overlay {
            self.push_anomaly(format!(
                "clear_without_draws ordinal={} (present with zero draws)",
                self.candidate_present_ordinal
            ));
            // 0-draw frames (input-wait idle loops in Sudoku/Solitaire)
            // would overwrite the good framebuffer with the cleared (black)
            // content. Instead, keep the previously presented frame and just
            // advance the index.
            self.completed_frame_index += 1;
            let prev_hash = framebuffer_hash(&self.presented_buffer);
            return Some(CompletedFrame {
                index: self.completed_frame_index,
                draw_count,
                skipped_draws: self.skipped_draws_this_frame,
                internal_hash: framebuffer_hash(&self.completed_buffer),
                presented_hash: prev_hash,
                handle_signature: vec![],
            });
        }
        if draw_count != 0 && draw_count != 4 {
            self.push_anomaly(format!(
                "unexpected_draw_count ordinal={} draws={} (steady=4)",
                self.candidate_present_ordinal, draw_count
            ));
        }

        self.completed_buffer.copy_from_slice(&self.framebuffer);
        let mut presented = self.framebuffer.clone();
        // Pixel-coord engines (Tetris) render bottom-to-top so need vflip.
        // NDC engines (Sudoku/Solitaire) render top-to-bottom and don't.
        if self.present_vflip && !self.ndc_frame {
            flip_vertical_in_place(&mut presented, FB_WIDTH, FB_HEIGHT);
        }
        self.presented_buffer.copy_from_slice(&presented);
        self.presented = Some(presented);
        self.completed_frame_index += 1;

        let internal_hash = framebuffer_hash(&self.completed_buffer);
        let presented_hash = framebuffer_hash(&self.presented_buffer);
        let handle_signature: Vec<u32> = self.draws.iter().map(|d| d.handle).collect();

        // Consecutive-frame hash tracking (req 12). A repeated splash is not
        // treated as broken.
        if self.first_presented_hash.is_none() {
            self.first_presented_hash = Some(presented_hash);
        }
        if self.prev_presented_hash == Some(presented_hash) {
            self.repeated_presented_count += 1;
        } else if self.completed_frame_index > 1 && self.first_changed_frame.is_none() {
            self.first_changed_frame = Some(self.completed_frame_index);
        }
        self.prev_presented_hash = Some(presented_hash);
        self.unique_presented_hashes.insert(presented_hash);

        Some(CompletedFrame {
            index: self.completed_frame_index,
            draw_count,
            skipped_draws: self.skipped_draws_this_frame,
            internal_hash,
            presented_hash,
            handle_signature,
        })
    }

    /// Mark a draw observed while no frame is active (anomaly). Auto-begins so
    /// rendering continues without crashing.
    pub fn note_draw_outside_frame(&mut self) {
        self.push_anomaly("draw_outside_active_frame".to_string());
        self.frame_active = true;
    }

    /// Record a skipped draw (e.g. unresolved handle 3).
    pub fn note_skipped_draw(&mut self, reason: String) {
        self.skipped_draws_this_frame += 1;
        self.push_anomaly(format!("skipped_draw {}", reason));
    }

    fn push_anomaly(&mut self, msg: String) {
        // Bounded; keep enough to diagnose the first ~120 frames.
        if self.frame_anomalies.len() < self.diagnostics_budget * 4 {
            self.frame_anomalies.push(msg);
        }
    }

    /// Build a `LiveGlUpload` from decoded ordinal-99 arguments, copying the
    /// supplied guest pixel bytes immediately. Row order is preserved.
    pub fn build_upload(
        index: usize,
        target: u32,
        width: u32,
        height: u32,
        source_format: u32,
        pixel_type: u32,
        source_ptr: u32,
        payload: &[u8],
        tex_name: Option<u32>,
    ) -> LiveGlUpload {
        let format = format_from_gl(source_format, pixel_type);
        let texture = format.and_then(|fmt| {
            let expected = pix_payload_size(fmt, width as usize, height as usize);
            if payload.len() < expected {
                return None;
            }
            Some(Texture::from_bytes(
                &payload[..expected],
                width as usize,
                height as usize,
                fmt,
                // A8 tint: white, matching the offline replay convention.
                Rgba8::rgba(255, 255, 255, 255),
            ))
        });
        LiveGlUpload {
            index,
            target,
            width: width as usize,
            height: height as usize,
            source_format,
            pixel_type,
            source_ptr,
            source_file: None,
            source_file_offset: None,
            format,
            texture,
            tex_name,
        }
    }

    /// Select the best-supported live texture by matching decoded draw
    /// dimensions. This is an *inferred* association (logged as such); it
    /// prefers live upload evidence (dimensions/format) over filenames.
    pub fn select_upload_by_dims(&self, w: usize, h: usize) -> Option<usize> {
        self.uploads
            .iter()
            .find(|u| u.texture.is_some() && u.width == w && u.height == h)
            .map(|u| u.index)
    }

    /// Select a live texture by its decoded GL texture name. This is the most
    /// reliable association when ord45 supplied a tex-name in its descriptor
    /// (Tetris/Holdem layout). Prefers the most recent matching upload so that
    /// level-0 reloads replace earlier ones. Only matches uploads that actually
    /// decoded a texture.
    pub fn select_upload_by_tex_name(&self, tex_name: u32) -> Option<usize> {
        self.uploads
            .iter()
            .rev()
            .find(|u| u.texture.is_some() && u.tex_name == Some(tex_name))
            .map(|u| u.index)
    }

    /// Select a live texture by texture name only if the chosen upload can
    /// contain the supplied texel-centered UV extents. Some Tetris A8 resources
    /// are all tagged with the same small texture name (`0x8`); blindly picking
    /// the latest matching name pins unrelated menu/spinner draws to the last
    /// uploaded font sheet. Rejecting non-containing uploads lets the existing
    /// UV/dimension fallback choose the intended resource.
    fn select_upload_by_tex_name_containing_slice(
        &self,
        tex_name: u32,
        uvs: &[(f32, f32)],
    ) -> Option<usize> {
        let (_min_u, _min_v, max_u, max_v) = uv_extents_slice(uvs);
        let need_w = max_u.ceil().max(1.0) as usize;
        let need_h = max_v.ceil().max(1.0) as usize;
        self.select_upload_by_tex_name(tex_name).filter(|idx| {
            self.uploads
                .get(*idx)
                .map(|u| u.width >= need_w && u.height >= need_h)
                .unwrap_or(false)
        })
    }

    fn select_upload_by_tex_name_containing(
        &self,
        tex_name: u32,
        uvs: &[(f32, f32); 4],
    ) -> Option<usize> {
        self.select_upload_by_tex_name_containing_slice(tex_name, uvs)
    }

    /// Select a live texture for the supplied texel-centered UVs. Full-texture
    /// quads match by exact UV span; atlas sub-rects (e.g. Tetris menu A8
    /// strips) match the smallest decoded upload that contains the UV extents.
    fn select_upload_for_uvs(&self, uvs: &[(f32, f32); 4]) -> Option<usize> {
        self.select_upload_for_uv_slice(uvs)
    }

    fn select_upload_for_uv_slice(&self, uvs: &[(f32, f32)]) -> Option<usize> {
        let (min_u, min_v, max_u, max_v) = uv_extents_slice(uvs);
        let span_w = (max_u - min_u).round().max(1.0) as usize;
        let span_h = (max_v - min_v).round().max(1.0) as usize;
        if let Some(idx) = self.select_upload_by_dims(span_w, span_h) {
            return Some(idx);
        }

        self.select_smallest_containing_upload(max_u, max_v)
    }

    fn select_upload_for_uv_slice_with_tex_name(
        &self,
        tex_name: u32,
        uvs: &[(f32, f32)],
    ) -> Option<usize> {
        let (min_u, min_v, max_u, max_v) = uv_extents_slice(uvs);
        let span_w = (max_u - min_u).round().max(1.0) as usize;
        let span_h = (max_v - min_v).round().max(1.0) as usize;
        if let Some(idx) = self
            .uploads
            .iter()
            .rev()
            .find(|u| {
                u.texture.is_some()
                    && u.tex_name == Some(tex_name)
                    && u.width == span_w
                    && u.height == span_h
            })
            .map(|u| u.index)
        {
            return Some(idx);
        }

        self.select_smallest_containing_upload_with_tex_name(tex_name, max_u, max_v)
    }

    /// Generated text UVs describe one glyph cell inside a font atlas. Prefer
    /// A8 uploads whose dimensions are exact multiples of that cell size before
    /// falling back to the generic "smallest containing texture" rule. This
    /// keeps Tetris text glyphs on f10x12/f16x16 font atlases instead of small
    /// unrelated A8 UI strips that merely contain the same UV extents.
    fn select_upload_for_generated_text_uvs(&self, uvs: &[(f32, f32); 4]) -> Option<usize> {
        let (_min_u, _min_v, max_u, max_v) = uv_extents(uvs);
        let (span_w, span_h) = infer_dims_from_uvs(uvs);
        let cell_w = span_w.saturating_add(1).max(1);
        let cell_h = span_h.saturating_add(1).max(1);
        let need_w = max_u.ceil().max(1.0) as usize;
        let need_h = max_v.ceil().max(1.0) as usize;
        self.uploads
            .iter()
            .filter(|u| {
                u.texture.is_some()
                    && u.format == Some(TextureFormat::A8)
                    && u.width >= need_w
                    && u.height >= need_h
                    && u.width % cell_w == 0
                    && u.height % cell_h == 0
                    && (u.width / cell_w) >= 32
            })
            .min_by_key(|u| (u.width * u.height, u.index))
            .map(|u| u.index)
            .or_else(|| self.select_smallest_containing_upload(max_u, max_v))
    }

    fn select_smallest_containing_upload(&self, max_u: f32, max_v: f32) -> Option<usize> {
        let need_w = max_u.ceil().max(1.0) as usize;
        let need_h = max_v.ceil().max(1.0) as usize;
        self.uploads
            .iter()
            .filter(|u| u.texture.is_some() && u.width >= need_w && u.height >= need_h)
            .min_by_key(|u| (u.width * u.height, u.index))
            .map(|u| u.index)
    }

    fn select_smallest_containing_upload_with_tex_name(
        &self,
        tex_name: u32,
        max_u: f32,
        max_v: f32,
    ) -> Option<usize> {
        let need_w = max_u.ceil().max(1.0) as usize;
        let need_h = max_v.ceil().max(1.0) as usize;
        self.uploads
            .iter()
            .filter(|u| {
                u.texture.is_some()
                    && u.tex_name == Some(tex_name)
                    && u.width >= need_w
                    && u.height >= need_h
            })
            .min_by_key(|u| (u.width * u.height, u.index))
            .map(|u| u.index)
    }

    /// Rasterize one draw into the internal framebuffer using the existing
    /// rasterizer. Returns the produced `LiveDrawRecord`.
    pub fn rasterize_draw(
        &mut self,
        draw_index: usize,
        handle: u32,
        state_ptr: u32,
        translation: (f32, f32),
        positions: [(f32, f32); 4],
        uvs: [(f32, f32); 4],
        has_uv: bool,
        solid_color: Option<Rgba8>,
        tint: Rgba8,
        used_generated_uvs: bool,
    ) -> LiveDrawRecord {
        let bounds = bounds_for(&positions);
        let inferred_dim = if has_uv {
            let (w, h) = infer_dims_from_uvs(&uvs);
            Some((w, h))
        } else {
            None
        };

        let selected_upload = if has_uv && used_generated_uvs && (0x1000_0000..0x1080_0000).contains(&handle) {
            self.select_upload_by_tex_name_containing(handle, &uvs)
                .or_else(|| self.select_upload_for_generated_text_uvs(&uvs))
        } else if has_uv {
            self.select_upload_by_tex_name_containing(handle, &uvs)
                .or_else(|| self.select_upload_for_uv_slice_with_tex_name(handle, &uvs))
                .or_else(|| self.select_upload_for_uvs(&uvs))
                .or_else(|| {
                    if state_ptr != 0 && state_ptr < 0x1000_0000 && state_ptr != handle {
                        self.select_upload_for_uv_slice_with_tex_name(state_ptr, &uvs)
                    } else {
                        None
                    }
                })
        } else {
            self.select_upload_by_tex_name(handle)
        };

        let mut record = LiveDrawRecord {
            draw_index,
            handle,
            state_ptr,
            translation,
            positions,
            uvs,
            has_uv,
            solid_color,
            tint,
            used_generated_uvs,
            position_array: None,
            uv_array: None,
            enabled_arrays: Vec::new(),
            state_words: Vec::new(),
            bounds,
            coverage: 0,
            selected_upload,
            inferred_dim,
            skipped_reason: None,
        };

        // NDC-to-pixel scaling for engine families that pass 0–1 positions.
        // These engines use a projection where the full screen maps to a
        // (0,0)–(1.something, 0.something) NDC range. Rather than just
        // multiplying by screen dims (which clips if max > 1.0), we treat
        // the NDC quad as if it should fill the viewport: find the min/max
        // extents and scale them to (0, FB_WIDTH) × (0, FB_HEIGHT).
        let max_coord = positions.iter().map(|p| p.0.max(p.1)).fold(0.0f32, f32::max);
        let pixel_positions = if max_coord < 2.0 {
            self.ndc_frame = true;
            let (min_x, min_y, max_x, max_y) = bounds;
            // Shift and scale so (min_x, min_y)–(max_x, max_y) maps to
            // (0, 0)–(FB_WIDTH, FB_HEIGHT).
            let range_x = (max_x - min_x).max(0.001);
            let range_y = (max_y - min_y).max(0.001);
            positions.map(|(x, y)| {
                ((x - min_x) / range_x * FB_WIDTH as f32,
                 (y - min_y) / range_y * FB_HEIGHT as f32)
            })
        } else {
            positions
        };

        if handle == 0x3 {
            if let Some(color) = solid_color {
                record.selected_upload = None;
                record.coverage = rasterize_solid_quad(
                    &mut self.framebuffer,
                    FB_WIDTH,
                    FB_HEIGHT,
                    color,
                    &pixel_positions,
                );
                return record;
            }
        }

        let Some(upload_idx) = selected_upload else {
            if let Some(color) = solid_color {
                record.coverage = rasterize_solid_quad(
                    &mut self.framebuffer,
                    FB_WIDTH,
                    FB_HEIGHT,
                    color,
                    &pixel_positions,
                );
                return record;
            }
            record.skipped_reason = Some(format!(
                "no live upload matched UV span {:?} (handle={:#x})",
                inferred_dim, handle
            ));
            return record;
        };
        let Some(texture) = self.uploads.get(upload_idx).and_then(|u| u.texture.clone()) else {
            record.skipped_reason = Some(format!("upload #{upload_idx} has no decoded texture"));
            return record;
        };

        record.coverage = rasterize_quad_tinted(
            &mut self.framebuffer,
            FB_WIDTH,
            FB_HEIGHT,
            &texture,
            &pixel_positions,
            &uvs,
            tint,
        );
        record
    }

    pub fn rasterize_triangle_strip_record(
        &mut self,
        draw_index: usize,
        handle: u32,
        state_ptr: u32,
        translation: (f32, f32),
        positions: &[(f32, f32)],
        uvs: Option<&[(f32, f32)]>,
        tint: Rgba8,
    ) -> LiveDrawRecord {
        let positions4 = first_four_positions(positions);
        let uvs4 = uvs.map(first_four_uvs).unwrap_or([(0.0, 0.0); 4]);
        let inferred_dim = uvs.map(infer_dims_from_uv_slice);
        let selected_upload = uvs
            .and_then(|uvs| self.select_upload_by_tex_name_containing_slice(handle, uvs))
            .or_else(|| uvs.and_then(|uvs| self.select_upload_for_uv_slice_with_tex_name(handle, uvs)))
            .or_else(|| uvs.and_then(|uvs| self.select_upload_for_uv_slice(uvs)))
            // 4th fallback: some engines (e.g. Solitaire) put the GL texture
            // name in state_ptr rather than handle. Try matching by state_ptr.
            .or_else(|| {
                if state_ptr != 0 && state_ptr < 0x1000_0000 && state_ptr != handle {
                    uvs.and_then(|uvs| self.select_upload_for_uv_slice_with_tex_name(state_ptr, uvs))
                } else {
                    None
                }
            });
        let mut record = LiveDrawRecord {
            draw_index,
            handle,
            state_ptr,
            translation,
            positions: positions4,
            uvs: uvs4,
            has_uv: uvs.is_some(),
            solid_color: None,
            tint,
            used_generated_uvs: false,
            position_array: None,
            uv_array: None,
            enabled_arrays: Vec::new(),
            state_words: Vec::new(),
            bounds: bounds_for_slice(positions),
            coverage: 0,
            selected_upload,
            inferred_dim,
            skipped_reason: None,
        };
        let Some(upload_idx) = selected_upload else {
            record.skipped_reason = Some(format!(
                "no live upload matched triangle-strip UV span {:?} (handle={:#x})",
                inferred_dim, handle
            ));
            return record;
        };
        let Some(texture) = self.uploads.get(upload_idx).and_then(|u| u.texture.clone()) else {
            record.skipped_reason = Some(format!("upload #{upload_idx} has no decoded texture"));
            return record;
        };

        // NDC-to-pixel scaling for engine families that pass 0–1 positions.
        let max_coord = positions.iter().map(|p| p.0.max(p.1)).fold(0.0f32, f32::max);
        let pixel_positions: Vec<(f32, f32)> = if max_coord < 2.0 {
            self.ndc_frame = true;
            let min_x = positions.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
            let min_y = positions.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
            let max_x = positions.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
            let max_y = positions.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
            let range_x = (max_x - min_x).max(0.001);
            let range_y = (max_y - min_y).max(0.001);
            positions
                .iter()
                .map(|(x, y)| (
                    (x - min_x) / range_x * FB_WIDTH as f32,
                    (y - min_y) / range_y * FB_HEIGHT as f32,
                ))
                .collect()
        } else {
            positions.to_vec()
        };

        if let Some(uvs) = uvs {
            for i in 0..pixel_positions.len().saturating_sub(2) {
                let tri = [
                    (pixel_positions[i].0, pixel_positions[i].1, uvs[i].0, uvs[i].1),
                    (
                        pixel_positions[i + 1].0,
                        pixel_positions[i + 1].1,
                        uvs[i + 1].0,
                        uvs[i + 1].1,
                    ),
                    (
                        pixel_positions[i + 2].0,
                        pixel_positions[i + 2].1,
                        uvs[i + 2].0,
                        uvs[i + 2].1,
                    ),
                ];
                record.coverage += rasterize_triangle_tinted(
                    &mut self.framebuffer,
                    FB_WIDTH,
                    FB_HEIGHT,
                    &texture,
                    &tri,
                    tint,
                );
            }
        }
        record
    }

    /// Produce the presented framebuffer (a copy), applying the configurable
    /// vertical presentation flip when enabled. The internal framebuffer is
    /// never mutated by presentation.
    pub fn present(&self) -> Vec<Rgba8> {
        let mut out = self.framebuffer.clone();
        if self.present_vflip && !self.ndc_frame {
            flip_vertical_in_place(&mut out, FB_WIDTH, FB_HEIGHT);
        }
        out
    }

    pub fn internal_hash(&self) -> u64 {
        framebuffer_hash(&self.framebuffer)
    }

    pub fn presented_hash(&self) -> u64 {
        let presented = self.present();
        framebuffer_hash(&presented)
    }

    /// Write both diagnostic PPMs (internal = native order, presented = with
    /// optional vflip). Returns true if both writes succeeded.
    pub fn write_diagnostic_ppms(
        &self,
        internal_path: &std::path::Path,
        presented_path: &std::path::Path,
    ) -> bool {
        let presented = self.present();
        let ok_a = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            framebuffer_to_ppm(internal_path, &self.framebuffer, FB_WIDTH, FB_HEIGHT);
        }))
        .is_ok();
        let ok_b = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            framebuffer_to_ppm(presented_path, &presented, FB_WIDTH, FB_HEIGHT);
        }))
        .is_ok();
        ok_a && ok_b
    }
}

fn bounds_for(positions: &[(f32, f32); 4]) -> (f32, f32, f32, f32) {
    bounds_for_slice(positions)
}

fn bounds_for_slice(positions: &[(f32, f32)]) -> (f32, f32, f32, f32) {
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

fn first_four_positions(positions: &[(f32, f32)]) -> [(f32, f32); 4] {
    let mut out = [(0.0, 0.0); 4];
    for (dst, src) in out.iter_mut().zip(positions.iter().copied()) {
        *dst = src;
    }
    out
}

fn first_four_uvs(uvs: &[(f32, f32)]) -> [(f32, f32); 4] {
    let mut out = [(0.0, 0.0); 4];
    for (dst, src) in out.iter_mut().zip(uvs.iter().copied()) {
        *dst = src;
    }
    out
}

/// Infer intended texture dimensions from texel-centered UVs. The captured
/// UVs are half-texel centered (e.g. 0.5 .. 50.5 for a 50px texture), so the
/// span rounds to the texture dimension.
fn uv_extents(uvs: &[(f32, f32); 4]) -> (f32, f32, f32, f32) {
    uv_extents_slice(uvs)
}

fn uv_extents_slice(uvs: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    uvs.iter().fold(
        (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ),
        |acc, (u, v)| (acc.0.min(*u), acc.1.min(*v), acc.2.max(*u), acc.3.max(*v)),
    )
}

fn infer_dims_from_uvs(uvs: &[(f32, f32); 4]) -> (usize, usize) {
    infer_dims_from_uv_slice(uvs)
}

fn infer_dims_from_uv_slice(uvs: &[(f32, f32)]) -> (usize, usize) {
    let (min_u, min_v, max_u, max_v) = uv_extents_slice(uvs);
    let w = (max_u - min_u).round().max(1.0) as usize;
    let h = (max_v - min_v).round().max(1.0) as usize;
    (w, h)
}

/// Flip a framebuffer vertically in place. Used only for presentation output.
pub fn flip_vertical_in_place(fb: &mut [Rgba8], width: usize, height: usize) {
    for y in 0..(height / 2) {
        let top = y * width;
        let bottom = (height - 1 - y) * width;
        for col in 0..width {
            fb.swap(top + col, bottom + col);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb565_2x2() -> Vec<u8> {
        // 4 pixels, 2 bytes each, all distinct so flips are detectable.
        vec![0x00, 0xf8, 0xe0, 0x07, 0x1f, 0x00, 0xff, 0xff]
    }

    #[test]
    fn build_upload_decodes_pixels_and_preserves_dims() {
        let payload = rgb565_2x2();
        let upload =
            LiveGlState::build_upload(0, 0x0de1, 2, 2, 0x1907, 0x8363, 0x1000_0000, &payload, None);
        assert_eq!(upload.format, Some(TextureFormat::Rgb565));
        assert_eq!(upload.width, 2);
        assert_eq!(upload.height, 2);
        let tex = upload.texture.expect("texture decoded");
        assert_eq!(tex.width, 2);
        assert_eq!(tex.height, 2);
        assert_eq!(tex.pixels.len(), 4);
        // top-left pixel is red in the source 565 layout
        assert_eq!(tex.pixels[0].r, 255);
    }

    #[test]
    fn build_upload_rejects_short_payload() {
        let payload = vec![0u8; 2]; // far too short for 2x2 RGB565
        let upload =
            LiveGlState::build_upload(0, 0x0de1, 2, 2, 0x1907, 0x8363, 0x1000_0000, &payload, None);
        assert_eq!(upload.format, Some(TextureFormat::Rgb565));
        assert!(upload.texture.is_none(), "short payload must not decode");
    }

    #[test]
    fn build_upload_rejects_unsupported_format() {
        let upload = LiveGlState::build_upload(0, 0x0de1, 2, 2, 0xdead, 0xbeef, 0x1000_0000, &[], None);
        assert!(upload.format.is_none());
        assert!(upload.texture.is_none());
    }

    #[test]
    fn select_upload_prefers_tex_name_then_falls_back_to_dims() {
        let mut lg = LiveGlState::new(true, false, false, "test".to_string());
        // Two uploads with identical dimensions but distinct GL texture names.
        lg.uploads.push(LiveGlState::build_upload(
            0,
            0x0de1,
            50,
            50,
            0x1908,
            0x8034,
            0x1000_0000,
            &vec![0u8; 50 * 50 * 2],
            Some(0x13),
        ));
        lg.uploads.push(LiveGlState::build_upload(
            1,
            0x0de1,
            50,
            50,
            0x1908,
            0x8034,
            0x1000_0010,
            &vec![0u8; 50 * 50 * 2],
            Some(0x23),
        ));
        // Draw bound to handle 0x23 must pick upload #1, not the dim-matched #0.
        assert_eq!(lg.select_upload_by_tex_name(0x23), Some(1));
        assert_eq!(lg.select_upload_by_tex_name(0x13), Some(0));
        // Unknown handle falls back to None (caller then uses dim/UV matching).
        assert_eq!(lg.select_upload_by_tex_name(0x99), None);
        // Reloads of the same name resolve to the most recent upload.
        lg.uploads.push(LiveGlState::build_upload(
            2,
            0x0de1,
            50,
            50,
            0x1908,
            0x8034,
            0x1000_0020,
            &vec![0u8; 50 * 50 * 2],
            Some(0x13),
        ));
        assert_eq!(lg.select_upload_by_tex_name(0x13), Some(2));
    }

    #[test]
    fn select_upload_matches_by_dimensions() {
        let mut lg = LiveGlState::new(true, false, false, "test".to_string());
        lg.uploads.push(LiveGlState::build_upload(
            0,
            0x0de1,
            50,
            50,
            0x1908,
            0x8034,
            0x1000_0000,
            &vec![0u8; 50 * 50 * 2],
            None,
        ));
        lg.uploads.push(LiveGlState::build_upload(
            1,
            0x0de1,
            250,
            162,
            0x1908,
            0x8033,
            0x1000_0010,
            &vec![0u8; 250 * 162 * 2],
            None,
        ));
        assert_eq!(lg.select_upload_by_dims(50, 50), Some(0));
        assert_eq!(lg.select_upload_by_dims(250, 162), Some(1));
        assert_eq!(lg.select_upload_by_dims(999, 999), None);
    }

    #[test]
    fn select_upload_for_uvs_uses_smallest_containing_atlas_when_span_is_subrect() {
        let mut lg = LiveGlState::new(true, false, false, "test".to_string());
        lg.uploads.push(LiveGlState::build_upload(
            0,
            0x0de1,
            980,
            24,
            0x1906,
            0x1401,
            0x1000_0000,
            &vec![0xff; 980 * 24],
            None,
        ));
        lg.uploads.push(LiveGlState::build_upload(
            1,
            0x0de1,
            320,
            99,
            0x1906,
            0x1401,
            0x1000_0010,
            &vec![0xff; 320 * 99],
            None,
        ));
        let menu_strip_uvs = [(0.5, 37.5), (0.5, 3.5), (308.5, 3.5), (308.5, 37.5)];
        assert_eq!(lg.select_upload_for_uvs(&menu_strip_uvs), Some(1));
    }

    #[test]
    fn tex_name_match_must_contain_uvs_before_it_wins() {
        let mut lg = LiveGlState::new(true, false, false, "test".to_string());
        // Intended menu strip upload.
        lg.uploads.push(LiveGlState::build_upload(
            0,
            0x0de1,
            320,
            99,
            0x1906,
            0x1401,
            0x1000_0000,
            &vec![0xff; 320 * 99],
            Some(0x8),
        ));
        // Later A8 font upload with the same ambiguous tex name; this was
        // incorrectly selected for menu-strip UVs even though height 32 cannot
        // contain v=60.5.
        lg.uploads.push(LiveGlState::build_upload(
            1,
            0x0de1,
            1568,
            32,
            0x1906,
            0x1401,
            0x1000_0010,
            &vec![0xff; 1568 * 32],
            Some(0x8),
        ));
        let menu_strip_uvs = [(0.5, 60.5), (0.5, 39.5), (310.5, 39.5), (310.5, 60.5)];
        assert_eq!(lg.select_upload_by_tex_name(0x8), Some(1));
        assert_eq!(lg.select_upload_by_tex_name_containing(0x8, &menu_strip_uvs), None);
        assert_eq!(
            lg.select_upload_by_tex_name_containing(0x8, &menu_strip_uvs)
                .or_else(|| lg.select_upload_for_uv_slice_with_tex_name(0x8, &menu_strip_uvs))
                .or_else(|| lg.select_upload_for_uvs(&menu_strip_uvs)),
            Some(0)
        );
    }

    #[test]
    fn same_tex_name_uv_fallback_beats_unrelated_exact_dimensions() {
        let mut lg = LiveGlState::new(true, false, false, "test".to_string());
        // Unrelated exact 50x50 upload with another tex name (EA logo).
        lg.uploads.push(LiveGlState::build_upload(
            0,
            0x0de1,
            50,
            50,
            0x1908,
            0x8034,
            0x1000_0000,
            &vec![0xff; 50 * 50 * 2],
            Some(0x1b),
        ));
        // Same-name A8 upload that contains the 50x50 UVs (e.g. arrows sheet).
        lg.uploads.push(LiveGlState::build_upload(
            1,
            0x0de1,
            52,
            100,
            0x1906,
            0x1401,
            0x1000_1000,
            &vec![0xff; 52 * 100],
            Some(0x8),
        ));
        // Latest same-name upload cannot contain v=50.5.
        lg.uploads.push(LiveGlState::build_upload(
            2,
            0x0de1,
            1568,
            32,
            0x1906,
            0x1401,
            0x1000_2000,
            &vec![0xff; 1568 * 32],
            Some(0x8),
        ));
        let uvs = [(0.5, 49.5), (0.5, -0.5), (50.5, -0.5), (50.5, 49.5)];
        assert_eq!(lg.select_upload_for_uvs(&uvs), Some(0));
        assert_eq!(
            lg.select_upload_by_tex_name_containing(0x8, &uvs)
                .or_else(|| lg.select_upload_for_uv_slice_with_tex_name(0x8, &uvs))
                .or_else(|| lg.select_upload_for_uvs(&uvs)),
            Some(1)
        );
    }

    #[test]
    fn generated_text_uvs_prefer_matching_font_cell_atlas() {
        let mut lg = LiveGlState::new(true, false, false, "test".to_string());
        lg.uploads.push(LiveGlState::build_upload(
            0,
            0x0de1,
            36,
            20,
            0x1906,
            0x1401,
            0x1000_0000,
            &vec![0xff; 36 * 20],
            None,
        ));
        lg.uploads.push(LiveGlState::build_upload(
            1,
            0x0de1,
            32,
            32,
            0x1906,
            0x1401,
            0x1000_0800,
            &vec![0xff; 32 * 32],
            None,
        ));
        lg.uploads.push(LiveGlState::build_upload(
            2,
            0x0de1,
            784,
            20,
            0x1906,
            0x1401,
            0x1000_1000,
            &vec![0xff; 784 * 20],
            None,
        ));
        lg.uploads.push(LiveGlState::build_upload(
            3,
            0x0de1,
            1568,
            32,
            0x1906,
            0x1401,
            0x1000_2000,
            &vec![0xff; 1568 * 32],
            None,
        ));
        let glyph_16_uvs = [(400.5, 15.5), (400.5, 0.5), (415.5, 0.5), (415.5, 15.5)];
        assert_eq!(
            lg.select_upload_for_generated_text_uvs(&glyph_16_uvs),
            Some(3)
        );
    }

    #[test]
    fn present_applies_configurable_vflip_only_when_enabled() {
        let mut lg = LiveGlState::new(false, false, false, "test".to_string());
        lg.framebuffer[0] = Rgba8::rgba(255, 0, 0, 255);
        lg.framebuffer[FB_WIDTH * (FB_HEIGHT - 1)] = Rgba8::rgba(0, 0, 255, 255);
        let no_flip = lg.present();
        assert_eq!(no_flip[0], Rgba8::rgba(255, 0, 0, 255));
        assert_eq!(
            no_flip[FB_WIDTH * (FB_HEIGHT - 1)],
            Rgba8::rgba(0, 0, 255, 255)
        );

        lg.present_vflip = true;
        let flipped = lg.present();
        assert_eq!(flipped[0], Rgba8::rgba(0, 0, 255, 255));
        assert_eq!(
            flipped[FB_WIDTH * (FB_HEIGHT - 1)],
            Rgba8::rgba(255, 0, 0, 255)
        );
        // internal buffer is never mutated by presentation
        assert_eq!(lg.framebuffer[0], Rgba8::rgba(255, 0, 0, 255));
    }

    #[test]
    fn infer_dims_from_texel_centered_uvs() {
        // 50x50 texture: UVs span 0.5..50.5 in both axes
        let uvs = [(0.5, 0.5), (0.5, -0.5), (50.5, -0.5), (50.5, 49.5)];
        let (w, h) = super::infer_dims_from_uvs(&uvs);
        assert_eq!((w, h), (50, 50));
    }

    #[test]
    fn reset_for_frame_clears_per_frame_state_but_keeps_uploads() {
        let mut lg = LiveGlState::new(true, false, false, "test".to_string());
        lg.uploads.push(LiveGlState::build_upload(
            0,
            0x0de1,
            2,
            2,
            0x1907,
            0x8363,
            0x1000_0000,
            &rgb565_2x2(),
            None,
        ));
        lg.translation = (10.0, 20.0);
        lg.draw_count_in_frame = 2;
        lg.framebuffer[0] = Rgba8::rgba(1, 2, 3, 4);
        lg.reset_for_frame();
        assert_eq!(lg.translation, (0.0, 0.0));
        assert_eq!(lg.draw_count_in_frame, 0);
        assert_eq!(lg.framebuffer[0], Rgba8::rgba(0, 0, 0, 0));
        assert_eq!(lg.uploads.len(), 1, "uploads persist across frames");
    }

    #[test]
    fn quad_group_count_accepts_tight_and_batched_quads() {
        assert_eq!(quad_group_count(DRAW_MODE, 0, 4), Some(1));
        assert_eq!(quad_group_count(DRAW_MODE, 0, 8), Some(2));
        assert_eq!(quad_group_count(DRAW_MODE, 0, 28), Some(7));
    }

    #[test]
    fn quad_group_count_rejects_non_quad_shapes() {
        assert_eq!(quad_group_count(DRAW_MODE, 1, 4), None);
        assert_eq!(quad_group_count(DRAW_MODE, 0, 3), None);
        assert_eq!(quad_group_count(DRAW_MODE, 0, 10), None);
        assert_eq!(quad_group_count(4, 0, 4), None);
    }
}
