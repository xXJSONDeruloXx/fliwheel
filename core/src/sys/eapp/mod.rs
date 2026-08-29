use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use armv4t_emu::{reg, Cpu, Mode as ArmMode};
use chrono::{Datelike, Local, Timelike};
use thiserror::Error;

mod gl_decode;
mod gl_trace;
mod live_gl;
mod rasterizer;
mod usse;
pub use gl_decode::{
    bytes_from_snapshot, decode_fixed_16_16, first_frame, fixed_words_from_snapshot,
    float_words_from_snapshot, format_from_gl, pix_payload_size, register, stack_word,
    texture_upload_candidates, words_from_snapshot, TextureUploadCandidate,
};
use gl_trace::hex_bytes;
pub use gl_trace::{
    GlFileBacking, GlFrameRecord, GlImportRecord, GlMemoryRegion, GlMemorySnapshot,
    GlRegisterSnapshot, GlStackWordSnapshot, GlTraceFixture, GlTraceRecorder, GlValueClass,
};
use live_gl::LiveGlState;
pub use rasterizer::{
    blend_src_over, decode_texture_pixels, framebuffer_hash, framebuffer_to_ppm, rasterize_quad,
    rasterize_triangle, sample_nearest, Rgba8, Texture, TextureFormat,
};
use usse::{UsseProgram, UsseVm};

use crate::devices::generic::Ram;
use crate::devices::{Device, Probe};
use crate::error::*;
use crate::gui::{ButtonCallback, RenderCallback, ScrollCallback, TakeControls};
use crate::memory::{armv4t_adaptor::MemoryAdapter, Memory};

const FILE_VMA_BASE: u32 = 0x1800_0000;
const RECENT_PC_LIMIT: usize = 64;
const BOOTSTRAP_RETURN_PC: u32 = 0x1eff_fffc;
const GUEST_CALLBACK_RETURN_PC: u32 = 0x1eff_fff8;
const WORK_RAM_BASE: u32 = 0x1000_0000;
/// Vortex's input wrapper keeps the physical button state in this guest word.
/// PR #3's direct runner finds the same word at runtime and maps Select to bit
/// 0x01; the generic fliwheel InputEvents packet uses logical field bits and
/// therefore cannot replace this title-local ABI by itself.
const VORTEX_INPUT_FLAGS: u32 = 0x1806_3e5c;
const GL_IDENTITY_MATRIX: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
];
/// Separate guest heap used by the RetailOS-style allocator imports. PR #3's
/// direct runner keeps this arena above the synthetic app RAM, and LOST's
/// internal pool allocator relies on that separation during initialization.
const GAME_HEAP_BASE: u32 = 0x1900_0000;
const GAME_HEAP_SIZE: usize = 64 * 1024 * 1024;
const LEGACY_ENV_PREFIX: &str = "CLICKY_";
const SAVE_DIR: &str = ".fliwheel-saves";
const LEGACY_SAVE_DIR: &str = ".clicky-saves";

/// Read a fliwheel runtime option by its suffix. New callers use the
/// `FLIWHEEL_` namespace; the old `CLICKY_` spelling remains a deliberate,
/// centralized compatibility alias for existing experiments and scripts.
fn fliwheel_var_os(suffix: &str) -> Option<std::ffi::OsString> {
    let current = format!("FLIWHEEL_{}", suffix);
    let legacy = format!("{}{}", LEGACY_ENV_PREFIX, suffix);
    std::env::var_os(current).or_else(|| std::env::var_os(legacy))
}

fn fliwheel_var(suffix: &str) -> Result<String, std::env::VarError> {
    let current = format!("FLIWHEEL_{}", suffix);
    match std::env::var(current) {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => {
            std::env::var(format!("{}{}", LEGACY_ENV_PREFIX, suffix))
        }
        Err(error) => Err(error),
    }
}

/// Multiply two column-major 4x4 matrices using the render-server layout.
fn multiply_column_major(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            out[column * 4 + row] = (0..4)
                .map(|inner| a[inner * 4 + row] * b[column * 4 + inner])
                .sum();
        }
    }
    out
}

/// Build one post-multiplied matrix helper result for the Vortex ABI.
fn matrix_helper_transform(
    ordinal: u32,
    current: [f32; 16],
    args: [u32; 4],
    stack_z: f32,
) -> Option<[f32; 16]> {
    let mut transform = GL_IDENTITY_MATRIX;
    let float_arg = |index: usize| f32::from_bits(args[index]);
    match ordinal {
        169 => {
            transform[12] = float_arg(1);
            transform[13] = float_arg(2);
            transform[14] = float_arg(3);
        }
        171 => {
            transform[0] = float_arg(1);
            transform[5] = float_arg(2);
            transform[10] = float_arg(3);
        }
        173 => {
            let angle = float_arg(1).to_radians();
            let (mut x, mut y, mut z) = (float_arg(2), float_arg(3), stack_z);
            let length = (x * x + y * y + z * z).sqrt();
            if length > 0.0 {
                x /= length;
                y /= length;
                z /= length;
            }
            let (sin, cos) = angle.sin_cos();
            let inverse_cos = 1.0 - cos;
            transform[0] = x * x * inverse_cos + cos;
            transform[1] = y * x * inverse_cos + z * sin;
            transform[2] = x * z * inverse_cos - y * sin;
            transform[4] = x * y * inverse_cos - z * sin;
            transform[5] = y * y * inverse_cos + cos;
            transform[6] = y * z * inverse_cos + x * sin;
            transform[8] = x * z * inverse_cos + y * sin;
            transform[9] = y * z * inverse_cos - x * sin;
            transform[10] = z * z * inverse_cos + cos;
        }
        _ => return None,
    }
    Some(multiply_column_major(&current, &transform))
}

/// Build the column-major orthographic projection used by OpenGLES:167.
fn orthographic_matrix(args: [u32; 4], top: f32, z_near: f32, z_far: f32) -> Option<[f32; 16]> {
    let (left, right, bottom) = (
        f32::from_bits(args[1]),
        f32::from_bits(args[2]),
        f32::from_bits(args[3]),
    );
    if args[0] == 0 || right == left || top == bottom || z_far == z_near {
        return None;
    }
    let mut out = [0.0; 16];
    out[0] = 2.0 / (right - left);
    out[5] = 2.0 / (top - bottom);
    out[10] = -2.0 / (z_far - z_near);
    out[12] = -(right + left) / (right - left);
    out[13] = -(top + bottom) / (top - bottom);
    out[14] = -(z_far + z_near) / (z_far - z_near);
    out[15] = 1.0;
    Some(out)
}

/// Apply the column-major MVP/viewport transform used by the direct Vortex
/// runner. Its non-identity matrices are affine for the observed eApp, but
/// keep the homogeneous divide so this mirrors the oracle's fixed-function
/// vertex path rather than baking in that observation.
fn project_mvp_position(mvp: [f32; 16], x: f32, y: f32, z: f32, w: f32) -> (f32, f32) {
    if mvp == GL_IDENTITY_MATRIX {
        return (x, y);
    }
    let v = [x, y, z, w];
    let mut out = [0.0f32; 4];
    for (row, value) in out.iter_mut().enumerate() {
        *value = (0..4).map(|column| mvp[column * 4 + row] * v[column]).sum();
    }
    let inv_w = if out[3].abs() > 1e-6 { 1.0 / out[3] } else { 1.0 };
    (
        (out[0] * inv_w + 1.0) * 0.5 * live_gl::FB_WIDTH as f32,
        (out[1] * inv_w + 1.0) * 0.5 * live_gl::FB_HEIGHT as f32,
    )
}

// Use a 64 MiB synthetic app RAM window, matching the high-memory 5G-class
// iPods that many clickwheel games targeted. Smaller scratch windows truncate
// guest heaps/arenas: PopCap titles were observed copying assets past both
// 0x1080_0000 (8 MiB) and 0x1200_0000 (32 MiB).
const WORK_RAM_SIZE: usize = 64 * 1024 * 1024;

/// Base address of the synthetic aperture used by the original clicky HLE
/// bring-up (observed in Zuma/Bejeweled at `0x1400000c`). The firmware-side
/// reference model also documents `0x14000000..0x18000000` as an uncached
/// SDRAM alias; `FLIWHEEL_EAPP_UNCACHED_ALIAS=1` selects that A/B path.
const HW_STUB_BASE: u32 = 0x1400_0000;
const HW_STUB_SIZE: usize = 0x400_0000; // 64 MiB - covers entire 0x14000000..0x18000000 gap
                                        // (DMA channel register banks at 64KB strides: 0x14000000, 0x14010000, 0x14020000, etc.)

/// PopCap engine DMA framebuffer base — the engine writes RGB565 pixels
/// directly into this region for display via hardware DMA transfer.
const DMA_FB_BASE: u32 = 0x1402_0000;
const DMA_FB_SIZE: usize = 320 * 240 * 2; // 153,600 bytes for 320×240 RGB565

/// Guest callsite for the shared eapp text-runtime "push one char into a
/// text object" helper. Convention (recovered from disassembly of the Tetris
/// scalar formatter at `0x18008480..0x1800857c`): `r0 = text_obj`,
/// `r1 = char code unit` (ASCII for digits/letters/`:`, UTF-16 for wider
/// glyphs). The helper computes the per-glyph texgen UVs internally, but for
/// titles that share this runtime (Tetris and its sibling engine family) the
/// pushed char is the authoritative per-glyph selector and is only available
/// at this callsite — it is never written into a UTF-16 buffer for the scalar
/// (register-computed) formatter path. Recording it here lets the renderer
/// reconstruct the intended text without hardcoding strings. The constant is a
/// per-binary address, but the convention is shared-runtime; non-Tetris titles
/// simply never hit this PC and are unaffected.
const TEXT_PUSH_CHAR_PC: u32 = 0x1801_616c;
/// Cluster-A clock formatter entry (VMA). After `cmp r3,#0; bxeq lr` guard,
/// so r3 (time-value ptr) is non-zero here. Used by the temporary RE hook.
const TEXT_FORMAT_TIME_ENTRY_PC: u32 = 0x1800_83b4;
/// Env-gated guest-PC probes for the Tetris localization/async string-table
/// path. These are reverse-engineering diagnostics only; default execution is
/// unchanged unless `EAPP_STRING_TRACE=1` is set.
const STRING_TRACE_PCS: &[u32] = &[
    0x1800_3bd0, // boot resource-progress dispatcher over 0x18025674 state
    0x1800_3c08, // state 0/1: allocate/load Strings.dta
    0x1800_3c68, // state 2/3: transition through 0x4228
    0x1800_3c74, // state 4: build wav resource descriptors
    0x1800_3d40, // state 4 tail: register wav descriptor callback
    0x1800_3d60, // state 5: request prefs.sav
    0x1800_3da8, // state 6: request game.sav
    0x1800_3eb8, // audio table consumer: resolve type/index into a handle
    0x1800_4fac, // Strings.dta second-stage callback / progress updater
    0x1800_5400, // state-4 wav descriptor callback/scanner
    0x1800_5468, // state-4 scanner tail-calls 0x15c30 for next wav desc
    0x1800_5480, // state-4 complete: set boot state 5 and re-enter 0x03bd0
    0x1800_5498, // audio resource registration wrapper before Audio:0
    0x1800_7b0c, // scene leaf factory: chooses text slot + string object
    0x1800_7b6c, // scene leaf factory variant with embedded flag
    0x1800_c7a0, // generic scene/list node initializer (binds +0x10/+0x14)
    0x1800_cb84, // scene/list node allocator/constructor variant A
    0x1800_cbf8, // scene/list node allocator/constructor variant B
    0x1800_c938, // generic scene/list node draw recursion entry
    0x1800_9464, // UTF-16 text draw helper entry: r0=text_obj, r3=string object
    0x1800_9514, // UTF-16 text draw helper: about to read string-object pointer/len
    0x1801_62e4, // generic text-object draw wrapper entry before vtable dispatch
    0x1801_6320, // generic text-object draw wrapper bx ip to concrete draw helper
    0x1801_26d8, // string object value-ptr getter: returns [obj+8]
    0x1801_2704, // string object length getter: returns [obj+0xc]
    0x1801_270c, // string object setter: [obj+8]=ptr, [obj+0xc]=len
    0x1801_c940, // options/settings object constructor from vtable 0x18023e00
    0x1801_d76c, // dispatcher: pop head of pending list, link/unlink entry
    0x1801_e0fc, // file-table parse step
    0x1801_e45c, // file-table async completion trampoline
    0x1801_e484, // file-table completion/update body
    0x1801_e708, // resource/menu object activation
    0x1801_eed8, // menu/resource ctor helper: build display/list object
    0x1801_ef1c, // menu/resource state selector: chooses [0x60/0x58/0x5c]
    0x1801_f000, // menu/resource layout builder (uses string object at +0x50)
    0x1801_f068, // menu/resource layout: string length getter callsite
    0x1801_f1b4, // menu/resource dimension update; calls 0x1f000 on change
    0x1801_f250, // menu/resource deserialize/update from PRCT stream
    0x1801_f394, // menu/resource serialize/writeback to PRCT stream
    0x1801_f474, // menu/resource serialize: string length getter callsite
    0x1801_f4a8, // menu/resource runtime refresh from prefs/save globals
    0x1801_f558, // menu/resource runtime state branch on [obj+0xc]
    0x1801_f5a8, // menu/resource reset/default-state helper
    0x1801_f69c, // menu/resource x/offset query helper
    0x1801_f6ec, // menu/resource non-empty string predicate
    0x1801_f72c, // menu/resource activation/render handoff
    0x1801_f794, // menu/resource constructor with six string fields
    0x1801_f900, // menu resource array construction
    0x1801_fa90, // menu resource lookup/dispatch entry
    0x1801_faa8, // menu resource lookup loaded table entry
    0x1801_fb3c, // menu resource update/ensure path
    0x1801_fc68, // AsyncFileIO request completion callback (trampoline)
    0x1801_fc94, // completion status handoff (marks owner done)
    0x1801_d1b4, // audio-stream owner cb: forwards request/status/bytes to second manager
    0x1801_d370, // shared owner cb: store byte_count → ctx+0x120, dispatch processor
    0x1801_d500, // begin-load: assign slot index, set entry[7]=1 lock, call processor
    0x1801_d548, // d500 direct processor path before bx ip
    0x1801_d5bc, // d500 alternate async path: calls 0x1801fd74
    0x1801_d5cc, // d500 range/threshold fail path after async-path skip
    0x1801_d258, // AsyncFileIO:2 secondary callback: may start second owner stage
    0x1801_d68c, // AsyncFileIO:2 secondary callback: may re-enter initiator C or final processor
    0x1801_d424, // AsyncFileIO:2 tertiary callback used by initiator C
    0x1801_d644, // load-manager LINK fn: push entry to pending list
    0x1801_d664, // dead-spin guard if entry[7]!=0 (duplicate reg)
    0x1801_d8d0, // manager init (alloc 10-slot free-list)
    0x1801_d770, // audio-stream manager: pop a pending entry and start its I/O
    0x1801_d800, // audio-stream manager: start the shared owner read
    0x1801_d824, // audio-stream manager: start the secondary owner read
    0x1801_d840, // audio-stream manager: inspect the I/O initiator result
    0x1801_d858, // audio-stream manager: return the selected slot or -1
    0x1801_fd74, // I/O initiator C used by audio-stream manager after d500
    0x1801_fddc, // initiator C success branch: request state becomes 3
    0x1801_fe28, // I/O initiator A (read path, 0x1801d370 shared owner cb)
    0x1801_fcc8, // I/O initiator B (read path, 0x1801d1b4 cb)
    0x1801_fec8, // AFTER AsyncFileIO:3 store [r+4]: capture the in-flight byte
    0x1801_fed8, // 0x1fe28 success-branch return; capture r6 (AsyncFileIO:3 result)
    0x1801_5308, // Strings.dta processor (offset base path)
    0x1801_5c30, // generic descriptor async registrar used by state-4 wav list
    0x1801_5c74, // descriptor registrar return from 0x1dff8 (r0=success)
    0x1801_9770, // texture processor (mirror path, +0xc offset)
    // State machine cases for scene graph selection investigation
    0x1802_22a4, // main per-frame entry function (EAPP header entry pointer)
    0x1800_51bc, // state 0 case: initial boot state
    0x1800_51f0, // state 1 case: legal/loading screen (where we're stuck)
    0x1800_53a8, // state 2 case: legal->menu transition target
    0x1800_533c, // state 3/4/5 case: advances to state 6
    0x1800_535c, // state 6 case: menu steady state
    0x1801_c014, // scene root installer: reads [clock_obj+0x2c], calls vtable[0x44]
    // Name-entry vs main-menu constructor selection
    0x1801_8f40, // name-entry screen constructor entry
    0x1801_8f70, // name-entry: copy decorative/sample text
    0x1801_90ac, // name-entry: calls 0x18007b0c for Enter your name leaf
    0x1801_90cc, // name-entry constructor tail / return
    0x1801_8c18, // shared screen constructor: profile/state branch entry
    0x1801_8cb8, // shared screen constructor: compare profile/state result
    0x1801_c580, // profile/state getter used by the screen constructor
    0x1800_6170, // profile/state object field getter ([object+8])
    0x1801_95a8, // options/settings object constructor (compare with name-entry)
    0x1801_c940, // vtable constructor that builds menu/options objects
    // Scene root pointer and UI object activators
    0x1801_c95c, // post-save object construction entry
    0x1801_c008, // UI update / scene refresh dispatcher
];
// Keep the direct-runner comparison stack aligned with PR #3's 8 MiB RAM
// window. The backing work-RAM remains 64 MiB; only the initial SP is made
// comparable so escaped stack pointers cannot become an oracle difference.
const STACK_TOP: u32 = 0x117f_f000;
const TRAMPOLINE_BASE: u32 = 0x1f00_0000;
const TRAMPOLINE_STRIDE: u32 = 0x20;
const SCREEN_WIDTH: usize = 320;
const SCREEN_HEIGHT: usize = 240;
const SCREEN_PIXELS: usize = SCREEN_WIDTH * SCREEN_HEIGHT;
const IMAGE_RAM_SLACK: usize = 2 * 1024 * 1024;
const EAPP_HEADER_SIZE: usize = 0x28;
const IMPORT_NAME_LEN: usize = 0x20;
const IMPORT_COUNT_OFFSET: usize = 0x30;
const IMPORT_NEXT_OFFSET: usize = 0x34;
const IMPORT_STUBS_OFFSET: usize = 0x38;
const IMPORT_SENTINEL_NAME: &str = "$$$$ a^n + b^n = c^n | n>2 $$$$";
const DEFAULT_FRAMEBUFFER: u32 = 0xff101820;
const HLE_INFO_FRAMEBUFFER: u32 = 0xff203040;
const HLE_WARN_FRAMEBUFFER: u32 = 0xff604020;
const HLE_OPENGL_FRAMEBUFFER: u32 = 0xff205020;
const GL_TEXTURE_2D: u32 = 0x0de1;
const GL_TEXTURE_RECTANGLE: u32 = 0x84f5;
const GL_PALETTE8_RGBA8_OES: u32 = 0x8b96;
const GL_PALETTE8_R5_G6_B5_OES: u32 = 0x8b97;

fn ordinal45_resource_format(format: u32) -> Option<TextureFormat> {
    match format {
        // Observed in Mahjong resource texture objects. These are copied from
        // guest work RAM and decoded as alpha masks with white tint until more
        // exact palette/color state is proven.
        0x8808 | 0x0801 => Some(TextureFormat::A8),
        _ => None,
    }
}

fn palette8_entry_size(format: u32) -> Option<usize> {
    match format {
        GL_PALETTE8_RGBA8_OES => Some(4),
        GL_PALETTE8_R5_G6_B5_OES => Some(2),
        _ => None,
    }
}

fn decode_palette8(raw: &[u8], width: usize, height: usize, format: u32) -> Option<Vec<Rgba8>> {
    let pixel_count = width.checked_mul(height)?;
    let entry_size = palette8_entry_size(format)?;
    let expected = 256usize.checked_mul(entry_size)?.checked_add(pixel_count)?;
    if raw.len() < expected {
        return None;
    }

    let palette = &raw[..entry_size * 256];
    Some(
        (0..pixel_count)
            .map(|pixel| {
                let index = raw[entry_size * 256 + pixel] as usize;
                let base = index * entry_size;
                match format {
                    GL_PALETTE8_RGBA8_OES => Rgba8::rgba(
                        palette[base],
                        palette[base + 1],
                        palette[base + 2],
                        palette[base + 3],
                    ),
                    // The iPod's palette entries are little-endian RGB565, as in
                    // the PR #3 runner. These episode frames are opaque.
                    GL_PALETTE8_R5_G6_B5_OES => {
                        let value = u16::from_le_bytes([palette[base], palette[base + 1]]);
                        let red = (value >> 11) & 0x1f;
                        let green = (value >> 5) & 0x3f;
                        let blue = value & 0x1f;
                        Rgba8::rgba(
                            ((red * 255 + 15) / 31) as u8,
                            ((green * 255 + 31) / 63) as u8,
                            ((blue * 255 + 15) / 31) as u8,
                            255,
                        )
                    }
                    _ => unreachable!("palette format validated above"),
                }
            })
            .collect(),
    )
}

fn is_palette8_compressed_upload_call(args: [u32; 4]) -> bool {
    matches!(args[0], GL_TEXTURE_2D | GL_TEXTURE_RECTANGLE)
        && palette8_entry_size(args[2]).is_some()
}

fn quad_from_slice(pts: &[(f32, f32)]) -> [(f32, f32); 4] {
    debug_assert!(pts.len() >= 4);
    [pts[0], pts[1], pts[2], pts[3]]
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum EappKey {
    Up,
    Down,
    Left,
    Right,
    Action,
    Menu,
}

#[derive(Default)]
pub struct EappBinds {
    pub keys: HashMap<EappKey, ButtonCallback>,
    pub wheel: Option<ScrollCallback>,
}

#[derive(Debug, Default, Clone)]
pub struct EappInputState {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub action: bool,
    pub menu: bool,
    pub wheel_delta: f32,
}

/// A guest-side sound trigger resolved to the resource index used by the
/// shared audio wrapper. The path is populated from the preceding WAV/M4A
/// resource read when the bundle exposes one; a missing path is retained as
/// evidence that the title uses an audio ABI shape we have not mapped yet.
#[derive(Debug, Clone)]
pub struct EappAudioEvent {
    pub frame: u64,
    pub resource_type: u32,
    pub resource_index: u32,
    pub host_path: Option<PathBuf>,
}

pub type EappAudioEventQueue = Arc<Mutex<VecDeque<EappAudioEvent>>>;

#[derive(Debug, Clone)]
pub struct EappMetadata {
    pub title: String,
    pub bundle_dir: PathBuf,
    pub executable_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct EappHeader {
    pub load_addr_guess: u32,
    pub format_version: u32,
    pub header_size: u32,
    pub imports_addr: u32,
    pub entry_addr: u32,
    pub init_addr: u32,
    pub aux_addr: u32,
}

#[derive(Debug, Clone)]
pub struct EappImportModule {
    pub name_addr: u32,
    pub name: String,
    pub count: u32,
    pub next_addr: u32,
    pub stubs_addr: u32,
    pub literals_addr: u32,
}

#[derive(Debug, Clone)]
pub struct EappImage {
    pub metadata: EappMetadata,
    pub header: EappHeader,
    pub imports: Vec<EappImportModule>,
    pub image: Vec<u8>,
}

#[derive(Debug, Clone)]
struct BoundImport {
    module: String,
    ordinal: u32,
}

#[derive(Debug, Clone)]
struct StartupProgressTrace {
    enabled: bool,
    max_logs: usize,
    interval: u64,
    logged: usize,
    last_framebuffer_hash: Option<u64>,
    first_hash_change_frame: Option<u64>,
}

impl StartupProgressTrace {
    fn from_env() -> StartupProgressTrace {
        let enabled = fliwheel_var_os("STARTUP_PROGRESS_TRACE")
            .map(|v| v.to_string_lossy() == "1")
            .unwrap_or(false);
        let max_logs = fliwheel_var_os("STARTUP_PROGRESS_FRAMES")
            .and_then(|v| v.to_string_lossy().parse::<usize>().ok())
            .unwrap_or(180);
        let interval = fliwheel_var_os("STARTUP_PROGRESS_INTERVAL")
            .and_then(|v| v.to_string_lossy().parse::<u64>().ok())
            .unwrap_or(60);
        StartupProgressTrace {
            enabled,
            max_logs,
            interval: interval.max(1),
            logged: 0,
            last_framebuffer_hash: None,
            first_hash_change_frame: None,
        }
    }
}

#[derive(Debug, Clone)]
struct StartupArtifactCapture {
    enabled: bool,
    dir: PathBuf,
    manifest_path: PathBuf,
    periodic_interval: u64,
    max_frames: u64,
    max_dumps: u64,
    dump_start_frame: u64,
    target_frames: Vec<u64>,
    manifest_rows: u64,
    dump_count: u64,
    last_hash: Option<u64>,
}

impl StartupArtifactCapture {
    fn from_env() -> StartupArtifactCapture {
        let enabled = fliwheel_var_os("STARTUP_CAPTURE_DIR").is_some();
        let dir = fliwheel_var_os("STARTUP_CAPTURE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp/fliwheel_startup_capture"));
        let manifest_path = dir.join("manifest.tsv");
        let periodic_interval = fliwheel_var_os("STARTUP_CAPTURE_PERIOD")
            .and_then(|v| v.to_string_lossy().parse::<u64>().ok())
            .unwrap_or(30)
            .max(1);
        let max_frames = fliwheel_var_os("STARTUP_CAPTURE_MAX_FRAMES")
            .and_then(|v| v.to_string_lossy().parse::<u64>().ok())
            .unwrap_or(1200);
        let max_dumps = fliwheel_var_os("STARTUP_CAPTURE_MAX_DUMPS")
            .and_then(|v| v.to_string_lossy().parse::<u64>().ok())
            .unwrap_or(400);
        // Late-game probes can skip the startup animation while retaining the
        // complete manifest. A zero value preserves the historical behavior.
        let dump_start_frame = fliwheel_var_os("STARTUP_CAPTURE_DUMP_START_FRAME")
            .and_then(|v| v.to_string_lossy().parse::<u64>().ok())
            .unwrap_or(0);
        // Explicit checkpoints are useful for oracle comparisons where a
        // title's framebuffer changes every frame. When present, only these
        // frames are dumped instead of consuming the dump budget on every
        // hash change after `dump_start_frame`.
        let mut target_frames = fliwheel_var_os("STARTUP_CAPTURE_TARGET_FRAMES")
            .map(|v| {
                v.to_string_lossy()
                    .split(',')
                    .filter_map(|raw| raw.trim().parse::<u64>().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        target_frames.sort_unstable();
        target_frames.dedup();
        if enabled {
            let _ = fs::create_dir_all(&dir);
            let _ = fs::write(
                &manifest_path,
                "guest_frame\thost_us\tguest_time_current\tguest_time_delta\tdraw_count\thandles\tinternal_hash\tpresented_hash\tdump_reason\tpath\n",
            );
        }
        StartupArtifactCapture {
            enabled,
            dir,
            manifest_path,
            periodic_interval,
            max_frames,
            max_dumps,
            dump_start_frame,
            target_frames,
            manifest_rows: 0,
            dump_count: 0,
            last_hash: None,
        }
    }
}

pub struct Eapp {
    cpu: Cpu,
    bus: EappBus,
    metadata: EappMetadata,
    header: EappHeader,
    imports: Vec<BoundImport>,
    trampoline_to_import: HashMap<u32, usize>,
    logged_imports: HashSet<(String, u32)>,
    recent_pcs: VecDeque<u32>,
    input_state: Arc<Mutex<EappInputState>>,
    /// Previous logical InputEvents event-id mask. The firmware event-list API
    /// reports button transitions (press/release nodes), not a permanently
    /// valid pointer to the current held-state. Keep the prior mask so we can
    /// emit edge nodes and clear the guest list head when no edge occurred.
    input_event_prev_mask: u8,
    /// The Sudoku/Solitaire-family wrapper does not pass its work-RAM owner on
    /// every InputEvents poll. Keep the last reversed-register owner so an
    /// edge produced by a later poll can still be linked, and defer clearing
    /// the consumed node until the following poll.
    input_event_reversed_owner: Option<u32>,
    input_event_reversed_clear_pending: bool,
    /// Absolute clickwheel position before the direct EAPP import ABI maps it
    /// to the guest's normalized 8-bit wheel ring. The host model keeps the
    /// physical 96-detent position so subsequent deltas and wrap behavior
    /// remain faithful to the hardware.
    wheel_position: u8,
    /// LOST's direct-runner oracle uses the RetailOS wheel encoder recovered
    /// in the PR reference: a 120-step raw ring transformed into the low byte
    /// of an event-present packet. Keep this separate from the shared
    /// 96-detent model because the two consumers expose different ABIs.
    lost_wheel_raw: u8,
    lost_wheel_remainder: f32,
    lost_wheel_pending: VecDeque<u8>,
    render_state: Arc<Mutex<Vec<u32>>>,
    controls: Option<EappBinds>,
    next_alloc: u32,
    /// Last texture name returned by the OpenGLES glGenTextures contract.
    /// Zero remains the guest's unbound name; the first generated name is 1.
    next_texture_name: u32,
    /// Sizes of blocks returned by the low work-RAM allocator. Vortex grows
    /// its localization strings through miscTBD:2 (realloc), so the HLE must
    /// retain enough allocator metadata to preserve the old bytes and grow
    /// the common top-of-heap block in place.
    alloc_sizes: HashMap<u32, u32>,
    game_heap_next: u32,
    bootstrap_phase: BootstrapPhase,
    app_object: u32,
    frame_context: u32,
    frame_counter: u64,
    /// Optional diagnostic stop point for bounded guest traces. This is set
    /// only by `FLIWHEEL_EAPP_STOP_FRAME` and leaves normal runs unchanged.
    stop_frame: Option<u64>,
    /// Guest frame on which the scripted relative wheel delta was last
    /// applied. A title may poll InputEvents more than once per frame; the
    /// script models one physical wheel report per frame, like live host
    /// input, rather than multiplying motion by the poll count.
    input_script_wheel_frame: Option<u64>,
    /// Total CPU steps executed (for throttling DMA present checks)
    step_counter: u64,
    pending_guest_calls: VecDeque<PendingGuestCall>,
    /// Number of callbacks that were already due when the current callback
    /// batch began. Callbacks queued while servicing one of those callbacks
    /// belong to the next frame, just like RetailOS's completion queue.
    guest_callback_batch_remaining: usize,
    /// Host file contents staged for delivery to the guest, keyed by the guest
    /// request-object address that asked for them.
    staged_files: HashMap<u32, StagedFile>,
    /// Request objects we've already dumped once, to keep logs tractable.
    dumped_requests: HashSet<u32>,
    /// Per-PC counters for env-gated Tetris localization/string-table tracing.
    string_trace_hits: HashMap<u32, u32>,
    /// Bounded guest-PC probes used for title-specific runtime reverse
    /// engineering. Empty unless FLIWHEEL_EAPP_PC_TRACE is set.
    guest_pc_trace_pcs: HashSet<u32>,
    guest_pc_trace_hits: HashMap<u32, u32>,
    guest_pc_trace_limit: u32,
    /// Per-(module, ordinal) call counters, to find render-critical imports.
    import_call_counts: HashMap<(String, u32), u64>,
    /// Per-frame import counters used by the optional startup-progress trace.
    frame_import_counts: HashMap<(String, u32), u64>,
    startup_progress: StartupProgressTrace,
    startup_capture: StartupArtifactCapture,
    startup_signature_reports: HashSet<String>,
    /// Guest-RAM pointer handles seen at ordinal-159, for one-shot dumping.
    dumped_pointer_handles: HashSet<u32>,
    /// Array pointers already dumped for diagnostic analysis.
    dumped_array_ptrs: HashSet<u32>,
    /// Nested text/font objects discovered from pointer-backed glyph draws.
    dumped_texgen_ptrs: HashSet<u32>,
    /// (handle, reason) pairs for skipped draws, so we only warn once per
    /// unique pair and avoid flooding the headed-run log.
    skipped_draw_warnings: HashSet<(u32, String)>,
    host_start: Instant,
    misc9_time_diag_count: u64,
    misc9_last_pointed_value: Option<u32>,
    /// Optional deterministic increment for the miscTBD:9 clock. The PR #3
    /// runner uses 16,667 microseconds per call in its fixed-clock oracle
    /// runs; keeping this opt-in preserves fliwheel's normal wall-clock mode.
    misc9_fixed_step: Option<u32>,
    misc9_fixed_clock: u32,
    async_request_count: u64,
    async_callback_queued_count: u64,
    guest_callback_invocation_count: u64,
    async_pending_requests: HashSet<u32>,
    /// Synthetic handles returned by direct AsyncFileIO open/read wrappers
    /// (ordinals 12/14/16). Keyed by the small guest-visible handle written
    /// into the caller's file object.
    async_open_files: HashMap<u32, PathBuf>,
    next_async_file_handle: u32,
    /// Synthetic handles returned by the legacy `Filesytem:0` open call.
    /// These are kept separate from direct AsyncFileIO handles because the
    /// two modules use different object layouts and read contracts.
    filesystem_open_files: HashMap<u32, FilesystemFile>,
    next_filesystem_handle: u32,
    /// Synthetic Audio:0 source handles. The guest treats a non-negative
    /// return as an allocated source and later passes the same value to
    /// Audio:23/Audio:1 during teardown. Keep identity state even before the
    /// host PCM sink is wired so the ABI does not collapse every source to 0.
    audio_handles: HashMap<u32, AudioHandle>,
    next_audio_handle: u32,
    /// PopCap titles register each sound source immediately before its async
    /// WAV header read. Keep the most recent source until that read supplies
    /// the host path, then bind the path to the synthetic handle.
    audio_pending_asset_handle: Option<u32>,
    /// PAC-MAN allocates its sixteen sound handles first, then requests the
    /// sixteen WAV headers in the same order. Keep that FIFO separate from
    /// PopCap's interleaved registration/read contract.
    audio_pacman_pending_handles: VecDeque<u32>,
    /// Ms. PAC-MAN allocates a temporary source for each WAV header, releases
    /// it, then allocates the persistent source that gameplay uses. The WAV
    /// read therefore arrives before the handle that must receive the path;
    /// retain the measured source key until that second allocation appears.
    audio_mspacman_pending_asset_paths: VecDeque<(u32, u32, u32, PathBuf)>,
    /// Resource-indexed audio assets retained after the guest releases its
    /// startup table handles. Gameplay uses the resource index again later,
    /// so this shadow catalog is what lets the host sink resolve a sound.
    audio_resource_paths: HashMap<(u32, u32), PathBuf>,
    audio_last_registration: Option<(u32, u32)>,
    audio_events: EappAudioEventQueue,
    /// Directory entries from most recent AsyncFileIO:7 directory enumeration.
    /// Stored so the game can query pack names via subsequent calls.
    async_dir_entries: Vec<String>,
    /// Optional inclusive frame window in which to log every OpenGLES call
    /// with full args + return address, for reverse-engineering the GL stream.
    gl_trace_frames: Option<(u64, u64)>,
    /// Optional bounded OpenGLES capture recorder for machine-readable traces.
    gl_capture: Option<GlTraceRecorder>,
    staged_file_generation: u64,
    halted: bool,
    /// Optional live OpenGLES HLE state. Present only when
    /// `FLIWHEEL_EXPERIMENTAL_GL_HLE=1`; when `None` the legacy fill-color
    /// GL path is used unchanged.
    live_gl: Option<LiveGlState>,
    /// Parsed shader/render-server program from OpenGLES:164 (`rserver.bin`).
    usse_program: Option<UsseProgram>,
    usse_vm: UsseVm,
}

#[derive(Debug, Clone)]
struct StagedFile {
    /// Monotonic host-side generation so overlapping reused buffers can be
    /// attributed to the most recent AsyncFileIO delivery.
    generation: u64,
    /// Guest address where the file payload bytes have been copied.
    payload_addr: u32,
    /// Length in bytes.
    len: u32,
    /// Current guest-visible file position for handle-backed reads.
    offset: usize,
    /// Host path the bytes came from.
    host_path: PathBuf,
}

#[derive(Debug, Clone)]
struct FilesystemFile {
    /// Host path backing a handle returned by the legacy `Filesytem` ABI.
    host_path: PathBuf,
    /// Sequential read position used by `Filesytem:2`.
    offset: usize,
}

#[derive(Debug, Clone)]
struct AudioHandle {
    source_type: u32,
    slot_index: u32,
    table_base: u32,
    host_path: Option<PathBuf>,
}

#[derive(Debug, Copy, Clone)]
struct PendingGuestCall {
    pc: u32,
    arg0: u32,
    arg1: u32,
    arg2: u32,
    arg3: u32,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum BootstrapPhase {
    Entry,
    Init,
    Running,
    Done,
}

#[derive(Debug)]
struct EappBus {
    image: Ram,
    image_len: u32,
    work_ram: Ram,
    game_heap: Ram,
    dma_framebuf: Ram,
    /// The direct HLE runner used by PR #3 returns zero for unmapped guest
    /// reads. Vortex relies on that synthetic-memory behavior during startup;
    /// keep it title-scoped until the same contract is verified for every
    /// decrypted eApp.
    zero_unmapped_reads: bool,
    /// PR #3's direct HLE runner also makes stores to an unmapped address a
    /// no-op. Keep this opt-in for the Vortex A/B because the existing
    /// surface workaround intentionally relies on mapped backing storage.
    zero_unmapped_writes: bool,
    /// Treat the 0x14000000 window as the PP502x uncached SDRAM alias when
    /// explicitly requested. The default synthetic hardware path is retained
    /// until the decrypted-game A/B result is accepted as the new contract.
    uncached_sdram_alias: bool,
    /// Value returned for reads from the synthetic control aperture. The
    /// default preserves the historical HLE value; an explicit override is
    /// useful for bounded contract A/B runs without changing the framebuffer
    /// write path.
    hw_control_value: u32,
    /// Optional one-shot dump of the work-RAM source surrounding the first
    /// DMA framebuffer store. This is diagnostic because the optimized ARM
    /// copy has already advanced r1 by one 16-byte load at the store PC.
    dma_source_dump_dir: Option<PathBuf>,
    /// Optional one-shot dump of the upstream source seen by the first
    /// work-RAM watchpoint hit. This is useful for following the renderer's
    /// buffer-composition copy one stage earlier than the DMA source.
    watch_source_dump_dir: Option<PathBuf>,
    watch_source_dumped: bool,
    /// Track range of DMA FB writes
    hw_fb_write_count: usize,
    hw_fb_write_min: u32,
    hw_fb_write_max: u32,
    /// DMA frame counter: incremented each time pixel 0 is re-written
    hw_dma_frame: usize,
    /// Set to true when DMA FB has been written at least once.
    /// Cleared by the Eapp struct after presenting the DMA content.
    hw_dma_dirty: bool,
    /// Remaining hardware accesses to log when `FLIWHEEL_EAPP_HW_TRACE` is set.
    /// This is intentionally bounded because a framebuffer upload can produce
    /// tens of thousands of accesses before the interesting completion poll.
    hw_trace_remaining: usize,
    /// Optional read-only hardware trace budget. This lets RE runs skip the
    /// large pixel upload and observe the status/read phase that follows it.
    hw_trace_read_remaining: usize,
    /// Include the guest register state that precedes each bounded hardware
    /// trace record. This is opt-in because it makes the bulk pixel trace
    /// substantially larger, but it is useful for identifying the source and
    /// destination of an otherwise opaque memcpy into the aperture.
    hw_trace_regs: bool,
    /// Optional write-watchpoint range (start, end exclusive). When set,
    /// every byte write whose address falls in [start, end) is recorded to
    /// `watch_log` tagged with `pending_pc`. Used for RE: attributing
    /// object-field writes to the guest instruction that performed them.
    watch: Option<(u32, u32)>,
    /// Include register state with bounded watchpoint hits when requested.
    watch_regs: bool,
    /// Remaining reads to log when they overlap the write-watchpoint range.
    /// This is opt-in because a surface-sized range can be read millions of
    /// times during a bounded run.
    watch_read_remaining: usize,
    /// Remaining targeted entries for the shared memcpy implementation. This
    /// is filtered by the watch range so a PopCap surface copy can be seen at
    /// its entry without logging every unrelated asset copy.
    memcpy_trace_remaining: usize,
    /// PC of the instruction currently executing. Captured by `step()` before
    /// `cpu.step(...)` so the bus (which only sees `&mut EappBus`) can tag
    /// watchpoint hits. Stale by one instruction for multi-access instrs, but
    /// close enough to locate the writer.
    pending_pc: u32,
    pending_regs: [u32; 4],
    pending_lr: u32,
    pending_sp: u32,
    previous_pc: u32,
    previous_lr: u32,
    /// Accumulated write-watchpoint hits (addr, value, pc). Drained and
    /// dumped on fatal / at end of run. Bounded; a flooded range is a sign
    /// the watch was set too wide.
    watch_log: Vec<WatchHit>,
    /// Optional inclusive guest-frame window for watchpoint hits. This keeps
    /// startup initialization from consuming the bounded log during RE runs.
    watch_frame_window: Option<(u64, u64)>,
    /// Current guest frame, copied from `Eapp::frame_counter` immediately
    /// before each CPU step so memory accesses can be attributed to a frame.
    watch_frame: u64,
}

#[derive(Debug, Clone, Copy)]
struct WatchHit {
    addr: u32,
    val: u32,
    pc: u32,
    frame: u64,
    regs: Option<WatchRegs>,
}

#[derive(Debug, Clone, Copy)]
struct WatchRegs {
    r0: u32,
    r1: u32,
    r2: u32,
    r3: u32,
    lr: u32,
    sp: u32,
    prev_pc: u32,
    prev_lr: u32,
}

#[derive(Error, Debug)]
pub enum EappBuildError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not find an executable under {0}")]
    MissingExecutable(String),
    #[error("invalid eapp image: {0}")]
    InvalidImage(String),
}

impl Eapp {
    pub fn from_bundle_dir(bundle_dir: impl AsRef<Path>) -> Result<Eapp, EappBuildError> {
        let bundle_dir = bundle_dir.as_ref().to_path_buf();
        let executable_path = find_game_executable(&bundle_dir)?;
        let title = bundle_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| executable_path.display().to_string());
        let metadata = EappMetadata {
            title,
            bundle_dir,
            executable_path,
        };
        let image = EappImage::load(metadata)?;
        Eapp::from_image(image)
    }

    pub fn from_image(image: EappImage) -> Result<Eapp, EappBuildError> {
        let game_id = image.metadata.title.replace(' ', "_").to_ascii_lowercase();
        let zero_unmapped_reads = image.metadata.title == "12345";
        let zero_unmapped_writes = image.metadata.title == "12345"
            && fliwheel_var_os("VORTEX_PR3_GL165").is_some();
        let render_state = Arc::new(Mutex::new(vec![DEFAULT_FRAMEBUFFER; SCREEN_PIXELS]));
        let input_state = Arc::new(Mutex::new(EappInputState::default()));
        let controls = make_controls(Arc::clone(&input_state));

        let mut cpu = Cpu::new();
        cpu.reg_set(ArmMode::User, reg::PC, image.header.entry_addr);
        cpu.reg_set(ArmMode::User, reg::CPSR, 0xd3);
        cpu.reg_set(ArmMode::Supervisor, reg::SP, STACK_TOP);
        cpu.reg_set(ArmMode::User, reg::LR, BOOTSTRAP_RETURN_PC);

        let mut patched_image = image.image.clone();
        let mut imports = Vec::new();
        let mut trampoline_to_import = HashMap::new();
        let mut trampoline_addr = TRAMPOLINE_BASE;

        for module in &image.imports {
            for ordinal in 0..module.count {
                let import_idx = imports.len();
                let literal_addr = module.literals_addr + ordinal * 4;
                let literal_offset = vma_to_offset(literal_addr)? as usize;
                patched_image[literal_offset..literal_offset + 4]
                    .copy_from_slice(&trampoline_addr.to_le_bytes());

                imports.push(BoundImport {
                    module: module.name.clone(),
                    ordinal,
                });
                trampoline_to_import.insert(trampoline_addr, import_idx);
                trampoline_addr = trampoline_addr.wrapping_add(TRAMPOLINE_STRIDE);
            }
        }

        let mapped_image_len = patched_image.len() + IMAGE_RAM_SLACK;
        let mut image_ram = Ram::new(mapped_image_len);
        let image_zeroes = vec![0u8; mapped_image_len];
        image_ram.bulk_write(0, &image_zeroes);
        image_ram.bulk_write(0, &patched_image);

        let mut work_ram = Ram::new(WORK_RAM_SIZE);
        let zeroes = vec![0u8; WORK_RAM_SIZE];
        work_ram.bulk_write(0, &zeroes);

        let mut game_heap = Ram::new(GAME_HEAP_SIZE);
        game_heap.bulk_write(0, &vec![0u8; GAME_HEAP_SIZE]);

        let mut eapp = Eapp {
            cpu,
            bus: EappBus {
                image: image_ram,
                image_len: mapped_image_len as u32,
                work_ram,
                game_heap,
                dma_framebuf: Ram::new(320 * 240 * 2), // RGB565 320×240
                zero_unmapped_reads,
                zero_unmapped_writes,
                uncached_sdram_alias: fliwheel_var_os("EAPP_UNCACHED_ALIAS").is_some(),
                hw_control_value: Self::parse_hw_control_value_env(),
                dma_source_dump_dir: fliwheel_var_os("EAPP_DMA_SOURCE_DUMP_DIR").map(PathBuf::from),
                watch_source_dump_dir: fliwheel_var_os("EAPP_WATCH_SOURCE_DUMP_DIR")
                    .map(PathBuf::from),
                watch_source_dumped: false,
                hw_fb_write_count: 0,
                hw_fb_write_min: u32::MAX,
                hw_fb_write_max: 0,
                hw_dma_frame: 0,
                hw_dma_dirty: false,
                hw_trace_remaining: Self::parse_hw_trace_env(),
                hw_trace_read_remaining: Self::parse_hw_trace_reads_env(),
                hw_trace_regs: fliwheel_var_os("EAPP_HW_TRACE_REGS").is_some(),
                watch: None,
                watch_regs: fliwheel_var_os("EAPP_WATCH_REGS").is_some(),
                watch_read_remaining: Self::parse_watch_reads_env(),
                memcpy_trace_remaining: Self::parse_memcpy_trace_env(),
                pending_pc: 0,
                pending_regs: [0; 4],
                pending_lr: 0,
                pending_sp: 0,
                previous_pc: 0,
                previous_lr: 0,
                watch_log: Vec::new(),
                watch_frame_window: Self::parse_watch_frames_env(),
                watch_frame: 0,
            },
            metadata: image.metadata,
            header: image.header,
            imports,
            trampoline_to_import,
            logged_imports: HashSet::new(),
            recent_pcs: VecDeque::with_capacity(RECENT_PC_LIMIT),
            input_state,
            input_event_prev_mask: 0,
            input_event_reversed_owner: None,
            input_event_reversed_clear_pending: false,
            wheel_position: 0,
            lost_wheel_raw: 0,
            lost_wheel_remainder: 0.0,
            lost_wheel_pending: VecDeque::new(),
            render_state,
            controls: Some(controls),
            next_alloc: WORK_RAM_BASE + 0x1000,
            next_texture_name: 0,
            alloc_sizes: HashMap::new(),
            game_heap_next: GAME_HEAP_BASE,
            bootstrap_phase: BootstrapPhase::Entry,
            app_object: 0,
            frame_context: 0,
            frame_counter: 0,
            stop_frame: Self::parse_stop_frame_env(),
            input_script_wheel_frame: None,
            step_counter: 0,
            pending_guest_calls: VecDeque::new(),
            guest_callback_batch_remaining: 0,
            staged_files: HashMap::new(),
            dumped_requests: HashSet::new(),
            string_trace_hits: HashMap::new(),
            guest_pc_trace_pcs: Self::parse_pc_trace_env(),
            guest_pc_trace_hits: HashMap::new(),
            guest_pc_trace_limit: Self::parse_pc_trace_limit_env(),
            import_call_counts: HashMap::new(),
            frame_import_counts: HashMap::new(),
            startup_progress: StartupProgressTrace::from_env(),
            startup_capture: StartupArtifactCapture::from_env(),
            startup_signature_reports: HashSet::new(),
            dumped_pointer_handles: HashSet::new(),
            dumped_array_ptrs: HashSet::new(),
            dumped_texgen_ptrs: HashSet::new(),
            skipped_draw_warnings: HashSet::new(),
            host_start: Instant::now(),
            misc9_time_diag_count: 0,
            misc9_last_pointed_value: None,
            misc9_fixed_step: Self::parse_fixed_clock_step_env(),
            misc9_fixed_clock: 0,
            async_request_count: 0,
            async_callback_queued_count: 0,
            guest_callback_invocation_count: 0,
            async_pending_requests: HashSet::new(),
            async_open_files: HashMap::new(),
            next_async_file_handle: 1,
            filesystem_open_files: HashMap::new(),
            next_filesystem_handle: 1,
            audio_handles: HashMap::new(),
            next_audio_handle: 1,
            audio_pending_asset_handle: None,
            audio_pacman_pending_handles: VecDeque::new(),
            audio_mspacman_pending_asset_paths: VecDeque::new(),
            audio_resource_paths: HashMap::new(),
            audio_last_registration: None,
            audio_events: Arc::new(Mutex::new(VecDeque::new())),
            async_dir_entries: Vec::new(),
            gl_trace_frames: None,
            gl_capture: None,
            staged_file_generation: 0,
            halted: false,
            live_gl: Self::maybe_init_live_gl(game_id),
            usse_program: None,
            usse_vm: UsseVm::default(),
        };
        let preload_manifest = fliwheel_var_os("GL_PRELOAD_MANIFEST")
            .map(|value| value.to_string_lossy() == "1" || value.to_string_lossy() == "true")
            .unwrap_or(false);
        if preload_manifest {
            let bundle_dir = eapp.metadata.bundle_dir.clone();
            if let Some(lg) = eapp.live_gl.as_mut() {
                let loaded = lg.preload_bundle_textures(&bundle_dir);
                info!(
                    target: "EAPP_GL",
                    "manifest texture preload root={} loaded={}",
                    bundle_dir.display(),
                    loaded.len()
                );
                for entry in loaded {
                    info!(target: "EAPP_GL", "{}", entry);
                }
            }
        }
        if eapp.metadata.title == "12345" {
            // PR #3 calls Vortex's entry vector with the same RetailOS context
            // used by its repeating frame vector. That context is preceded by
            // the 0x90-byte Metadata playlist in the RetailOS heap, so both
            // the pointer values and the guest allocator's first post-context
            // block are part of the observable ABI. fliwheel normally creates
            // its generic context after entry returns in low work RAM; use the
            // direct-HLE layout here instead so address-sensitive game state
            // selection is comparable to the oracle.
            let _playlist = eapp.alloc_game_heap(0x90);
            eapp.app_object = eapp.alloc_game_heap(0x400);
            eapp.frame_context = eapp.app_object.wrapping_add(0x100);
            let _ = eapp.write_guest_bytes(eapp.app_object, &[5]);
            eapp.cpu.reg_set(eapp.cpu.mode(), 0, eapp.app_object);
            eapp.cpu.reg_set(eapp.cpu.mode(), 1, eapp.frame_context);
            eapp.cpu.reg_set(eapp.cpu.mode(), 2, 0);
            eapp.cpu.reg_set(eapp.cpu.mode(), 3, 0);
            info!(
                target: "EAPP",
                "VORTEX: prepared RetailOS context app_object={:#010x} frame_context={:#010x}",
                eapp.app_object,
                eapp.frame_context
            );
        }
        if eapp.metadata.title == "1B200" {
            // The direct runner starts by calling vector 0 with the shared
            // context. fliwheel normally starts execution at entry_addr before
            // it can allocate its host-side context, so prepare that one LOST
            // context up front to avoid executing vector 0 once with null args
            // and then calling it a second time after bootstrap returns.
            // PR #3 first reserves a 0x90-byte Metadata playlist before making
            // this context. Preserve that heap layout so guest allocations and
            // the rserver destination are comparable byte-for-byte.
            let _playlist = eapp.alloc_game_heap(0x90);
            eapp.app_object = eapp.alloc_game_heap(0x400);
            eapp.frame_context = eapp.app_object.wrapping_add(0x100);
            let _ = eapp.write_guest_bytes(eapp.app_object, &[5]);
            eapp.cpu.reg_set(eapp.cpu.mode(), 0, eapp.app_object);
            eapp.cpu.reg_set(eapp.cpu.mode(), 1, eapp.frame_context);
        }
        if eapp.metadata.title == "1B200"
            && fliwheel_var_os("EAPP_LOST_PATCH_RENDER_CALL").is_some()
        {
            // Experimental Lost patch: main loop BL at 0x1803B924 normally
            // targets 0x18007260 (a trivial branch to OpenGLES:13). The full
            // render submission helper begins at 0x18007264 and calls
            // OpenGLES:19. Change EBFF2E4D -> EBFF2E4E to target +4.
            let _ = eapp.write_guest_u32(0x1803_b924, 0xEBFF_2E4E);
            info!(target: "EAPP_GL", "lost_patch_render_call: patched 0x1803b924 BL target 0x18007260 -> 0x18007264");
        }
        eapp.bus.watch = Self::parse_watch_env();
        Ok(eapp)
    }

    pub fn title(&self) -> &str {
        &self.metadata.title
    }

    pub fn metadata(&self) -> &EappMetadata {
        &self.metadata
    }

    /// Return the shared host-side sound-event queue. The emulator thread
    /// owns the guest and appends events; a desktop frontend can drain this
    /// queue without taking ownership of the EAPP runner.
    pub fn audio_event_queue(&self) -> EappAudioEventQueue {
        Arc::clone(&self.audio_events)
    }

    pub fn render_callback(&self) -> RenderCallback {
        let render_state = Arc::clone(&self.render_state);
        Box::new(move |buf: &mut Vec<u32>| -> (usize, usize) {
            let frame = render_state.lock().unwrap();
            buf.splice(.., frame.iter().copied());
            (SCREEN_WIDTH, SCREEN_HEIGHT)
        })
    }

    pub fn run(&mut self) -> FatalMemResult<()> {
        while !self.halted {
            self.step()?;
        }
        Ok(())
    }

    pub fn run_cycles(&mut self, cycles: usize) -> FatalMemResult<()> {
        for _ in 0..cycles {
            if self.halted {
                break;
            }
            self.step()?;
        }
        Ok(())
    }

    /// Drain and log the accumulated write-watchpoint hits, if any. The watch
    /// range is set via `FLIWHEEL_EAPP_WATCH=addr,len`. Unlike the fatal-path
    /// dump, this is safe to call on graceful shutdown / after a bounded run,
    /// which makes the RE tool usable for titles that never fault (e.g.
    /// Tetris). Each hit is tagged with the guest PC that performed the write
    /// so the writer of any watched field can be identified.
    pub fn drain_watch_log(&mut self) {
        // Note: the CLI's `--timeout` may SIGTERM the process before end-of-run
        // drain fires. To ensure watch captures survive regardless of how the
        // run ends, `maybe_log_startup_progress` also calls drain at every
        // emitted startup_progress frame.
        if self.bus.watch_log.is_empty() {
            return;
        }
        let total = self.bus.watch_log.len();
        warn!(
            target: "EAPP",
            "watch drain: {} hits; logging all:",
            total,
        );
        for hit in self.bus.watch_log.drain(..) {
            // Render the value as both hex and a plausible ASCII char, since
            // the most common RE question is "where did the guest store this
            // character?". A u16/char value shows up as a low-byte printable.
            let lo = (hit.val & 0xff) as u8;
            let ascii = if (0x20..=0x7e).contains(&lo) {
                format!(" '{}'", lo as char)
            } else {
                String::new()
            };
            warn!(
                target: "EAPP",
                "  frame={} write addr={:#010x} val={:#010x}{} pc={:#010x}",
                hit.frame, hit.addr, hit.val, ascii, hit.pc,
            );
            if let Some(regs) = hit.regs {
                warn!(
                    target: "EAPP",
                    "    regs r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} lr={:#010x} sp={:#010x} prev_pc={:#010x} prev_lr={:#010x}",
                    regs.r0,
                    regs.r1,
                    regs.r2,
                    regs.r3,
                    regs.lr,
                    regs.sp,
                    regs.prev_pc,
                    regs.prev_lr,
                );
            }
        }
    }

    /// Log DMA framebuffer write stats (bytes written, range, coverage).
    pub fn log_dma_stats(&self) {
        let count = self.bus.hw_fb_write_count;
        let min = self.bus.hw_fb_write_min;
        let max = self.bus.hw_fb_write_max;
        if count == 0 {
            info!(target: "EAPP_HW", "DMA FB: no pixel writes");
            return;
        }
        let range = max - min;
        let coverage = range as usize * 100 / (DMA_FB_SIZE);
        info!(target: "EAPP_HW", "DMA FB: {} w32 writes, {} frames, range {:#010x}..{:#010x} ({} bytes, {}% coverage)", count, self.bus.hw_dma_frame, min, max, range, coverage);
    }

    /// If the DMA framebuffer has been written to (PopCap engine background),
    /// overlay it into the live_gl framebuffer and force-present. The PopCap
    /// game engine renders its background via software rasterization into the
    /// DMA buffer at 0x1402_0000 and does NOT use the GL lifecycle (no
    /// ordinal-158/157 calls). So we must inject a frame present here.
    fn maybe_present_dma_frame(&mut self) {
        // Only present once the DMA buffer is fully written. PopCap games
        // write the entire 320×240 RGB565 buffer in one pass (~38K w32 writes).
        // Presenting a partially-written buffer shows a half-rendered frame.
        let fully_written = self.bus.hw_fb_write_max as usize >= DMA_FB_SIZE - 256;
        if !fully_written {
            return;
        }

        // Read DMA buffer
        let dma_data = {
            let mut buf = vec![0u8; DMA_FB_SIZE];
            self.bus.dma_framebuf.bulk_read(0, &mut buf);
            buf
        };

        // Overlay DMA into live_gl and complete frame
        let completed = if let Some(lg) = self.live_gl.as_mut() {
            lg.begin_frame();
            lg.overlay_dma_rgb565(&dma_data);
            lg.complete_frame()
        } else {
            None
        };

        // Present (separate borrow from live_gl)
        if let Some(completed) = completed {
            if let Some(dir) = fliwheel_var_os("EAPP_DMA_DUMP_DIR") {
                let dir = PathBuf::from(dir);
                let _ = fs::create_dir_all(&dir);
                let path = dir.join(format!("dma_frame_{:06}.rgb565", completed.index));
                if !path.exists() {
                    if let Err(err) = fs::write(&path, &dma_data) {
                        warn!(
                            target: "EAPP_HW",
                            "DMA raw dump failed path={} error={}",
                            path.display(),
                            err
                        );
                    } else {
                        info!(
                            target: "EAPP_HW",
                            "DMA raw dump path={} bytes={}",
                            path.display(),
                            dma_data.len()
                        );
                    }
                }
            }
            self.live_log_completed_frame(&completed, true);
            self.live_log_signature_detail(&completed);
            // DMA-only PopCap presents do not pass through the ordinary
            // candidate-present path, so record them in the same startup
            // manifest as GL-backed frames.
            self.capture_startup_completed_frame(&completed);
            self.live_dump_completed_frame();
            let gate_b = self.live_gl.as_ref().map(|lg| lg.gate_b).unwrap_or(false);
            if gate_b {
                self.live_present_completed_to_window();
            }
        }
        self.bus.hw_dma_dirty = false;
    }

    /// Log the most-frequent import calls seen so far. Useful for finding
    /// render-critical ordinals inside the per-frame loop.
    pub fn log_top_imports(&self, limit: usize) {
        let mut counts: Vec<(&(String, u32), &u64)> = self.import_call_counts.iter().collect();
        counts.sort_by(|a, b| b.1.cmp(a.1));
        let mut rendered = String::new();
        for ((module, ordinal), count) in counts.into_iter().take(limit) {
            rendered.push_str(&format!("\n    {}:{} = {}", module, ordinal, count));
        }
        info!(target: "EAPP", "top {} imports by call count:{}", limit, rendered);
    }

    /// Set an inclusive frame window in which to log every OpenGLES call with
    /// full args + return address. Used for Option A diagnostics.
    pub fn set_gl_trace_window(&mut self, start: u64, end: u64) {
        self.gl_trace_frames = Some((start, end));
    }

    /// Enable bounded JSON-friendly OpenGLES trace capture.
    pub fn enable_gl_capture(
        &mut self,
        start_frame: u64,
        end_frame: u64,
        stack_snapshot_len: usize,
        pointer_snapshot_len: usize,
    ) {
        self.gl_capture = Some(GlTraceRecorder::new(
            start_frame,
            end_frame,
            stack_snapshot_len,
            pointer_snapshot_len,
        ));
    }

    /// Drain the current GL capture into a fixture with metadata filled in.
    pub fn take_gl_trace_fixture(&mut self) -> Option<GlTraceFixture> {
        let recorder = self.gl_capture.take()?;
        let mut fixture = recorder.finalize();
        fixture.title = self.metadata.title.clone();
        fixture.bundle_dir = self.metadata.bundle_dir.display().to_string();
        fixture.executable_path = self.metadata.executable_path.display().to_string();
        fixture.file_vma_base = FILE_VMA_BASE;
        fixture.work_ram_base = WORK_RAM_BASE;
        fixture.work_ram_size = WORK_RAM_SIZE;
        Some(fixture)
    }

    /// Serialize the active GL capture as JSON.
    pub fn write_gl_trace_fixture(&mut self, path: impl AsRef<Path>) -> Result<(), std::io::Error> {
        let fixture = match self.take_gl_trace_fixture() {
            Some(fixture) => fixture,
            None => return Ok(()),
        };
        let json = serde_json::to_vec_pretty(&fixture).map_err(|err| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("serde_json: {}", err))
        })?;
        fs::write(path, json)
    }

    fn capture_open_gl_import(&mut self, ordinal: u32, pc: u32, lr: u32, args: [u32; 4], ret: u32) {
        let Some((start, end)) = self.gl_capture.as_ref().map(|r| r.capture_range()) else {
            return;
        };
        if self.frame_counter < start || self.frame_counter > end {
            return;
        }

        let stack_len = self
            .gl_capture
            .as_ref()
            .map(|r| r.stack_snapshot_len())
            .unwrap_or(0x80);
        let pointer_len = self
            .gl_capture
            .as_ref()
            .map(|r| r.pointer_snapshot_len())
            .unwrap_or(0x80);
        let sp = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let registers = self.capture_registers(pc, lr, sp, args, pointer_len);
        let (stack, stack_bytes) = self.snapshot_memory_with_bytes(sp, stack_len);
        let stack_words = self.capture_stack_words(&stack_bytes, pointer_len);
        let record = GlImportRecord {
            seq: 0,
            seq_in_frame: 0,
            frame: self.frame_counter,
            ordinal,
            pc,
            lr,
            sp,
            return_value: ret,
            stack,
            stack_words,
            registers,
        };

        if let Some(recorder) = self.gl_capture.as_mut() {
            recorder.capture_record(self.frame_counter, record);
        }
    }

    fn capture_registers(
        &mut self,
        pc: u32,
        lr: u32,
        sp: u32,
        args: [u32; 4],
        pointer_len: usize,
    ) -> Vec<GlRegisterSnapshot> {
        let mut registers = Vec::with_capacity(16);
        for idx in 0..13u32 {
            let value = if idx < 4 {
                args[idx as usize]
            } else {
                self.cpu.reg_get(self.cpu.mode(), idx as u8)
            };
            registers.push(self.capture_register(format!("r{}", idx), value, pointer_len, idx < 4));
        }
        registers.push(self.capture_register("sp", sp, pointer_len, true));
        registers.push(self.capture_register("lr", lr, pointer_len, false));
        registers.push(self.capture_register("pc", pc, pointer_len, false));
        registers
    }

    fn capture_register(
        &mut self,
        name: impl Into<String>,
        value: u32,
        pointer_len: usize,
        allow_snapshot: bool,
    ) -> GlRegisterSnapshot {
        let name = name.into();
        let class = self.classify_trace_value(value);
        let float_value = matches!(class, GlValueClass::Float).then(|| f32::from_bits(value));
        let snapshot = if allow_snapshot
            && matches!(
                class,
                GlValueClass::MappedPointer | GlValueClass::CodePointer
            ) {
            Some(self.snapshot_memory(value, pointer_len))
        } else {
            None
        };
        GlRegisterSnapshot {
            name,
            value,
            class,
            float_value,
            snapshot,
        }
    }

    fn capture_stack_words(
        &mut self,
        stack_bytes: &[u8],
        pointer_len: usize,
    ) -> Vec<GlStackWordSnapshot> {
        let mut words = Vec::with_capacity(stack_bytes.len() / 4);
        for (index, chunk) in stack_bytes.chunks_exact(4).enumerate() {
            let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let class = self.classify_trace_value(value);
            let float_value = matches!(class, GlValueClass::Float).then(|| f32::from_bits(value));
            let snapshot = if matches!(
                class,
                GlValueClass::MappedPointer | GlValueClass::CodePointer
            ) {
                Some(self.snapshot_memory(value, pointer_len))
            } else {
                None
            };
            words.push(GlStackWordSnapshot {
                offset: index * 4,
                value,
                class,
                float_value,
                snapshot,
            });
        }
        words
    }

    fn classify_trace_value(&self, value: u32) -> GlValueClass {
        match self.memory_region(value) {
            GlMemoryRegion::WorkRam => GlValueClass::MappedPointer,
            GlMemoryRegion::Image | GlMemoryRegion::Trampoline => GlValueClass::CodePointer,
            GlMemoryRegion::Unmapped => {
                if value & 0x7f80_0000 != 0 {
                    GlValueClass::Float
                } else {
                    GlValueClass::Scalar
                }
            }
        }
    }

    fn memory_region(&self, value: u32) -> GlMemoryRegion {
        let work_end = WORK_RAM_BASE.saturating_add(WORK_RAM_SIZE as u32);
        let image_end = FILE_VMA_BASE.saturating_add(self.bus.image_len);
        if (WORK_RAM_BASE..work_end).contains(&value) {
            GlMemoryRegion::WorkRam
        } else if (FILE_VMA_BASE..image_end).contains(&value) {
            GlMemoryRegion::Image
        } else if (TRAMPOLINE_BASE..TRAMPOLINE_BASE.saturating_add(0x10000)).contains(&value) {
            GlMemoryRegion::Trampoline
        } else {
            GlMemoryRegion::Unmapped
        }
    }

    fn snapshot_memory(&mut self, addr: u32, len: usize) -> GlMemorySnapshot {
        self.snapshot_memory_with_bytes(addr, len).0
    }

    fn snapshot_memory_with_bytes(&mut self, addr: u32, len: usize) -> (GlMemorySnapshot, Vec<u8>) {
        let region = self.memory_region(addr);
        if addr == 0 || len == 0 {
            return (
                GlMemorySnapshot {
                    addr,
                    requested_len: len,
                    len: 0,
                    truncated: false,
                    region,
                    file_backing: None,
                    bytes_hex: String::new(),
                },
                Vec::new(),
            );
        }

        let mut bytes = Vec::with_capacity(len);
        for i in 0..len {
            match self.read_guest_u8(addr.wrapping_add(i as u32)) {
                Some(b) => bytes.push(b),
                None => break,
            }
        }
        let snapshot = GlMemorySnapshot {
            addr,
            requested_len: len,
            len: bytes.len(),
            truncated: bytes.len() < len,
            region,
            file_backing: self.file_backing_for_addr(addr),
            bytes_hex: hex_bytes(&bytes),
        };
        (snapshot, bytes)
    }

    fn file_backing_for_addr(&self, addr: u32) -> Option<GlFileBacking> {
        self.staged_files
            .values()
            .filter(|staged| {
                let end = staged.payload_addr.saturating_add(staged.len);
                (staged.payload_addr..end).contains(&addr)
            })
            .max_by_key(|staged| staged.generation)
            .map(|staged| GlFileBacking {
                path: self.describe_host_path(&staged.host_path),
                base_addr: staged.payload_addr,
                len: staged.len,
                offset: addr.saturating_sub(staged.payload_addr),
            })
    }

    fn read_usse_program_bytes(&mut self, addr: u32, len_hint: u32) -> Option<Vec<u8>> {
        if addr == 0 {
            return None;
        }
        let len = if let Some(backing) = self.file_backing_for_addr(addr) {
            backing.len.saturating_sub(backing.offset) as usize
        } else if len_hint != 0 && len_hint != u32::MAX {
            len_hint as usize
        } else {
            // Lost passes len_hint=0xffffffff for rserver.bin. Cap the blind
            // read to 256 KiB, stopping early on unmapped memory below.
            256 * 1024
        };
        let len = len.min(512 * 1024);
        self.read_guest_bytes(addr, len)
    }

    fn describe_host_path(&self, host_path: &Path) -> String {
        for save_dir in [SAVE_DIR, LEGACY_SAVE_DIR] {
            if let Ok(rel) = host_path.strip_prefix(self.metadata.bundle_dir.join(save_dir)) {
                return format!("{}/{}", save_dir, rel.display());
            }
        }
        if let Ok(rel) = host_path.strip_prefix(&self.metadata.bundle_dir) {
            return rel.display().to_string();
        }
        host_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| host_path.display().to_string())
    }

    /// Env/debug helper: scan guest work RAM for known menu strings in common
    /// encodings. This distinguishes "strings never loaded/parsed" from
    /// "strings exist but no draw path is issuing them" during eapp bring-up.
    pub fn scan_for_strings(&self) {
        let size = WORK_RAM_SIZE;
        let mut buf = vec![0u8; size];
        self.bus.work_ram.bulk_read(0, &mut buf);
        let labels = [
            "MENU", "PLAY", "VOLUME", "OPTIONS", "RECORDS", "HELP", "EXIT",
        ];
        let encodings: [(&str, fn(&str) -> Vec<u8>); 3] = [
            ("ascii", |s| s.as_bytes().to_vec()),
            ("utf16le", |s| {
                s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
            }),
            ("utf16be", |s| {
                s.encode_utf16().flat_map(|c| c.to_be_bytes()).collect()
            }),
        ];
        let find_hits = |pat: &[u8], limit: usize| -> Vec<u32> {
            let mut hits = Vec::new();
            if pat.is_empty() {
                return hits;
            }
            let mut pos = 0usize;
            while pos + pat.len() <= buf.len() {
                let Some(rel) = buf[pos..].windows(pat.len()).position(|w| w == pat) else {
                    break;
                };
                let off = pos + rel;
                hits.push(WORK_RAM_BASE + off as u32);
                pos = off + 1;
                if hits.len() >= limit {
                    break;
                }
            }
            hits
        };
        for label in labels {
            for (enc, make_pat) in encodings {
                let pat = make_pat(label);
                let hits = find_hits(&pat, 8);
                if !hits.is_empty() {
                    info!(
                        target: "EAPP",
                        "string_scan label={} enc={} hits={:?}",
                        label,
                        enc,
                        hits
                    );
                }
            }
        }

        let utf16be =
            |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(|c| c.to_be_bytes()).collect() };
        let decode_utf16be = |bytes: &[u8]| -> String {
            let words: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&words)
        };
        let find_delim = |start: usize, delim: u8| -> Option<usize> {
            let pat = [0u8, delim];
            if start >= buf.len() {
                return None;
            }
            buf[start..]
                .windows(2)
                .position(|w| w == pat)
                .map(|rel| start + rel)
        };
        let find_field_end = |start: usize| -> Option<usize> {
            if start >= buf.len() {
                return None;
            }
            buf[start..]
                .windows(2)
                .position(|w| matches!(w, [0, b'\t'] | [0, b'\n'] | [0, 0]))
                .map(|rel| start + rel)
        };
        let pointer_refs = |start: u32, end: u32, limit: usize| -> Vec<String> {
            let mut refs = Vec::new();
            for off in (0..buf.len().saturating_sub(3)).step_by(4) {
                let val = u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
                if (start..end).contains(&val) {
                    refs.push(format!(
                        "{:#010x}->{:#010x}",
                        WORK_RAM_BASE + off as u32,
                        val
                    ));
                    if refs.len() >= limit {
                        break;
                    }
                }
            }
            refs
        };

        // Parse selected `Strings.dta` rows in-place. The file is UTF-16BE,
        // tab-separated, with column 0 = symbolic ID and column 1 = English.
        // Logging row/value addresses plus pointer references tells us whether
        // the guest built a runtime row/value table, even if no draw path emits
        // the labels yet.
        let ids = [
            "TET_STRING_MAIN_MENU",
            "TET_STRING_PLAY",
            "TET_STRING_VOLUME",
            "TET_STRING_OPTIONS",
            "TET_STRING_RECORDS",
            "TET_STRING_HELP",
            "TET_STRING_EXIT",
        ];
        for id in ids {
            let id_pat = utf16be(id);
            let Some(id_addr) = find_hits(&id_pat, 1).first().copied() else {
                continue;
            };
            let id_off = (id_addr - WORK_RAM_BASE) as usize;
            let Some(tab0) = find_field_end(id_off) else {
                continue;
            };
            let value_start = tab0 + 2;
            let value_end = find_field_end(value_start).unwrap_or(value_start);
            let row_end = find_delim(value_start, b'\n')
                .filter(|&end| end <= value_end)
                .unwrap_or(value_end);
            let value = decode_utf16be(&buf[value_start..value_end]);
            let value_preview = if value.chars().count() > 96 {
                format!("{}…", value.chars().take(96).collect::<String>())
            } else {
                value.clone()
            };
            let value_addr = WORK_RAM_BASE + value_start as u32;
            let row_end_addr = WORK_RAM_BASE + row_end as u32;
            let row_refs = pointer_refs(id_addr, row_end_addr, 16);
            let value_refs = pointer_refs(
                value_addr,
                value_addr + (value_end - value_start) as u32,
                16,
            );
            info!(
                target: "EAPP",
                "string_row id={} id_addr={:#010x} value_addr={:#010x} value={:?} row_end={:#010x} row_ptr_refs=[{}] value_ptr_refs=[{}]",
                id,
                id_addr,
                value_addr,
                value_preview,
                row_end_addr,
                row_refs.join(","),
                value_refs.join(",")
            );
        }
    }

    /// Scan guest work RAM for large contiguous non-zero regions and report
    /// Dump per-PC totals from the optional RE string-path tracer. The
    /// per-PC trace *log* is throttled by `EAPP_STRING_TRACE_LIMIT` to avoid
    /// megabyte logs, but the underlying counters accrue all hits. This prints
    /// those totals so we can see the actual steady-state cycle of the load
    /// manager / dispatcher without being throttled by the log cap.
    pub fn dump_string_trace_totals(&mut self) {
        if string_trace_enabled() && !self.string_trace_hits.is_empty() {
            let mut entries: Vec<(u32, u32)> = self
                .string_trace_hits
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect();
            entries.sort_by_key(|&(pc, _)| pc);
            let mut out = String::from("string_trace_totals:");
            for (pc, count) in entries {
                out.push_str(&format!(" {:#010x}={}", pc, count));
            }
            info!(target: "EAPP_STRING_TRACE", "{}", out);
        }
        // Also dump the write-watchpoint log if any. By default it only gets
        // emitted on a fatal memory fault, which means RE runs that don't
        // fault never see the captured writes. Emitting here at end-of-run
        // lets watches on normally-executing paths (e.g. splash timers in
        // the FILE_VMA region) be inspected.
        self.drain_watch_log();
    }

    /// any whose size is plausible for a framebuffer (e.g. 320*240*2 = 153600
    /// bytes for RGB565, or *4 = 307200 for RGBA8888). Also samples the first
    /// nonzero word of each large region so we can recognise texture data.
    pub fn scan_for_framebuffer(&self) {
        const BLOCK: usize = 256;
        let size = WORK_RAM_SIZE;
        let mut buf = vec![0u8; size];
        self.bus.work_ram.bulk_read(0, &mut buf);

        let is_nonzero = |win: &[u8]| win.iter().any(|&b| b != 0);
        let mut regions: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i < size {
            // find next nonzero 256B block
            if !is_nonzero(&buf[i..i + BLOCK]) {
                i += BLOCK;
                continue;
            }
            let start = i;
            while i < size && is_nonzero(&buf[i..i + BLOCK]) {
                i += BLOCK;
            }
            regions.push((start, i - start));
        }

        // Only report regions >= ~1KB; sort by size desc.
        regions.retain(|&(_, len)| len >= 1024);
        regions.sort_by(|a, b| b.1.cmp(&a.1));

        info!(
            target: "EAPP",
            "work-ram nonzero regions (>=1KB): {} found; top 12 by size:",
            regions.len()
        );
        for &(off, len) in regions.iter().take(12) {
            let addr = WORK_RAM_BASE + off as u32;
            // sample first 4 nonzero words
            let mut sample = String::new();
            let mut taken = 0;
            let mut j = off;
            while j + 4 <= off + len && taken < 4 {
                let w = u32::from_le_bytes([buf[j], buf[j + 1], buf[j + 2], buf[j + 3]]);
                if w != 0 {
                    sample.push_str(&format!(" {:#010x}", w));
                    taken += 1;
                }
                j += 4;
            }
            // framebuffer-size hint
            let fb_hint = match len {
                153600 => " == 320*240*2 (RGB565)",
                307200 => " == 320*240*4 (RGBA8888)",
                76800 => " == 320*240*1 (A8)",
                _ => "",
            };
            info!(
                target: "EAPP",
                "  {:#010x} len={}{} sample:{}",
                addr, len, fb_hint, sample
            );
        }
    }

    pub fn step(&mut self) -> FatalMemResult<()> {
        if self.halted {
            return Ok(());
        }
        // PopCap DMA background present: when the game writes RGB565 pixels
        // to the DMA framebuffer outside the GL frame lifecycle, inject a
        // DMA-only frame present. Throttle to ~every 10K steps to avoid overhead.
        // Only inject if no GL frame is currently active (avoid double-begin).
        self.step_counter += 1;
        if self.bus.hw_dma_dirty
            && self.live_gl.is_some()
            && self.step_counter % 10000 == 0
            && !self
                .live_gl
                .as_ref()
                .map(|lg| lg.frame_active)
                .unwrap_or(false)
        {
            self.maybe_present_dma_frame();
        }
        let pc = self.cpu.reg_get(self.cpu.mode(), reg::PC);
        self.record_pc(pc);
        // Surface the current PC to the bus so write-watchpoint hits can be
        // tagged with the writer's PC. Set before `cpu.step`, which may emit
        // memory writes for the instruction at `pc`.
        self.bus.previous_pc = self.bus.pending_pc;
        self.bus.previous_lr = self.bus.pending_lr;
        self.bus.pending_pc = pc;
        self.bus.watch_frame = self.frame_counter;
        let mode = self.cpu.mode();
        for idx in 0..4 {
            self.bus.pending_regs[idx] = self.cpu.reg_get(mode, idx as u8);
        }
        self.bus.pending_lr = self.cpu.reg_get(mode, reg::LR);
        self.bus.pending_sp = self.cpu.reg_get(mode, reg::SP);
        self.bus.trace_memcpy(pc);
        if pc == BOOTSTRAP_RETURN_PC || (pc == 0 && self.bootstrap_phase != BootstrapPhase::Done) {
            self.handle_bootstrap_return();
            return Ok(());
        }
        if pc == GUEST_CALLBACK_RETURN_PC {
            self.handle_guest_callback_return();
            return Ok(());
        }
        if let Some(&import_idx) = self.trampoline_to_import.get(&pc) {
            self.handle_import(import_idx)?;
            return Ok(());
        }

        // Tetris parsed-resource RE shim for AsyncFileIO:0 wav streaming.
        // Initiator-B (`0x1801fcc8`) calls AsyncFileIO:0, then immediately
        // writes `[request+4]=1` at `0x1801fd4c` to mark the request in-flight.
        // If the completion is only queued for the outer scheduler, the guest
        // can remain inside the current boot/render loop and never return to
        // dispatch it. When the env-gated parsed path is enabled, mark the
        // just-started request complete after that store has executed; the
        // queued owner callback (`0x1801fbfc`) still performs the real callback
        // cascade and byte-count forwarding once control returns.
        if pc == 0x1801_fd50u32
            && fliwheel_var("EAPP_ASYNC3_COMPLETE")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false)
            && self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("66666"))
        {
            let mode = self.cpu.mode();
            let req = self.cpu.reg_get(mode, 4);
            let state = self.read_guest_u32(req.wrapping_add(4)).unwrap_or(0) & 0xff;
            let cb = self.read_guest_u32(req.wrapping_add(0x0c)).unwrap_or(0);
            if state == 1 && cb != 0 {
                let _ = self.write_guest_bytes(req.wrapping_add(4), &[2]);
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:0 post-start complete mark req={:#010x} cb={:#010x}",
                    req,
                    cb
                );
            }
        }

        // Vortex (12345) compatibility shim: its early OpenGLES surface setup
        // calls binary-local block-copy helpers at 0x18014d38/0x18011274 and
        // later GL/state initializers at 0x1800aa40/0x18013eec. The first pair
        // can load near-null block-copy destinations from literal-pool-backed
        // register blocks; the second pair expects `[global+4]` to point at a
        // mutable GL state block before writing fields like +0x24/+0x60/+0x9c.
        // Bootstrap preallocates work-RAM scratch structures for both cases.
        // This is intentionally gated by bundle id and exact PC/range so
        // working titles cannot observe it.
        if self
            .metadata
            .bundle_dir
            .to_str()
            .map_or(false, |p| p.contains("12345"))
        {
            if pc == 0x1801_4d54u32 || pc == 0x1801_1290u32 {
                let mode = self.cpu.mode();
                let current_r0 = self.cpu.reg_get(mode, 0);
                if current_r0 < WORK_RAM_BASE {
                    let surface_buf = self.read_guest_u32(WORK_RAM_BASE + 0xff4).unwrap_or(0);
                    if surface_buf != 0 {
                        self.cpu.reg_set(mode, 0, surface_buf);
                    }
                }
            }
            if pc == 0x1801_8ae8u32 || pc == 0x1801_8aecu32 {
                let mode = self.cpu.mode();
                let current_r4 = self.cpu.reg_get(mode, 4);
                if current_r4 == 0 {
                    let object = self.read_guest_u32(WORK_RAM_BASE + 0xff8).unwrap_or(0);
                    if object != 0 {
                        self.cpu.reg_set(mode, 4, object);
                    }
                }
            }
            if pc == 0x1801_3e00u32 || pc == 0x1801_3e04u32 || pc == 0x1801_3e08u32 {
                let mode = self.cpu.mode();
                let current_r4 = self.cpu.reg_get(mode, 4);
                if current_r4 == 0 {
                    let object = self.read_guest_u32(WORK_RAM_BASE + 0xff8).unwrap_or(0);
                    if object != 0 {
                        self.cpu.reg_set(mode, 4, object);
                    }
                }
            }
            if pc == 0x1801_0f4cu32 && self.gl_hle_enabled() {
                // PR #3 reaches this Vortex request callback with state 10,
                // entering the guest's own audio-manager reset at 0x180111cc.
                // The HLE async completion path otherwise leaves the request
                // in the equivalent 9/12 copy state, which writes zero at
                // +0x104 and suppresses every subsequent SFX trigger. Keep
                // the correction at the measured callback and only for the
                // matching request shape; the guest still performs the reset.
                let req = self.cpu.reg_get(self.cpu.mode(), 0);
                let state = self.read_guest_u8(req.wrapping_add(0x44)).unwrap_or(0);
                let kind = self.read_guest_u32(req.wrapping_add(0x4c)).unwrap_or(0);
                if state == 9 && kind == 0x0c {
                    let _ = self.write_guest_bytes(req.wrapping_add(0x44), &[0x0a]);
                }
            }
            if (0x1800_ab08u32..=0x1800_ab3cu32).contains(&pc) {
                let mode = self.cpu.mode();
                let current_r4 = self.cpu.reg_get(mode, 4);
                let state_block = self.read_guest_u32(WORK_RAM_BASE + 0xffc).unwrap_or(0);
                if current_r4 != 0 && state_block != 0 {
                    let target_slot = current_r4.wrapping_add(4);
                    let current_ptr = self.read_guest_u32(target_slot).unwrap_or(0);
                    if current_ptr < WORK_RAM_BASE {
                        let _ = self.write_guest_u32(target_slot, state_block);
                    }
                    let current_r1 = self.cpu.reg_get(mode, 1);
                    if current_r1 < WORK_RAM_BASE {
                        self.cpu.reg_set(mode, 1, state_block);
                    }
                    if pc == 0x1800_ab38u32 || pc == 0x1800_ab3cu32 {
                        let current_r0 = self.cpu.reg_get(mode, 0);
                        if current_r0 < WORK_RAM_BASE {
                            self.cpu.reg_set(mode, 0, state_block);
                        }
                    }
                }
            }
            if (0x1801_3ef4u32..=0x1801_3f1cu32).contains(&pc) {
                let mode = self.cpu.mode();
                let current_r3 = self.cpu.reg_get(mode, 3);
                let state_block = self.read_guest_u32(WORK_RAM_BASE + 0xffc).unwrap_or(0);
                if current_r3 != 0 && state_block != 0 {
                    let target_slot = current_r3.wrapping_add(4);
                    let current_ptr = self.read_guest_u32(target_slot).unwrap_or(0);
                    if current_ptr < WORK_RAM_BASE {
                        let _ = self.write_guest_u32(target_slot, state_block);
                    }
                    let current_r1 = self.cpu.reg_get(mode, 1);
                    if current_r1 < WORK_RAM_BASE {
                        self.cpu.reg_set(mode, 1, state_block);
                    }
                }
            }
        }

        // Texas Hold'em (33333) RE diagnostic: capture the exact AsyncFileIO:3
        // completion request/owner that reaches the game's callback trampoline
        // at 0x1802fcc4 before the known null-owner fatal at 0x1802fd00.
        // Disabled by default; enable with EAPP_TEXAS_TRACE=1.
        if std::env::var_os("EAPP_TEXAS_TRACE").is_some()
            && self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("33333"))
            && (pc == 0x1802_fcc4u32 || pc == 0x1802_fcf0u32 || pc == 0x1802_fd00u32)
        {
            let mode = self.cpu.mode();
            let r0 = self.cpu.reg_get(mode, 0);
            let r1 = self.cpu.reg_get(mode, 1);
            let r2 = self.cpu.reg_get(mode, 2);
            let r3 = self.cpu.reg_get(mode, 3);
            let owner_from_req = self.read_guest_u32(r0.wrapping_add(8)).unwrap_or(0);
            let status = self.read_guest_u32(r0.wrapping_add(0x20)).unwrap_or(0);
            let byte_count = self.read_guest_u32(r0.wrapping_add(0x24)).unwrap_or(0);
            let cb_pc = self.read_guest_u32(r0.wrapping_add(0x34)).unwrap_or(0);
            let cb_ctx = self.read_guest_u32(r0.wrapping_add(0x38)).unwrap_or(0);
            let owner_state = self.read_guest_u32(r0.wrapping_add(4)).unwrap_or(0);
            let owner_done = self.read_guest_u32(r0.wrapping_add(8)).unwrap_or(0);
            let owner_cb = self.read_guest_u32(r0.wrapping_add(0x0c)).unwrap_or(0);
            let owner_ctx = self.read_guest_u32(r0.wrapping_add(0x10)).unwrap_or(0);
            info!(
                target: "EAPP_TEXAS_TRACE",
                "pc={:#010x} frame={} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} req_owner={:#010x} req_status={:#010x} req_bytes={:#010x} req_cb={:#010x} req_ctx={:#010x} as_owner_state={:#010x} as_owner_done={:#010x} as_owner_cb={:#010x} as_owner_ctx={:#010x}",
                pc,
                self.frame_counter,
                r0,
                r1,
                r2,
                r3,
                owner_from_req,
                status,
                byte_count,
                cb_pc,
                cb_ctx,
                owner_state,
                owner_done,
                owner_cb,
                owner_ctx
            );
        }

        // Capture scalar-formatter char pushes at the shared-runtime text
        // helper callsite. The char (r1) is the authoritative per-glyph
        // selector for the register-computed formatter path (Tetris draws
        // 9-14, `HH:MM AM/PM`), where no UTF-16 buffer is ever written.
        if pc == TEXT_PUSH_CHAR_PC {
            let mode = self.cpu.mode();
            let text_obj = self.cpu.reg_get(mode, 0);
            let char_code = self.cpu.reg_get(mode, 1);
            if let Some(lg) = self.live_gl.as_mut() {
                lg.record_text_char_push(text_obj, char_code);
            }
        }
        // RE (iter 22): cleaner host-event ingress for the Tetris legal→menu
        // gate. The per-frame main function `0x180222a4` builds event flags at
        // `[0x180256d0]` from the app event list, then calls `0x50b0 -> 0x5aa4`
        // to copy those flags into `[0x18025eb0]`. `0x1b630` later copies that
        // value into `slot+0x8c` and gates the legit byte-setter on bits 0x10
        // or 0x08. This env path simulates a host/input event at the upstream
        // mailbox rather than patching the downstream slot immediately before
        // `0x1b630` reads it.
        if pc == 0x1802_22a4u32 {
            if let Ok(raw) = fliwheel_var("EAPP_HOST_EVENT_FLAGS") {
                let parse_u32 = |s: &str| -> Option<u32> {
                    let trimmed = s.trim();
                    if let Some(hex) = trimmed
                        .strip_prefix("0x")
                        .or_else(|| trimmed.strip_prefix("0X"))
                    {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        trimmed.parse::<u32>().ok()
                    }
                };
                let delay = fliwheel_var("EAPP_HOST_EVENT_DELAY")
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                if self.frame_counter >= delay {
                    if let Some(flags) = parse_u32(&raw) {
                        let addr = 0x1802_56d0;
                        let old = self.read_guest_u32(addr).unwrap_or(0);
                        let _ = self.write_guest_u32(addr, old | flags);
                    }
                }
            }
        }

        // Older RE (iter 20): downstream PC-hook injection retained for direct
        // comparison. Prefer `FLIWHEEL_EAPP_HOST_EVENT_FLAGS=0x18` now because it
        // uses the guest's real `0x50b0 -> 0x5aa4` event-copy path.
        if fliwheel_var_os("EAPP_AUDIO_SLOT_BIT").is_some() && pc == 0x1801_b630u32 {
            let bit: u32 = fliwheel_var("EAPP_AUDIO_SLOT_BIT_VAL")
                .ok()
                .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                .unwrap_or(0x10);
            let _ = self.write_guest_u32(0x1802_5eb0, bit);
        }

        // Temporary RE: observe cluster-A clock-formatter entry (VMA
        // 0x180083b4, after the `cmp r3,#0; bxeq lr` guard). r0=text_obj,
        // r3=ptr to time value, *r3 = the time value (signed). Confirms
        // whether the garbage chars (' .) come from a negative *r3.
        if texgen_verbose_enabled() && pc == TEXT_FORMAT_TIME_ENTRY_PC {
            let mode = self.cpu.mode();
            let text_obj = self.cpu.reg_get(mode, 0);
            let time_ptr = self.cpu.reg_get(mode, 3);
            let time_val = self.read_guest_u32(time_ptr).unwrap_or(0) as i32;
            info!(
                target: "EAPP_GL",
                "texgen_time_entry text_obj={:#010x} time_ptr={:#010x} time_val_i32={} time_val_hex={:#010x}",
                text_obj, time_ptr, time_val, time_val as u32
            );
        }

        self.observe_audio_guest_pc(pc);
        self.maybe_trace_string_path(pc);
        self.maybe_trace_guest_pc(pc);

        self.maybe_patch_guest_state(pc);
        if self.handle_guest_svc(pc) {
            return Ok(());
        }

        let mut mem = MemoryAdapter::new(&mut self.bus);
        self.cpu.step(&mut mem);
        if let Some((access, e)) = mem.exception.take() {
            let pc = self.cpu.reg_get(self.cpu.mode(), reg::PC);
            warn!(target: "EAPP", "recent pc trace: {}", self.format_recent_pcs());
            let mode = self.cpu.mode();
            warn!(
                target: "EAPP",
                "fault regs pc={:#010x} fault_addr={:#010x} kind={:?} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} r4={:#010x} r5={:#010x} r6={:#010x} r7={:#010x} r8={:#010x} r9={:#010x} r10={:#010x} r11={:#010x} r12={:#010x} sp={:#010x} lr={:#010x}",
                pc,
                access.offset,
                access.kind,
                self.cpu.reg_get(mode, 0),
                self.cpu.reg_get(mode, 1),
                self.cpu.reg_get(mode, 2),
                self.cpu.reg_get(mode, 3),
                self.cpu.reg_get(mode, 4),
                self.cpu.reg_get(mode, 5),
                self.cpu.reg_get(mode, 6),
                self.cpu.reg_get(mode, 7),
                self.cpu.reg_get(mode, 8),
                self.cpu.reg_get(mode, 9),
                self.cpu.reg_get(mode, 10),
                self.cpu.reg_get(mode, 11),
                self.cpu.reg_get(mode, 12),
                self.cpu.reg_get(mode, reg::SP),
                self.cpu.reg_get(mode, reg::LR),
            );
            // Dump the object whose access faulted (r0 for the common
            // null-deref / null-vtable cases) so the vtable word and
            // surrounding header fields are visible at the fault. This is a
            // generic diagnostic, only fires on fatal, and is intentionally
            // bounded to avoid pulling huge regions into logs.
            if let Some(obj) = self
                .read_guest_words(self.cpu.reg_get(mode, 0), 16)
                .get(0..16)
            {
                warn!(
                    target: "EAPP",
                    "fault object @r0 words=[{}]",
                    obj.iter()
                        .map(|w| format!("{:#010x}", w))
                        .collect::<Vec<_>>()
                        .join(",")
                );
            }
            // For null-deref-via-pointer cases, also dump whatever the
            // faulting object's pointer-fields reference. This surfaces the
            // "next" sibling in intrusive-list cleanup loops (the most
            // common teardown pattern), which carries a valid vtable when the
            // faulting node does not — revealing the expected class pointer.
            let obj_base = self.cpu.reg_get(mode, 0);
            if let Some(obj) = self.read_guest_words(obj_base, 4).get(0..4) {
                for (i, ptr) in obj.iter().copied().enumerate() {
                    if (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&ptr)
                        && ptr != obj_base
                    {
                        if let Some(sib) = self.read_guest_words(ptr, 8).get(0..8) {
                            warn!(
                                target: "EAPP",
                                "fault object sibling @r0[{}]={:#010x} words=[{}]",
                                i,
                                ptr,
                                sib.iter()
                                    .map(|w| format!("{:#010x}", w))
                                    .collect::<Vec<_>>()
                                    .join(",")
                            );
                        }
                    }
                }
            }
            // If r5 looks like a list anchor (the common cleanup-loop pattern
            // where r5 == r6 == list head), dump a few nodes starting from it.
            let anchor = self.cpu.reg_get(mode, 5);
            if (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&anchor)
                && anchor != obj_base
                && anchor == self.cpu.reg_get(mode, 6)
            {
                if let Some(head) = self.read_guest_words(anchor, 8).get(0..8) {
                    warn!(
                        target: "EAPP",
                        "fault list anchor @r5={:#010x} words=[{}]",
                        anchor,
                        head.iter()
                            .map(|w| format!("{:#010x}", w))
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                }
            }
            // Dump the write-watchpoint log (if any) so the constructor / writer
            // of the faulting object's fields can be identified by PC. This
            // is the payoff of the `FLIWHEEL_EAPP_WATCH` RE hook: every write
            // to the watched range is shown with its guest PC, making it
            // possible to ask "who set word 1 (refcount) but never word 0
            // (vtable)?"
            if !self.bus.watch_log.is_empty() {
                warn!(
                    target: "EAPP",
                    "watch hits ({} total; first 64):",
                    self.bus.watch_log.len()
                );
                for hit in self.bus.watch_log.iter().take(64) {
                    warn!(
                        target: "EAPP",
                        "  write addr={:#010x} val={:#010x} pc={:#010x}",
                        hit.addr, hit.val, hit.pc
                    );
                }
            }
            e.resolve(
                "EAPP",
                MemExceptionCtx {
                    pc,
                    access,
                    in_device: format!("eapp, {}", self.bus.probe(access.offset)),
                },
            )?;
        }
        Ok(())
    }

    fn handle_import(&mut self, import_idx: usize) -> FatalMemResult<()> {
        let import = self.imports[import_idx].clone();
        let pc = self.cpu.reg_get(self.cpu.mode(), reg::PC);
        self.record_pc(pc);
        let lr = self.cpu.reg_get(self.cpu.mode(), reg::LR);
        let args = [
            self.cpu.reg_get(self.cpu.mode(), 0),
            self.cpu.reg_get(self.cpu.mode(), 1),
            self.cpu.reg_get(self.cpu.mode(), 2),
            self.cpu.reg_get(self.cpu.mode(), 3),
        ];

        let key = (import.module.clone(), import.ordinal);
        *self.import_call_counts.entry(key.clone()).or_insert(0u64) += 1;
        *self.frame_import_counts.entry(key.clone()).or_insert(0u64) += 1;

        let in_gl_trace = self
            .gl_trace_frames
            .map(|(s, e)| self.frame_counter >= s && self.frame_counter <= e)
            .unwrap_or(false);
        if in_gl_trace && import.module == "OpenGLES" {
            info!(
                target: "EAPP_GL",
                "frame {} GL:{} lr={:#010x} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x}",
                self.frame_counter,
                import.ordinal,
                lr,
                args[0],
                args[1],
                args[2],
                args[3]
            );
        }

        if self.logged_imports.insert(key.clone()) {
            info!(
                target: "EAPP_IMPORT",
                "{}:{} pc={:#010x} lr={:#010x} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x}",
                import.module,
                import.ordinal,
                pc,
                lr,
                args[0],
                args[1],
                args[2],
                args[3]
            );
        } else {
            debug!(
                target: "EAPP_IMPORT",
                "{}:{} pc={:#010x} lr={:#010x}",
                import.module,
                import.ordinal,
                pc,
                lr
            );
        }

        let ret = match import.module.as_str() {
            "OpenGLES" => self.handle_open_gl_import(import.ordinal, args),
            "InputEvents" => self.handle_input_events_import(import.ordinal, args),
            "Settings" => self.handle_settings_import(import.ordinal, args),
            "Metadata" => 0,
            "miscTBD" => self.handle_misc_import(import.ordinal, args),
            "Audio" => self.handle_audio_import(import.ordinal, pc, lr, args),
            "AsyncFileIO" => self.handle_async_file_io_import(import.ordinal, args),
            "Filesytem" => self.handle_filesystem_import(import.ordinal, args),
            other => {
                warn!(target: "EAPP_IMPORT", "unhandled module {}", other);
                self.fill_framebuffer(HLE_WARN_FRAMEBUFFER);
                0
            }
        };

        if import.module == "OpenGLES" {
            self.capture_open_gl_import(import.ordinal, pc, lr, args, ret);
        }

        if import.module == "Audio" && fliwheel_var_os("AUDIO_TRACE").is_some() {
            info!(
                target: "EAPP_AUDIO",
                "AudioRet:{} frame={} lr={:#010x} ret={:#010x}",
                import.ordinal,
                self.frame_counter,
                lr,
                ret
            );
        }

        // Env-gated (`FLIWHEEL_ALLOC_TRACE=1`) allocator-return log. `miscTBD:0`
        // is the runtime allocator and the only path that returns freshly-
        // zeroed guest memory; logging its (caller_lr, returned_addr, len)
        // lets any faulting work-RAM object be attributed to the guest call
        // site that created it. Useful for null-vtable / null-deref teardown
        // investigations.
        if import.module == "miscTBD"
            && import.ordinal == 0
            && fliwheel_var_os("ALLOC_TRACE").is_some()
        {
            info!(
                target: "EAPP_ALLOC",
                "miscTBD:0 alloc lr={:#010x} ret={:#010x} len={} r1={:#010x}",
                lr, ret, args[0], args[1]
            );
        }

        self.cpu.reg_set(self.cpu.mode(), 0, ret);
        self.cpu.reg_set(self.cpu.mode(), reg::PC, lr & !1);
        Ok(())
    }

    fn handle_open_gl_import(&mut self, ordinal: u32, args: [u32; 4]) -> u32 {
        // Decode likely present/swap surface handles for diagnostic purposes.
        // Observed once-per-frame ordinals: 157, 158, 165. The handle in r0
        // (e.g. 0x0003f001) is logged with any guest memory it might point at.
        if matches!(ordinal, 157 | 158 | 165) {
            let handle = args[0];
            info!(
                target: "EAPP_GL",
                "GL:{} surface handle r0={:#010x} (r1={:#010x} r2={:#010x} r3={:#010x})",
                ordinal, handle, args[1], args[2], args[3]
            );
            self.decode_surface_handle(ordinal, handle);
            if self.gl_hle_enabled() {
                if let Some(lg) = self.live_gl.as_mut() {
                    lg.lifecycle_log.push(format!(
                        "frame={} ordinal={} handle={:#010x} (lifecycle role unconfirmed)",
                        self.frame_counter, ordinal, handle
                    ));
                }
            }
        }

        // Experimental live GL HLE path. When enabled, dispatch each observed
        // ordinal into persistent state and a software framebuffer. When
        // disabled, the legacy fill-color diagnostic path is used unchanged.
        if self.gl_hle_enabled() {
            return self.handle_open_gl_hle(ordinal, args);
        }

        self.fill_framebuffer(HLE_OPENGL_FRAMEBUFFER);
        0
    }

    fn gl_hle_enabled(&self) -> bool {
        self.live_gl.is_some()
    }

    /// Parse `FLIWHEEL_EAPP_WATCH=0xADDR,0xLEN` into a work-RAM write-
    /// watchpoint range `(start, end)`. Used by the in-bus watch hook to
    /// attribute field writes to the guest instruction that performed them.
    /// Returns `None` when unset, so default behavior is unchanged.
    fn parse_watch_env() -> Option<(u32, u32)> {
        let raw = fliwheel_var("EAPP_WATCH").ok()?;
        let mut parts = raw.split(',');
        let addr_str = parts.next()?.trim();
        let len_str = parts.next().map(|s| s.trim()).unwrap_or("0x20");
        let parse_num = |s: &str| -> Option<u32> {
            let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
            u32::from_str_radix(s, 16)
                .ok()
                .or_else(|| s.parse::<u32>().ok())
        };
        let addr = parse_num(addr_str)?;
        let len = parse_num(len_str).unwrap_or(0x20);
        Some((addr, addr.wrapping_add(len)))
    }

    /// Parse `FLIWHEEL_EAPP_WATCH_FRAMES=start-end` into an inclusive guest
    /// frame window. A single frame number is also accepted. The range is
    /// intentionally separate from `FLIWHEEL_EAPP_WATCH` so an existing
    /// address watch keeps its historical all-frame behavior when unset.
    fn parse_watch_frames_env() -> Option<(u64, u64)> {
        fliwheel_var("EAPP_WATCH_FRAMES")
            .ok()
            .and_then(|raw| Self::parse_watch_frame_range(&raw))
    }

    fn parse_watch_frame_range(raw: &str) -> Option<(u64, u64)> {
        let mut parts = raw.trim().splitn(2, '-');
        let start = parts.next()?.trim().parse::<u64>().ok()?;
        let end = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(start);
        (start <= end).then_some((start, end))
    }

    fn parse_stop_frame_env() -> Option<u64> {
        fliwheel_var("EAPP_STOP_FRAME")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|frame| *frame > 0)
    }

    /// Parse `FLIWHEEL_EAPP_FIXED_CLOCK`. A boolean value selects the
    /// 16,667-microsecond increment used by the PR #3 direct-runner oracle;
    /// a positive decimal or hexadecimal value selects another increment.
    /// The option is deliberately opt-in so ordinary runs retain wall time.
    fn parse_fixed_clock_step_env() -> Option<u32> {
        let raw = fliwheel_var_os("EAPP_FIXED_CLOCK")?;
        let value = raw.to_string_lossy();
        if value == "1" || value.eq_ignore_ascii_case("true") {
            return Some(16_667);
        }
        let trimmed = value.trim();
        let number = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .or_else(|| trimmed.parse::<u32>().ok());
        number.filter(|step| *step > 0)
    }

    /// Parse a comma-separated list of guest PCs for the bounded diagnostic
    /// trace. This is intentionally opt-in so normal execution has no extra
    /// per-instruction parsing or logging overhead.
    fn parse_pc_trace_env() -> HashSet<u32> {
        let parse_num = |s: &str| -> Option<u32> {
            let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
            u32::from_str_radix(s, 16)
                .ok()
                .or_else(|| s.parse::<u32>().ok())
        };
        fliwheel_var("EAPP_PC_TRACE")
            .ok()
            .into_iter()
            .flat_map(|raw| raw.split(',').filter_map(parse_num).collect::<Vec<_>>())
            .collect()
    }

    fn parse_pc_trace_limit_env() -> u32 {
        fliwheel_var("EAPP_PC_TRACE_LIMIT")
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .unwrap_or(24)
            .max(1)
    }

    /// Parse `FLIWHEEL_EAPP_HW_TRACE`. `1` enables a bounded trace with the
    /// default limit; a decimal value selects an explicit access limit.
    fn parse_hw_trace_env() -> usize {
        match fliwheel_var("EAPP_HW_TRACE") {
            Ok(value) if value.trim() == "1" => 256,
            Ok(value) => value.trim().parse::<usize>().unwrap_or(0),
            Err(_) => 0,
        }
    }

    fn parse_hw_trace_reads_env() -> usize {
        fliwheel_var("EAPP_HW_TRACE_READS")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0)
    }

    fn parse_hw_control_value_env() -> u32 {
        fliwheel_var("EAPP_HW_CONTROL_VALUE")
            .ok()
            .and_then(|value| {
                let value = value.trim();
                value
                    .strip_prefix("0x")
                    .or_else(|| value.strip_prefix("0X"))
                    .map(|hex| u32::from_str_radix(hex, 16).ok())
                    .unwrap_or_else(|| value.parse::<u32>().ok())
            })
            .unwrap_or(1)
    }

    fn parse_memcpy_trace_env() -> usize {
        match fliwheel_var("EAPP_MEMCPY_TRACE") {
            Ok(value) if value.trim() == "1" => 256,
            Ok(value) => value.trim().parse::<usize>().unwrap_or(0),
            Err(_) => 0,
        }
    }

    fn parse_watch_reads_env() -> usize {
        match fliwheel_var("EAPP_WATCH_READS") {
            Ok(value) if value.trim() == "1" => 256,
            Ok(value) => value.trim().parse::<usize>().unwrap_or(0),
            Err(_) => 0,
        }
    }

    /// Read the experimental GL HLE env flags and construct live state only
    /// when `FLIWHEEL_EXPERIMENTAL_GL_HLE=1`. Returns `None` (legacy path) when
    /// the flag is absent or not enabled, so default behavior is unchanged.
    fn maybe_init_live_gl(game_id: String) -> Option<LiveGlState> {
        let enabled = fliwheel_var_os("EXPERIMENTAL_GL_HLE")
            .map(|v| v.to_string_lossy() == "1")
            .unwrap_or(false);
        if !enabled {
            return None;
        }
        let present_vflip = fliwheel_var_os("GL_PRESENT_VFLIP")
            .and_then(|v| v.to_string_lossy().parse::<u32>().ok())
            .map(|n| n != 0)
            .unwrap_or_else(|| live_gl_default_present_vflip(&game_id));
        let gate_b = fliwheel_var_os("GL_GATE_B")
            .map(|v| v.to_string_lossy() == "1")
            .unwrap_or(false);
        let continuous = fliwheel_var_os("GL_LIVE_CONTINUOUS")
            .map(|v| v.to_string_lossy() == "1")
            .unwrap_or(false);
        let dump_frames = fliwheel_var_os("GL_DUMP_FRAMES")
            .and_then(|v| v.to_string_lossy().parse::<usize>().ok())
            .unwrap_or(0);
        info!(
            target: "EAPP_GL",
            "experimental GL HLE enabled: present_vflip={} gate_b={} continuous={} dump_frames={}",
            present_vflip, gate_b, continuous, dump_frames
        );
        let mut lg = LiveGlState::new(present_vflip, gate_b, continuous, game_id);
        lg.dump_remaining = dump_frames;
        Some(lg)
    }

    /// Experimental live GL HLE dispatch. Called for every OpenGLES import
    /// when the flag is enabled. Records state for the observed ordinals and
    /// drives the software framebuffer via `LiveGlState`.
    fn handle_open_gl_hle(&mut self, ordinal: u32, args: [u32; 4]) -> u32 {
        let frame = self.frame_counter;
        let boundary = matches!(self.live_gl.as_ref(), Some(lg) if frame != lg.last_frame_counter);
        if boundary {
            // On the guest frame boundary, emit the previous frame's lifecycle
            // trace (evidence for begin/present detection) before resetting.
            if let Some(lg) = self.live_gl.as_mut() {
                let prev_frame = lg.last_frame_counter;
                let draws = lg.draw_count_in_frame;
                lg.previous_frame_draw_count = draws;
                if let Some(summary) = lg.take_frame_trace_summary(prev_frame, draws) {
                    info!(target: "EAPP_GL", "{}", summary);
                    if lg.lifecycle_reports.len() < lg.lifecycle_report_budget {
                        lg.lifecycle_reports.push(summary);
                    }
                }
                lg.last_frame_counter = frame;
                if texgen_verbose_enabled() {
                    for line in lg.take_text_char_diag(prev_frame) {
                        info!(target: "EAPP_GL", "{}", line);
                    }
                }
                lg.reset_for_frame();
            }
        }

        // Record this call in the current frame's lifecycle trace.
        let trace_handle = if matches!(ordinal, 157 | 158 | 165 | 159) {
            args[0]
        } else {
            0
        };
        if let Some(lg) = self.live_gl.as_mut() {
            lg.ordinal_trace.push((ordinal, trace_handle));
        }

        let ret = match ordinal {
            13 => {
                self.live_handle_clear_color(args);
                0
            }
            12 => {
                self.live_handle_clear(args);
                0
            }
            99 => {
                self.live_handle_upload(args);
                0
            }
            21 => {
                self.live_handle_copy_texture(args);
                0
            }
            105 => {
                self.live_handle_sub_upload(args);
                0
            }
            137 => {
                self.live_handle_array_def(args);
                0
            }
            40 => {
                self.live_handle_enable_array(args);
                0
            }
            36 => {
                self.live_handle_disable_array(args);
                0
            }
            169 => {
                // Vortex's #169 is the mat4 translate helper, not the
                // ordinary screen-space translation ABI used by the other
                // live-HLE title families. The matrix write below is the
                // only state change its direct oracle observes.
                if self.metadata.title != "12345"
                    || fliwheel_var_os("VORTEX_PR3_GL165").is_none()
                {
                    self.live_handle_translate(args);
                }
                self.live_handle_vortex_matrix_op(169, args);
                0
            }
            171 | 173 | 175 => {
                self.live_handle_vortex_matrix_op(ordinal, args);
                0
            }
            // OpenGLES:147 is the scalar fixed-point form of the colour
            // register update. LOST uses it for the translucent black bars
            // over the episode background; 148/120 are the vector fixed and
            // float forms used by other clickwheel titles.
            147 => {
                self.live_handle_uniform4x_scalar(args);
                0
            }
            148 => {
                self.live_handle_uniform4x(args, true);
                0
            }
            120 => {
                self.live_handle_uniform4x(args, false);
                0
            }
            159 => {
                self.live_handle_bind_material(args);
                // LOST checks the pipeline-select result before entering its
                // ordinary vertex-array submission path. PR #3's direct HLE
                // runner returns success here; leaving the generic zero
                // result makes the title stop after its clear/present pair.
                u32::from(matches!(self.metadata.title.as_str(), "1B200" | "12345"))
            }
            37 => {
                self.live_handle_draw(args);
                0
            }
            38 => {
                self.live_handle_draw_elements(args);
                0
            }
            45 => {
                self.live_handle_gen_textures(args);
                self.live_handle_resource_upload(args);
                0
            }
            // Candidate lifecycle from observed live ordering:
            // 158 always precedes all steady-state draws; 157 always follows.
            // Neutral names until exact ABI semantics are proven.
            158 => {
                self.live_handle_candidate_begin();
                if self.metadata.title == "12345"
                    && fliwheel_var_os("VORTEX_PR3_GL165").is_some()
                {
                    0x3000
                } else {
                    0
                }
            }
            157 => {
                self.live_handle_candidate_present();
                0
            }
            165 => {
                self.live_handle_ordinal_165(args);
                0
            }
            // Ordinal 164: shader program create/link. Takes pointer to shader
            // binary (rserver.bin), returns a program ID. Must return non-zero
            // so the game sees a valid program. Lost and TWA both use this.
            164 => {
                // Return a pseudo-handle so the game thinks the program compiled.
                // The actual shader data at r1 is ignored (no real GPU here).
                // A non-zero return is critical: Lost checks the program handle
                // and won't draw if it's 0 (meaning compilation failed).
                info!(target: "EAPP_GL", "ordinal_164: shader_create addr={:#010x} len_hint={:#010x}", args[1], args[2]);
                // Parse/cache the loaded shader/render-server program. For Lost,
                // args[1] points at rserver.bin loaded via AsyncFileIO:3.
                if let Some(program_bytes) = self.read_usse_program_bytes(args[1], args[2]) {
                    let program = UsseProgram::parse(args[1], &program_bytes);
                    info!(target: "EAPP_GL", "ordinal_164: parsed_usse {}", program.summary());
                    self.usse_vm = UsseVm::default();
                    self.usse_program = Some(program);
                } else {
                    info!(target: "EAPP_GL", "ordinal_164: unable to read shader bytes at {:#010x}", args[1]);
                }
                // Scan the rserver header after the game calls ordinal-164 to see
                // if the iPod's GL driver has written anything there. For Lost,
                // rserver.bin is loaded at 0x10001038, and the 0x200-byte header
                // is at 0x10001038..0x10001237.
                let bin_addr = args[1];
                if bin_addr >= 0x10000000 && bin_addr < 0x10800000 {
                    let header_start = bin_addr;
                    let header_words = self.read_guest_words(header_start, 0x80); // First 128 words (0x200 bytes)
                    let non_zero: Vec<(String, u32)> = header_words
                        .iter()
                        .enumerate()
                        .filter(|(_, &w)| w != 0)
                        .map(|(i, w)| (format!("+0x{:03x}", i * 4), *w))
                        .collect();
                    if !non_zero.is_empty() {
                        let summary: Vec<String> = non_zero
                            .iter()
                            .map(|(off, val)| format!("{}={:#010x}", off, val))
                            .collect();
                        info!(target: "EAPP_GL", "ordinal_164: rserver header non-zero: {}", summary.join(", "));
                    } else {
                        info!(target: "EAPP_GL", "ordinal_164: rserver header all zeros (0x200 bytes)");
                    }
                    // Approach: Fill the rserver header with pointers to Thumb stubs
                    // that return 0 or 1. On a real iPod, ordinal 164 would parse
                    // the rserver binary and write function pointers to the header
                    // (a dispatch table). Without valid pointers, the game code
                    // finds null function entries and skips rendering entirely.
                    // We create Thumb stubs (mov r0,#1; bx lr) in work RAM and
                    // fill the header with pointers (bit 0 set for Thumb mode).
                    // Behind FLIWHEEL_EAPP_THUMB_STUBS=1 env var.
                    if fliwheel_var_os("EAPP_THUMB_STUBS").is_some() {
                        // Allocate 16 bytes for Thumb stubs (2 instructions each = 4 bytes per stub)
                        let stub_base = self.alloc_zeroed(16);
                        // Thumb LE packed as 32-bit words:
                        // stub0 (return 0): mov r0,#0 (0x2000); bx lr (0x4770) = 0x47702000
                        // stub1 (return 1): mov r0,#1 (0x2001); bx lr (0x4770) = 0x47702001
                        self.write_guest_u32(stub_base, 0x47702000); // stub0: return 0
                        self.write_guest_u32(stub_base.wrapping_add(4), 0x47702001); // stub1: return 1
                        let stub0 = stub_base | 1; // Thumb mode bit
                        let stub1 = (stub_base.wrapping_add(4)) | 1; // Thumb mode bit
                                                                     // Fill header with stub1 pointers (return 1 = success)
                        for i in 0..0x80 {
                            self.write_guest_u32(header_start.wrapping_add(i * 4), stub1);
                        }
                        info!(target: "EAPP_GL", "ordinal_164: filled rserver header with Thumb stubs (stub0={:#010x}, stub1={:#010x})", stub0, stub1);
                    }
                    // Nuclear option: fill the rserver header with incrementing values
                    // to see if the game's rendering engine uses any of them.
                    // Behind FLIWHEEL_EAPP_FILL_RSERVER_HEADER=1 env var.
                    if fliwheel_var_os("EAPP_FILL_RSERVER_HEADER").is_some() {
                        for i in 0..0x80 {
                            let val = (i + 1) as u32; // 1, 2, 3, ...
                            self.write_guest_u32(header_start.wrapping_add(i * 4), val);
                        }
                        info!(target: "EAPP_GL", "ordinal_164: filled rserver header with incrementing values (1..128)");
                    }
                }
                1u32 // First program handle = 1
            }
            // Ordinal 167: shader program use/bind. Lost doesn't call this but TWA does.
            167 => {
                if self.metadata.title == "12345"
                    && fliwheel_var_os("VORTEX_PR3_GL165").is_some()
                {
                    self.live_handle_vortex_ortho(args);
                } else {
                    info!(target: "EAPP_GL", "ordinal_167: shader_bind program={:#010x}", args[0]);
                }
                0
            }
            // Ordinal 152: glGetProgramiv or similar query. Lost calls this after 164.
            // r0=query_type r1=buf_ptr r2=size_ptr. May write GL_LINK_STATUS etc.
            // Return success by writing 1 (GL_TRUE) at the buffer pointer.
            // Also write size (4) to size_ptr if provided, and return the
            // program handle as R0 so the game can use it.
            152 => {
                if args[1] != 0 {
                    let _ = self.write_guest_u32(args[1], 1); // GL_TRUE = link success
                }
                if args[2] != 0 {
                    // PR #3's direct LOST runner measures this ordinal as the
                    // render-server start contract: its two output words are
                    // 1 and 2, not the generic program-query size of 4.
                    let value = if self.metadata.title == "1B200" { 2 } else { 4 };
                    let _ = self.write_guest_u32(args[2], value);
                }
                info!(target: "EAPP_GL", "ordinal_152: program_query r0={} buf={:#010x} size_ptr={:#010x}", args[0], args[1], args[2]);
                1u32 // return program handle 1
            }
            // Ordinal 153: glViewport-like. Some games call this during init.
            153 => {
                // glViewport(x, y, w, h) — many games pass 0 dimensions
                // during init or when querying the shader. Lost passes h=0
                // which causes divide-by-zero in the render server. Fix:
                // if width or height is 0, fill with default 320×240.
                let (x, y, w, h) = (args[0], args[1], args[2], args[3]);
                if w == 0 || h == 0 || w == 0xFFFFFFFF || h == 0xFFFFFFFF {
                    info!(target: "EAPP_GL", "ordinal_153: viewport fixup: ({},{},{},{}) -> (0,0,320,240)", x, y, w, h);
                    if let Some(lg) = self.live_gl.as_mut() {
                        lg.viewport_w = 320;
                        lg.viewport_h = 240;
                    }
                } else {
                    if let Some(lg) = self.live_gl.as_mut() {
                        lg.viewport_w = w;
                        lg.viewport_h = h;
                    }
                }
                info!(target: "EAPP_GL", "ordinal_153: viewport x={} y={} w={} h={}", x, y, w, h);
                // LOST branches on a successful render-server lifecycle
                // result. The reference runner returns 1 for this title;
                // retain the generic viewport result for every other bundle.
                u32::from(self.metadata.title == "1B200")
            }
            // Ordinal 19 is glCompressedTexImage2D for the clickwheel titles
            // that use paletted artwork. Sims uses the rectangle-texture
            // target (0x84f5) rather than GL_TEXTURE_2D. LOST's episode
            // frames use the RGB565 palette variant (0x8b97), while the
            // earlier artwork uses RGBA8 (0x8b96). Keep the old render-server
            // diagnostic fallback for non-texture calls until that alternate
            // ABI is proven.
            19 => {
                if is_palette8_compressed_upload_call(args) {
                    self.live_handle_compressed_upload(args);
                } else {
                    if let Some(program) = self.usse_program.as_ref() {
                        program.step_placeholder(&mut self.usse_vm, 64);
                        info!(target: "EAPP_GL", "ordinal_19: render_dispatch r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} usse_pc={} executed={} r0_raw={:#010x} halted={}",
                            args[0], args[1], args[2], args[3], self.usse_vm.pc_word, self.usse_vm.executed_words, self.usse_vm.scalar_regs[0], self.usse_vm.halted);
                    } else {
                        info!(target: "EAPP_GL", "ordinal_19: render_dispatch r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} usse=<none>", args[0], args[1], args[2], args[3]);
                    }
                }
                0
            }
            // Draw-adjacent state ordinal; retained as a no-op until a title
            // proves a fixed-function matrix contract for it.
            149 => 0,
            // OpenGLES:125 is glUniformMatrix4fv in the direct Vortex path.
            // The oracle uploads the current model/projection matrix through
            // r3 before each model-space draw. Keep this title-scoped while
            // the other families continue using their established screen
            // coordinate paths.
            125 => {
                if self.metadata.title == "12345"
                    && fliwheel_var_os("VORTEX_PR3_GL165").is_some()
                {
                    self.live_handle_vortex_mvp(args);
                }
                0
            }
            // Ordinal 4: glBindTexture(target, texture).
            // r0=target (e.g. 0xDE1=GL_TEXTURE_2D), r1=texture_name.
            // Capture the texture name so that the next ordinal-99 upload
            // can be associated with it. A real bind is the latest candidate
            // even when an older ordinal-45 descriptor left a pending name;
            // PopCap loads several textures through this bind-before-upload
            // sequence and reuses the same material handles across them.
            4 => {
                let tex_name = args[1];
                if let Some(lg) = self.live_gl.as_mut() {
                    // Keep the bind live after the upload. `pending_tex_name`
                    // is consumed by the next TexImage2D, while this state
                    // identifies the texture sampled by the next draw.
                    lg.bound_tex_name = (tex_name != 0).then_some(tex_name);
                }
                if tex_name != 0 {
                    if let Some(lg) = self.live_gl.as_mut() {
                        lg.pending_tex_name = Some(tex_name);
                    }
                }
                0
            }
            _ => 0,
        };
        ret
    }

    /// Candidate begin from observed live ordering: ordinal 158 is the first
    /// surface ordinal and always precedes steady-state draws. Neutral name;
    /// exact ABI semantics remain unproven.
    fn live_handle_candidate_begin(&mut self) {
        let continuous = self
            .live_gl
            .as_ref()
            .map(|lg| lg.continuous_capture)
            .unwrap_or(false);
        if !continuous {
            return; // one-shot diagnostic capture keeps its existing heuristic
        }
        if let Some(lg) = self.live_gl.as_mut() {
            let outcome = lg.begin_frame();
            if matches!(outcome, live_gl::BeginOutcome::DoubleBegin) {
                warn!(target: "EAPP_GL", "candidate_begin double-begin detected");
            }
        }
    }

    /// OpenGLES ordinal 21: `glCopyTexImage2D`. Vortex binds a screen-sized
    /// texture, copies the current title frame into it, and samples that
    /// texture on the following frame's menu composition.
    fn live_handle_copy_texture(&mut self, args: [u32; 4]) {
        let sp = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let y = self.read_guest_u32(sp).unwrap_or(0) as i32 as i64;
        let width = self.read_guest_u32(sp.wrapping_add(0x04)).unwrap_or(0) as usize;
        let height = self.read_guest_u32(sp.wrapping_add(0x08)).unwrap_or(0) as usize;
        let x = args[3] as i32 as i64;
        let tex_name = self.live_gl.as_ref().and_then(|lg| lg.bound_tex_name);
        if let Some(tex_name) = tex_name {
            if let Some(lg) = self.live_gl.as_mut() {
                lg.copy_framebuffer_to_texture(tex_name, x, y, width, height);
            }
        }
    }

    /// OpenGLES ordinal 105: `glTexSubImage2D`. The PR #3 oracle uses this
    /// to refill existing screen-sized textures when Vortex switches from
    /// the menu scene to the first level.
    fn live_handle_sub_upload(&mut self, args: [u32; 4]) {
        let sp = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let x = args[2] as usize;
        let y = args[3] as usize;
        let width = self.read_guest_u32(sp).unwrap_or(0) as usize;
        let height = self.read_guest_u32(sp.wrapping_add(0x04)).unwrap_or(0) as usize;
        let source_format = self.read_guest_u32(sp.wrapping_add(0x08)).unwrap_or(0);
        let pixel_type = self.read_guest_u32(sp.wrapping_add(0x0c)).unwrap_or(0);
        let source_ptr = self.read_guest_u32(sp.wrapping_add(0x10)).unwrap_or(0);
        let Some(format) = format_from_gl(source_format, pixel_type) else {
            warn!(
                target: "EAPP_GL",
                "live_sub_upload skipped: unsupported format src_fmt={:#x} pix_type={:#x}",
                source_format,
                pixel_type
            );
            return;
        };
        let expected = pix_payload_size(format, width, height);
        let Some(payload) = self.read_guest_bytes(source_ptr, expected) else {
            warn!(
                target: "EAPP_GL",
                "live_sub_upload skipped: short/invalid source ptr {:#010x} want={} bytes",
                source_ptr,
                expected
            );
            return;
        };
        let tex_name = self.live_gl.as_ref().and_then(|lg| lg.bound_tex_name);
        let Some(tex_name) = tex_name else {
            warn!(
                target: "EAPP_GL",
                "live_sub_upload skipped: no bound texture {}x{} at ({},{})",
                width,
                height,
                x,
                y
            );
            return;
        };
        let updated = self
            .live_gl
            .as_mut()
            .map(|lg| {
                lg.sub_image_to_texture(
                    tex_name,
                    x,
                    y,
                    width,
                    height,
                    source_format,
                    pixel_type,
                    &payload,
                )
            })
            .unwrap_or(false);
        if updated {
            info!(
                target: "EAPP_GL",
                "live_sub_upload tex#{:#x} {}x{} at ({},{}) fmt={:#x} type={:#x}",
                tex_name,
                width,
                height,
                x,
                y,
                source_format,
                pixel_type
            );
        } else {
            warn!(
                target: "EAPP_GL",
                "live_sub_upload skipped: no decoded texture tex#{:#x} {}x{} at ({},{})",
                tex_name,
                width,
                height,
                x,
                y
            );
        }
    }

    /// Candidate present from observed live ordering: ordinal 157 is the last
    /// surface ordinal and always follows steady-state draws. Neutral name;
    /// exact ABI semantics remain unproven.
    fn live_handle_candidate_present(&mut self) {
        // Lost shader state patch: if we have a bind material with a pending
        // shader state (0xffffffff at [state_ptr+0x60]), patch it to 0 so the
        // game's rendering engine thinks the shader compiled. This must happen
        // during present so the patched value is visible at the start of the
        // NEXT frame.
        if let Some(lg) = self.live_gl.as_ref() {
            let state_ptr = lg.current_state_ptr;
            let handle = lg.current_handle;
            if handle < 0x1000_0000 && (state_ptr >= 0x1000_0000 && state_ptr < 0x2000_0000) {
                let shader_state_off = 0x60;
                let shader_state = self
                    .read_guest_u32(state_ptr.wrapping_add(shader_state_off))
                    .unwrap_or(0);
                if shader_state == 0xffffffff {
                    self.write_guest_u32(state_ptr.wrapping_add(shader_state_off), 0);
                    info!(target: "EAPP_GL", "present: patched shader state 0xffffffff -> 0 at {:#010x}+0x60", state_ptr);
                }
            }
        }
        // One-time work RAM scan for Lost: scan the large allocation
        // buffer and rserver region for non-zero data written during init.
        if self.frame_counter == 10 && fliwheel_var_os("EAPP_LOST_MEMSCAN").is_some() {
            let alloc_base = 0x10502B00;
            let scan_ranges: [(u32, &str); 5] = [
                (alloc_base, "large_alloc"),
                (0x10012038, "rserver_data_0x11000"),
                (0x18060000, "game_heap_0x1806"),
                (0x10012700, "rserver_ptrs_0x10012700"),
                (0x1001F400, "rserver_ptrs_0x1001F400"),
            ];
            for (base, name) in scan_ranges.iter() {
                let mut non_zero_count = 0usize;
                let mut first5: Vec<String> = vec![];
                for i in (0..0x8000).step_by(4) {
                    let addr = base.wrapping_add(i);
                    let w = match self.read_guest_u32(addr) {
                        Some(w) => w,
                        None => break,
                    };
                    if w != 0 {
                        non_zero_count += 1;
                        if first5.len() < 5 {
                            first5.push(format!("+0x{:04x}={:#010x}", i, w));
                        }
                    }
                }
                if non_zero_count > 0 {
                    if *name == "rserver_data_0x11000"
                        || *name == "rserver_ptrs_0x10012700"
                        || *name == "rserver_ptrs_0x1001F400"
                    {
                        // Full dump for rserver data
                        let mut all_nz: Vec<String> = vec![];
                        for i in (0..0x8000).step_by(4) {
                            let addr = base.wrapping_add(i);
                            let w = match self.read_guest_u32(addr) {
                                Some(w) if w != 0 => Some(format!("+0x{:04x}={:#010x}", i, w)),
                                _ => None,
                            };
                            if let Some(s) = w {
                                all_nz.push(s);
                            }
                        }
                        info!(target: "EAPP_GL", "lost_memscan {} base={:#010x}: FULL DUMP:\n  {}",
                            name, base, all_nz.join(",\n  "));
                    } else {
                        info!(target: "EAPP_GL", "lost_memscan {} base={:#010x}: {}/0x2000 non-zero, first: {}",
                            name, base, non_zero_count, first5.join(", "));
                    }
                } else {
                    info!(target: "EAPP_GL", "lost_memscan {} base={:#010x}: all zeros", name, base);
                }
            }
        }
        // Patch 0xFFFFFFFF values in game heap for Lost. The game heap
        // has many -1 values that represent uninitialized render state.
        if fliwheel_var_os("EAPP_LOST_PATCH_NEG1").is_some() {
            // Scan multiple heap regions for -1 markers
            let patch_ranges: [(u32, u32); 4] = [
                (0x18040000, 0x20000), // game data 0x1804
                (0x18060000, 0x20000), // game heap 0x1806
                (0x18080000, 0x20000), // game data 0x1808
                (0x10010000, 0x10000), // rserver area 0x1001
            ];
            let mut total_patched = 0usize;
            for (base, size) in patch_ranges.iter() {
                for i in (0..*size).step_by(4) {
                    let addr = base.wrapping_add(i);
                    let w = match self.read_guest_u32(addr) {
                        Some(w) => w,
                        None => break,
                    };
                    if w == 0xFFFFFFFF {
                        self.write_guest_u32(addr, 0);
                        total_patched += 1;
                    }
                }
            }
            if total_patched > 0 && self.frame_counter <= 20 {
                info!(target: "EAPP_GL", "lost_patch_neg1: patched {} -1s frame={}", total_patched, self.frame_counter);
            }
        }
        let continuous = self
            .live_gl
            .as_ref()
            .map(|lg| lg.continuous_capture)
            .unwrap_or(false);
        if !continuous {
            return; // one-shot diagnostic capture keeps its existing heuristic
        }
        // Read DMA framebuffer before borrowing live_gl (for PopCap background)
        let mut dma_data = {
            let mut buf = vec![0u8; DMA_FB_SIZE];
            self.bus.dma_framebuf.bulk_read(0, &mut buf);
            buf
        };
        let mut has_dma = dma_data.iter().any(|b| *b != 0x2d);

        // Lost splash screen injection: if no DMA data and the env var is set,
        // load the lostLaunch.raw.lcd5 from the bundle and write it to the
        // DMA framebuffer so the overlay system displays it.
        if !has_dma && fliwheel_var_os("EAPP_LOST_SPLASH").is_some() && self.frame_counter <= 1 {
            let splash_path = self.metadata.bundle_dir.join("lostLaunch.raw.lcd5");
            if let Ok(data) = std::fs::read(&splash_path) {
                // 16-byte header: width(4) + height(4) + stride(4) + '565L'(4)
                // Then 320×216 RGB565 pixel data = 138,240 bytes
                if data.len() >= 16 && data.len() >= 16 + 320 * 216 * 2 {
                    let pixel_data = &data[16..16 + 320 * 216 * 2];
                    // Write to DMA framebuffer (fill top 216 rows, bottom 24 stay black)
                    self.bus.dma_framebuf.bulk_write(0, pixel_data);
                    // Zero the bottom rows
                    let bottom = vec![0u8; 320 * 24 * 2];
                    self.bus.dma_framebuf.bulk_write(320 * 216 * 2, &bottom);
                    has_dma = true; // Treat as DMA content
                    info!(target: "EAPP_GL", "lost_splash: wrote 320x216 RGB565 to DMA framebuffer from {:?}", splash_path);
                    // Re-read DMA for the overlay
                    dma_data = {
                        let mut buf = vec![0u8; DMA_FB_SIZE];
                        self.bus.dma_framebuf.bulk_read(0, &mut buf);
                        buf
                    };
                } else {
                    info!(target: "EAPP_GL", "lost_splash: file too small ({}) or wrong format", data.len());
                }
            } else {
                info!(target: "EAPP_GL", "lost_splash: file not found {:?}", splash_path);
            }
        }

        // Overlay DMA background into the live_gl framebuffer before completing
        if has_dma {
            if let Some(lg) = self.live_gl.as_mut() {
                lg.overlay_dma_rgb565(&dma_data);
            }
        }

        let completed = match self.live_gl.as_mut().and_then(|lg| lg.complete_frame()) {
            Some(frame) => frame,
            None => {
                // The Sims/Sudoku/Solitaire engine family never calls
                // ordinal-158 (begin frame). Its per-frame loop is
                // 159 → 149 → 157 with no explicit begin. Auto-begin
                // so the present can succeed, the same way draws auto-
                // begin when ordinal-158 is absent.
                if let Some(lg) = self.live_gl.as_mut() {
                    lg.begin_frame();
                }
                // Overlay DMA background before completing the frame
                let dma_data = {
                    let mut buf = vec![0u8; DMA_FB_SIZE];
                    self.bus.dma_framebuf.bulk_read(0, &mut buf);
                    buf
                };
                let has_dma = dma_data.iter().any(|b| *b != 0x2d);
                if has_dma {
                    if let Some(lg) = self.live_gl.as_mut() {
                        lg.overlay_dma_rgb565(&dma_data);
                    }
                }
                match self.live_gl.as_mut().and_then(|lg| lg.complete_frame()) {
                    Some(frame) => frame,
                    None => {
                        warn!(target: "EAPP_GL", "candidate_present auto-begin still failed; discarded");
                        return;
                    }
                }
            }
        };

        // Continuous rendering publishes completed frames. 0-draw idle
        // frames (Sudoku/Solitaire input-wait loops) now preserve the
        // previous frame's content, so they are safe to present.
        let should_present = true;
        self.live_log_completed_frame(&completed, should_present);
        self.live_log_signature_detail(&completed);
        if should_present {
            self.capture_startup_completed_frame(&completed);
            self.live_dump_completed_frame();
            if self.live_gl.as_ref().map(|lg| lg.gate_b).unwrap_or(false) {
                self.live_present_completed_to_window();
            }
        }
    }

    /// Implement the ordinary `glGenTextures(n, out)` form seen by the Vortex
    /// eApp. The old resource-descriptor probe also lives on ordinal 45 for a
    /// few titles, so callers run that guarded probe afterward; the zero
    /// `r2` marker distinguishes the generated-name ABI from its dimensioned
    /// descriptor form.
    fn live_handle_gen_textures(&mut self, args: [u32; 4]) {
        let count = args[0];
        let out = args[1];
        if count == 0 || count > 256 || out == 0 || args[2] != 0 {
            return;
        }
        let mut generated = 0u32;
        for i in 0..count {
            self.next_texture_name = self.next_texture_name.wrapping_add(1);
            if !self.write_guest_u32(out.wrapping_add(i.wrapping_mul(4)), self.next_texture_name) {
                break;
            }
            generated += 1;
        }
        if generated != 0 {
            info!(
                target: "EAPP_GL",
                "glGenTextures n={} out={:#010x} first={} last={}",
                generated,
                out,
                self.next_texture_name.saturating_sub(generated).saturating_add(1),
                self.next_texture_name,
            );
        }
    }

    /// Ordinal 45: Mahjong-style resource texture descriptor. Captures show
    /// r1 pointing at a stack descriptor whose word 1 points at a work-RAM
    /// texture object. That object carries packed dimensions at word 4,
    /// material handle at word 2, pixel pointer at word 9, and a format-ish
    /// word at word 10 (`0x8808`/`0x0801` observed as A8 resources).
    ///
    /// This is deliberately guarded to copied guest bytes from mapped work RAM;
    /// unsupported shapes are ignored so ordinal-99 remains the primary upload
    /// path for Tetris and most other games.
    fn live_handle_resource_upload(&mut self, args: [u32; 4]) {
        let desc_ptr = args[1];
        let prep_width = args[2] as usize;
        let prep_height = args[3] as usize;
        // Even when prep_width/prep_height are 0 (TWA/iQuiz style ordinal-45
        // calls that just set up texture handles), still try to capture the
        // pending texture name from the descriptor for later match.
        // The full Mahjong-style upload path below still requires valid dims.
        // Cross-title evidence (Tetris, Texas Hold'em): the ordinal-45
        // descriptor at r1 is the texture object itself (not a pointer to
        // one) with layout:
        //   word0 = 0, word1 = GL texture name (small int, e.g. 0x13),
        //   word2 = source_format (GL_RGB/GL_RGBA/GL_ALPHA/GL_LUMINANCE_ALPHA),
        //   word3 = pixel_type, word4 = width, word5 = height, ...
        // The texture name is the same handle later bound by ordinal 159, so
        // capturing it here lets the following ordinal-99 `glTexImage2D` tag
        // its upload with the GL texture name and draw-time selection can match
        // by handle instead of (lossy) dimension inference. Distinguish this
        // layout from Mahjong's pointer-to-object layout by checking that
        // word1 is a small integer and word2 is a recognized GL format enum.
        if let (Some(w1), Some(w2)) = (
            self.read_guest_u32(desc_ptr.wrapping_add(4)),
            self.read_guest_u32(desc_ptr.wrapping_add(8)),
        ) {
            let is_small_handle = w1 != 0 && w1 < WORK_RAM_BASE;
            let is_gl_format = matches!(w2, 0x1906 | 0x1907 | 0x1908 | 0x190a);
            if is_small_handle && is_gl_format {
                if let Some(lg) = self.live_gl.as_mut() {
                    lg.pending_tex_name = Some(w1);
                }
            } else if !is_gl_format || !is_small_handle {
                // Log the descriptor contents for RE when the capture fails
                let w0 = self.read_guest_u32(desc_ptr).unwrap_or(0);
                let w3 = self.read_guest_u32(desc_ptr.wrapping_add(12)).unwrap_or(0);
                let w4 = self.read_guest_u32(desc_ptr.wrapping_add(16)).unwrap_or(0);
                let w5 = self.read_guest_u32(desc_ptr.wrapping_add(20)).unwrap_or(0);
                debug!(target: "EAPP_GL", "ordinal_45 desc missed: ptr={:#010x} w0={:#010x} w1={:#010x} w2={:#010x} w3={:#010x} w4={:#010x} w5={:#010x} small={} gl_fmt={} prep_w={} prep_h={}",
                    desc_ptr, w0, w1, w2, w3, w4, w5, is_small_handle, is_gl_format, prep_width, prep_height);
            }
        }
        // Early return for ordinal-45 calls that have no proper width/height
        // (TWA/iQuiz style: just set up texture handles, no bulk upload).
        if prep_width == 0 || prep_height == 0 {
            return;
        }
        let Some(texture_obj) = self.read_guest_u32(desc_ptr.wrapping_add(4)) else {
            return;
        };
        if !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&texture_obj) {
            return;
        }
        let Some(words) = self.read_guest_words_exact(texture_obj, 12) else {
            return;
        };
        let material_handle = words[2];
        let packed_dims = words[4];
        let width = (packed_dims & 0xffff) as usize;
        let height = (packed_dims >> 16) as usize;
        let pixel_ptr = words[9];
        let resource_format = words[10];
        if width == 0
            || height == 0
            || width != prep_width
            || height != prep_height
            || material_handle == 0
            || pixel_ptr == 0
            || width > 4096
            || height > 4096
        {
            return;
        }
        let Some(format) = ordinal45_resource_format(resource_format) else {
            warn!(
                target: "EAPP_GL",
                "ordinal45 resource skipped: unsupported fmt={:#x} handle={:#x} {}x{} ptr={:#010x}",
                resource_format,
                material_handle,
                width,
                height,
                pixel_ptr
            );
            return;
        };
        let byte_len = match format {
            TextureFormat::Rgb565 | TextureFormat::Rgba5551 | TextureFormat::Rgba4444 => {
                width.saturating_mul(height).saturating_mul(2)
            }
            TextureFormat::Rgba8888 => width.saturating_mul(height).saturating_mul(4),
            TextureFormat::LuminanceAlpha88 => width.saturating_mul(height).saturating_mul(2),
            TextureFormat::A8 => width.saturating_mul(height),
        };
        if byte_len == 0 || byte_len > 16 * 1024 * 1024 {
            return;
        }
        let Some(bytes) = self.read_guest_bytes(pixel_ptr, byte_len) else {
            warn!(
                target: "EAPP_GL",
                "ordinal45 resource skipped: invalid pixel ptr {:#010x} len={} handle={:#x}",
                pixel_ptr,
                byte_len,
                material_handle
            );
            return;
        };
        if bytes.len() != byte_len {
            return;
        }

        if let Some(lg) = self.live_gl.as_mut() {
            if let Some(existing) = lg.uploads.iter().find(|u| {
                u.source_ptr == pixel_ptr
                    && u.width == width
                    && u.height == height
                    && u.source_format == resource_format
            }) {
                lg.resource_uploads_by_handle
                    .insert(material_handle, existing.index);
                return;
            }
            let index = lg.uploads.len();
            let texture = Texture::from_bytes(
                &bytes,
                width,
                height,
                format,
                Rgba8::rgba(255, 255, 255, 255),
            );
            lg.uploads.push(live_gl::LiveGlUpload {
                index,
                target: 0,
                width,
                height,
                source_format: resource_format,
                pixel_type: 0,
                source_ptr: pixel_ptr,
                source_file: None,
                source_file_offset: None,
                format: Some(format),
                texture: Some(texture),
                tex_name: Some(material_handle),
            });
            lg.resource_uploads_by_handle.insert(material_handle, index);
            info!(
                target: "EAPP_GL",
                "ordinal45 resource upload #{} handle={:#x} {}x{} fmt={:#x} ptr={:#010x}",
                index,
                material_handle,
                width,
                height,
                resource_format,
                pixel_ptr
            );
        }
    }

    /// Ordinal 99: copy guest pixel bytes immediately, validate bounds, and
    /// build a live texture. Supports RGB565/RGBA5551/RGBA4444/A8. Row order
    /// is preserved exactly as uploaded.
    fn live_handle_upload(&mut self, args: [u32; 4]) {
        let target = args[0];
        let width = args[3];
        let sp = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let height = self.read_guest_u32(sp).unwrap_or(0);
        let source_format = self.read_guest_u32(sp.wrapping_add(0x08)).unwrap_or(0);
        let pixel_type = self.read_guest_u32(sp.wrapping_add(0x0c)).unwrap_or(0);
        let source_ptr = self.read_guest_u32(sp.wrapping_add(0x10)).unwrap_or(0);

        if source_ptr == 0 || width == 0 || height == 0 {
            warn!(
                target: "EAPP_GL",
                "live_upload skipped: invalid dims/ptr target={:#x} {}x{} src={:#010x}",
                target, width, height, source_ptr
            );
            return;
        }
        let format = format_from_gl(source_format, pixel_type);
        if format.is_none() {
            warn!(
                target: "EAPP_GL",
                "live_upload skipped: unsupported format src_fmt={:#x} pix_type={:#x}",
                source_format, pixel_type
            );
            return;
        }
        let expected = pix_payload_size(format.unwrap(), width as usize, height as usize);
        let payload = match self.read_guest_bytes(source_ptr, expected) {
            Some(bytes) if bytes.len() == expected => bytes,
            _ => {
                warn!(
                    target: "EAPP_GL",
                    "live_upload skipped: short/invalid source ptr {:#010x} want={} bytes",
                    source_ptr, expected
                );
                return;
            }
        };

        let index = self.live_gl.as_ref().map(|l| l.uploads.len()).unwrap_or(0);
        let backing = self.file_backing_for_addr(source_ptr);
        let tex_name = self
            .live_gl
            .as_mut()
            .and_then(|l| l.pending_tex_name.take());
        let mut upload = LiveGlState::build_upload(
            index,
            target,
            width,
            height,
            source_format,
            pixel_type,
            source_ptr,
            &payload,
            tex_name,
        );
        if let Some(backing) = backing {
            upload.source_file_offset = Some(source_ptr.saturating_sub(backing.base_addr));
            upload.source_file = Some(backing.path);
        }
        info!(
            target: "EAPP_GL",
            "live_upload idx={} {}x{} format={:?} src_fmt={:#x} pix_type={:#x} src_ptr={:#010x} bytes={} file={} file_off={} tex_name={}",
            index,
            width,
            height,
            upload.format,
            source_format,
            pixel_type,
            source_ptr,
            payload.len(),
            upload.source_file.as_deref().unwrap_or("<unknown>"),
            upload
                .source_file_offset
                .map(|off| format!("{}", off))
                .unwrap_or_else(|| "<unknown>".to_string()),
            upload
                .tex_name
                .map(|n| format!("{:#x}", n))
                .unwrap_or_else(|| "<none>".to_string()),
        );
        if let Some(lg) = self.live_gl.as_mut() {
            lg.uploads.push(upload);
        }
    }

    /// Ordinal 19: paletted glCompressedTexImage2D uploads.
    ///
    /// The iPod's compressed texture call uses four register arguments and
    /// four stack arguments: target, level, format, width, then height,
    /// border, image size, and a pointer to a palette followed by
    /// width*height one-byte indices. RGBA8 uses a 1024-byte palette and
    /// RGB565 uses a 512-byte palette. The data is copied immediately so a
    /// later async-file buffer reuse cannot change the captured texture.
    fn live_handle_compressed_upload(&mut self, args: [u32; 4]) {
        let width = args[3] as usize;
        let sp = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let height = self.read_guest_u32(sp).unwrap_or(0) as usize;
        let image_size = self.read_guest_u32(sp.wrapping_add(0x08)).unwrap_or(0) as usize;
        let source_ptr = self.read_guest_u32(sp.wrapping_add(0x0c)).unwrap_or(0);

        if width == 0 || height == 0 || width > 4096 || height > 4096 || source_ptr == 0 {
            warn!(
                target: "EAPP_GL",
                "compressed_upload skipped: invalid dims/ptr {}x{} src={:#010x}",
                width,
                height,
                source_ptr
            );
            return;
        }
        let Some(entry_size) = palette8_entry_size(args[2]) else {
            warn!(
                target: "EAPP_GL",
                "compressed_upload skipped: unsupported palette format={:#x}",
                args[2]
            );
            return;
        };
        let pixel_count = width.saturating_mul(height);
        let expected = entry_size.saturating_mul(256).saturating_add(pixel_count);
        if image_size < expected {
            warn!(
                target: "EAPP_GL",
                "compressed_upload skipped: short image_size={} want={} {}x{} src={:#010x}",
                image_size,
                expected,
                width,
                height,
                source_ptr
            );
            return;
        }
        let Some(raw) = self.read_guest_bytes(source_ptr, expected) else {
            warn!(
                target: "EAPP_GL",
                "compressed_upload skipped: invalid source ptr {:#010x} want={} bytes",
                source_ptr,
                expected
            );
            return;
        };
        let Some(pixels) = decode_palette8(&raw, width, height, args[2]) else {
            warn!(
                target: "EAPP_GL",
                "compressed_upload skipped: palette data is shorter than {}x{}",
                width,
                height
            );
            return;
        };

        let index = self.live_gl.as_ref().map(|l| l.uploads.len()).unwrap_or(0);
        let backing = self.file_backing_for_addr(source_ptr);
        let tex_name = self
            .live_gl
            .as_mut()
            .and_then(|l| l.pending_tex_name.take());
        let upload = live_gl::LiveGlUpload {
            index,
            target: args[0],
            width,
            height,
            source_format: args[2],
            pixel_type: image_size as u32,
            source_ptr,
            source_file: backing.as_ref().map(|b| b.path.clone()),
            source_file_offset: backing
                .as_ref()
                .map(|b| source_ptr.saturating_sub(b.base_addr)),
            format: Some(TextureFormat::Rgba8888),
            texture: Some(Texture {
                width,
                height,
                pixels,
            }),
            tex_name,
        };
        info!(
            target: "EAPP_GL",
            "compressed_upload idx={} {}x{} format={:#x} image_size={} src_ptr={:#010x} bytes={} file={} file_off={} tex_name={}",
            index,
            width,
            height,
            args[2],
            image_size,
            source_ptr,
            expected,
            upload.source_file.as_deref().unwrap_or("<unknown>"),
            upload
                .source_file_offset
                .map(|off| format!("{}", off))
                .unwrap_or_else(|| "<unknown>".to_string()),
            upload
                .tex_name
                .map(|name| format!("{:#x}", name))
                .unwrap_or_else(|| "<none>".to_string()),
        );
        if let Some(lg) = self.live_gl.as_mut() {
            lg.uploads.push(upload);
        }
    }

    /// Ordinal 137: record an array definition (direct args + sp+0, sp+4).
    /// Unknown array slots are preserved without semantic naming.
    ///
    /// Cross-title evidence (Cubis 2, Mahjong, Ms. PAC-MAN) shows some games
    /// issue `DrawArrays` immediately after ordinal 137 without a separate
    /// explicit enable for array 0. To match observed behavior, defining a
    /// valid client array also marks that slot enabled.
    fn live_handle_array_def(&mut self, args: [u32; 4]) {
        let array_index = args[0];
        let mut component_count = args[1];
        let format = args[2];
        let sp = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let stride = self.read_guest_u32(sp).unwrap_or(0);
        let guest_ptr = self.read_guest_u32(sp.wrapping_add(0x04)).unwrap_or(0);
        // VBO fix: when a VBO is active, the game may pass a VBO offset/pointer
        // in args[1] instead of a real component count. Detect this by checking
        // for absurdly large values and infer the real count from format+stride.
        // GL_FIXED is 4 bytes per component. If stride > 0, estimate comps from
        // stride / sizeof(GLfixed). Otherwise, use the last known good count.
        if component_count > 32 {
            let inferred = if format == live_gl::GL_FIXED && stride > 0 && stride % 4 == 0 {
                let total_comps = stride / 4; // total components across all arrays in this struct
                                              // For the position array (idx=0), typically 4 comps (x,y,z,w)
                                              // For UV array (idx=1), typically 2 comps (u,v)
                                              // Use a conservative estimate based on array_index
                if array_index == 0 {
                    total_comps.min(4)
                } else if array_index == 1 {
                    2
                } else {
                    4
                }
            } else {
                // Fallback: use 4 for pos, 2 for UV
                if array_index == 0 {
                    4
                } else if array_index == 1 {
                    2
                } else {
                    4
                }
            };
            info!(
                target: "EAPP_GL",
                "live_array idx={} VBO-mode comps={:#x} -> inferred {} (fmt={:#x} stride={})",
                array_index, component_count, inferred, format, stride
            );
            component_count = inferred;
        }
        let valid = guest_ptr != 0 && component_count != 0;
        info!(
            target: "EAPP_GL",
            "live_array idx={} comps={} format={:#x} stride={} ptr={:#010x} valid={}",
            array_index, component_count, format, stride, guest_ptr, valid
        );
        if let Some(lg) = self.live_gl.as_mut() {
            let def = live_gl::LiveArrayDef {
                array_index,
                component_count,
                format,
                stride,
                guest_ptr,
                valid,
                material_epoch: lg.current_material_epoch,
            };
            lg.arrays.insert(array_index, def);
            if valid {
                lg.enabled_arrays.insert(array_index);
            }
        }
        // Diagnostic: dump array contents once per unique pointer when the
        // current material is pointer-backed. Helps decode glyph/UV layouts.
        if texgen_verbose_enabled()
            && valid
            && guest_ptr != 0
            && self.dumped_array_ptrs.insert(guest_ptr)
            && format == live_gl::GL_FIXED
        {
            let words_per_vertex = component_count as usize;
            // Dump up to 16 vertices (enough to see 4 quads of glyph data)
            let vertex_count = 16;
            let total_words = words_per_vertex * vertex_count;
            let words = self.read_guest_words(guest_ptr, total_words);
            // Render as vertices for readability
            let mut rendered = String::new();
            for v in 0..vertex_count {
                let base = v * words_per_vertex;
                if base >= words.len() {
                    break;
                }
                let comps: Vec<String> = words[base..(base + words_per_vertex).min(words.len())]
                    .iter()
                    .map(|w| {
                        // Render as both hex and fixed-point float for diagnosis
                        let f = decode_fixed_16_16(*w);
                        format!("{:#010x}({:.2})", w, f)
                    })
                    .collect();
                if !rendered.is_empty() {
                    rendered.push(',');
                }
                rendered.push_str(&format!("v{}=[{}]", v, comps.join(",")));
            }
            info!(
                target: "EAPP_GL",
                "array_dump idx={} ptr={:#010x} comps={} stride={} vertices=[{}]",
                array_index, guest_ptr, component_count, stride, rendered
            );
        }
    }

    /// Resolve the two client-array roles used by the direct PR runner. Slot
    /// 1 is normally UVs, but some titles use that slot for RGBA primary
    /// colours; slot 2 is the common colour-array location when slot 1 remains
    /// a two-component UV array. Keep the role decision based on component
    /// count rather than assuming every non-position array is a UV array.
    fn live_select_uv_and_color_arrays(
        lg: &live_gl::LiveGlState,
    ) -> (
        Option<live_gl::LiveArrayDef>,
        bool,
        Option<live_gl::LiveArrayDef>,
        bool,
    ) {
        let uv = lg
            .arrays
            .get(&1)
            .filter(|def| {
                def.valid && def.format == live_gl::GL_FIXED && def.component_count == 2
            })
            .cloned()
            .or_else(|| {
                lg.arrays.get(&2).filter(|def| {
                    def.valid && def.format == live_gl::GL_FIXED && def.component_count == 2
                }).cloned()
            });
        let colors = lg
            .arrays
            .get(&1)
            .filter(|def| {
                def.valid && def.format == live_gl::GL_FIXED && def.component_count >= 3
            })
            .cloned()
            .or_else(|| {
                lg.arrays.get(&2).filter(|def| {
                    def.valid && def.format == live_gl::GL_FIXED && def.component_count >= 3
                }).cloned()
            });
        let uv_enabled = uv
            .as_ref()
            .is_some_and(|def| lg.enabled_arrays.contains(&def.array_index));
        let color_enabled = colors
            .as_ref()
            .is_some_and(|def| lg.enabled_arrays.contains(&def.array_index));
        (uv, uv_enabled, colors, color_enabled)
    }

    /// Ordinal 40: enable/select an array by index (direct arg r0 only).
    fn live_handle_enable_array(&mut self, args: [u32; 4]) {
        let array_index = args[0];
        if let Some(lg) = self.live_gl.as_mut() {
            lg.enabled_arrays.insert(array_index);
        }
        debug!(target: "EAPP_GL", "live_enable_array idx={}", array_index);
    }

    /// Ordinal 36: disable/select an array by index (direct arg r0 only).
    ///
    /// Vortex disables its colour array before the alpha-only outer-ring draw.
    /// Leaving that client-state bit latched makes the fliwheel combiner treat
    /// the stale colour array as the primary colour and, more importantly,
    /// suppresses the PR runner's alpha-only current-colour modulation.
    fn live_handle_disable_array(&mut self, args: [u32; 4]) {
        let array_index = args[0];
        if let Some(lg) = self.live_gl.as_mut() {
            lg.enabled_arrays.remove(&array_index);
        }
        debug!(target: "EAPP_GL", "live_disable_array idx={}", array_index);
    }

    /// Ordinal 169: accumulate the guest's two-component draw transform
    /// (r1=first component, r2=second component as floats). Normal sprite
    /// groups reset this state after each draw; Tetris' cell groups restore
    /// the board origin before applying their local tile transform.
    fn live_handle_translate(&mut self, args: [u32; 4]) {
        let tx = f32::from_bits(args[1]);
        let ty = f32::from_bits(args[2]);
        if let Some(lg) = self.live_gl.as_mut() {
            lg.translation.0 += tx;
            lg.translation.1 += ty;
        }
    }

    /// Apply the matrix helpers used by the direct PR #3 Vortex oracle.
    ///
    /// Vortex builds the rotating outer overlay in guest memory with
    /// `translate`, `rotate`, and `multMatrix`; the next vertex-array
    /// submission reads the resulting coordinates from that state. The
    /// ordinary translation accumulator remains the compatibility path for
    /// other title families, so these writes stay behind the Vortex PR3 A/B
    /// flag.
    fn live_handle_vortex_matrix_op(&mut self, ordinal: u32, args: [u32; 4]) {
        if self.metadata.title != "12345" || fliwheel_var_os("VORTEX_PR3_GL165").is_none() {
            return;
        }
        let dst = args[0];
        if dst == 0 {
            return;
        }
        let stack_z = self
            .read_guest_u32(self.cpu.reg_get(self.cpu.mode(), reg::SP))
            .map(f32::from_bits)
            .unwrap_or(0.0);
        let out = if ordinal == 175 {
            let a = self.read_guest_float_matrix(args[1]);
            let b = self.read_guest_float_matrix(args[2]);
            a.zip(b).map(|(a, b)| multiply_column_major(&a, &b))
        } else {
            let current = self.read_guest_float_matrix(dst);
            current.and_then(|current| matrix_helper_transform(ordinal, current, args, stack_z))
        };
        let Some(matrix) = out else {
            return;
        };
        for (index, value) in matrix.iter().enumerate() {
            if !self.write_guest_u32(dst.wrapping_add((index as u32) * 4), value.to_bits()) {
                return;
            }
        }
        if fliwheel_var_os("EAPP_GL_MATRIX_LOG").is_some() {
            info!(
                target: "EAPP_GL",
                "vortex_matrix frame={} ordinal={} dst={:#010x} args={args:?} stack_z={stack_z} m={matrix:?}",
                self.frame_counter,
                ordinal,
                dst
            );
        }
    }

    fn read_guest_float_matrix(&mut self, addr: u32) -> Option<[f32; 16]> {
        if addr == 0 {
            return None;
        }
        let mut matrix = [0.0; 16];
        for (index, value) in matrix.iter_mut().enumerate() {
            *value = f32::from_bits(self.read_guest_u32(addr.wrapping_add((index as u32) * 4))?);
        }
        Some(matrix)
    }

    fn live_handle_vortex_ortho(&mut self, args: [u32; 4]) {
        let stack = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let top = self
            .read_guest_u32(stack)
            .map(f32::from_bits)
            .unwrap_or(0.0);
        let z_near = self
            .read_guest_u32(stack.wrapping_add(4))
            .map(f32::from_bits)
            .unwrap_or(0.0);
        let z_far = self
            .read_guest_u32(stack.wrapping_add(8))
            .map(f32::from_bits)
            .unwrap_or(0.0);
        let Some(matrix) = orthographic_matrix(args, top, z_near, z_far) else {
            info!(
                target: "EAPP_GL",
                "vortex_ortho skipped dst={:#010x} top={} z_near={} z_far={}",
                args[0],
                top,
                z_near,
                z_far
            );
            return;
        };
        for (index, value) in matrix.iter().enumerate() {
            if !self.write_guest_u32(args[0].wrapping_add((index as u32) * 4), value.to_bits()) {
                return;
            }
        }
        info!(
            target: "EAPP_GL",
            "vortex_ortho dst={:#010x} m={matrix:?}",
            args[0]
        );
    }

    /// Capture the Vortex vertex MVP uploaded by OpenGLES:125. The direct
    /// runner's ABI is `(location, count, transpose, matrix_ptr)`; only the
    /// location-zero upload participates in the vertex position path.
    fn live_handle_vortex_mvp(&mut self, args: [u32; 4]) {
        if args[0] != 0 || args[1] == 0 || args[3] == 0 {
            return;
        }
        let Some(matrix) = self.read_guest_float_matrix(args[3]) else {
            return;
        };
        if let Some(lg) = self.live_gl.as_mut() {
            lg.mvp = Some(matrix);
        }
        if fliwheel_var_os("GL_MATRIX_LOG").is_some() {
            info!(
                target: "EAPP_GL",
                "vortex_mvp ptr={:#010x} m={matrix:?}",
                args[3]
            );
        }
    }

    /// OpenGLES:13, glClearColor(r, g, b, a).
    fn live_handle_clear_color(&mut self, args: [u32; 4]) {
        let channels = [
            f32::from_bits(args[0]),
            f32::from_bits(args[1]),
            f32::from_bits(args[2]),
            f32::from_bits(args[3]),
        ];
        let to_u8 = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        let color = Rgba8::rgba(
            to_u8(channels[0]),
            to_u8(channels[1]),
            to_u8(channels[2]),
            to_u8(channels[3]),
        );
        if let Some(lg) = self.live_gl.as_mut() {
            lg.set_clear_color(color);
        }
        debug!(
            target: "EAPP_GL",
            "live_clear_color rgba=({},{},{},{})",
            color.r,
            color.g,
            color.b,
            color.a
        );
    }

    /// OpenGLES:12, glClear(mask).
    fn live_handle_clear(&mut self, args: [u32; 4]) {
        let mask = args[0];
        if let Some(lg) = self.live_gl.as_mut() {
            lg.clear(mask);
        }
        debug!(target: "EAPP_GL", "live_clear mask={:#x}", mask);
    }

    fn live_set_uniform_color(&mut self, location: i32, values: [f32; 4]) {
        if location != 4 {
            return;
        }
        let to_u8 = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        let color = Rgba8::rgba(
            to_u8(values[0]),
            to_u8(values[1]),
            to_u8(values[2]),
            to_u8(values[3]),
        );
        if let Some(lg) = self.live_gl.as_mut() {
            lg.set_modulate(color);
        }
        debug!(
            target: "EAPP_GL",
            "live_uniform_color loc={} rgba=({},{},{},{})",
            location,
            color.r,
            color.g,
            color.b,
            color.a
        );
    }

    /// OpenGLES:148/120, glUniform4x[v] at the colour-register location.
    /// `fixed` selects the 16.16 fixed-point or IEEE-float payload form.
    fn live_handle_uniform4x(&mut self, args: [u32; 4], fixed: bool) {
        let location = args[0] as i32;
        let count = args[1];
        let pointer = args[2];
        if count == 0 || pointer == 0 {
            return;
        }
        let decode = |word: u32| {
            if fixed {
                word as i32 as f32 / 65_536.0
            } else {
                f32::from_bits(word)
            }
        };
        let mut values = [0.0; 4];
        for (index, value) in values.iter_mut().enumerate() {
            let Some(word) = self.read_guest_u32(pointer.wrapping_add((index * 4) as u32)) else {
                return;
            };
            *value = decode(word);
        }
        self.live_set_uniform_color(location, values);
    }

    /// OpenGLES:147, glUniform4xAPPLE(location, x, y, z, w). The fourth
    /// component is the first stack argument in the ARM import ABI.
    fn live_handle_uniform4x_scalar(&mut self, args: [u32; 4]) {
        let stack = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let Some(w) = self.read_guest_u32(stack) else {
            return;
        };
        let decode = |word: u32| word as i32 as f32 / 65_536.0;
        self.live_set_uniform_color(
            args[0] as i32,
            [decode(args[1]), decode(args[2]), decode(args[3]), decode(w)],
        );
    }

    /// Ordinal 159: record the small selector/handle (r0) and state blob
    /// pointer (r1). The exact handle-creation path remains unsolved.
    fn live_handle_bind_material(&mut self, args: [u32; 4]) {
        let handle = args[0];
        let state_ptr = args[1];
        if let Some(lg) = self.live_gl.as_mut() {
            if lg.game_id == "66666"
                && lg.use_incremental_translation
                && matches!(handle, 0x13 | 0x19)
            {
                if !lg.frame_material_bound {
                    // Incremental frames arrive with an already-composed
                    // guest base before the first material bind. Capture it
                    // as-is; applying the full-board origin here would erase
                    // the active piece's guest-local placement.
                    lg.frame_base_translation = lg.translation;
                } else {
                    lg.translation = lg.frame_base_translation;
                }
                lg.frame_material_bound = true;
            } else if lg.game_id == "66666" && handle == 0x19 && lg.board_base_translation_valid {
                // Non-incremental frames use the centered origin learned
                // from the complete matrix draw.
                lg.translation = lg.frame_base_translation;
            }
            lg.current_handle = handle;
            lg.current_state_ptr = state_ptr;
            lg.current_material_epoch = lg.current_material_epoch.wrapping_add(1);
            // A material bind starts a fresh transform context for the next
            // draw group. Pointer text glyph loops then carry their own
            // per-glyph translation between draws until the next bind.
            lg.pointer_text_carry_handle = None;
            lg.pointer_text_carry = (0.0, 0.0);
        }
        info!(
            target: "EAPP_GL",
            "live_bind_material handle={:#x} state_ptr={:#010x}",
            handle, state_ptr
        );
        // One-time dump of state_ptr object for shader-program materials
        // (small handle + high state_ptr = likely shader-bound resource)
        if handle < 0x1000_0000 && (state_ptr >= 0x1000_0000 && state_ptr < 0x2000_0000) {
            if self.dumped_pointer_handles.insert(state_ptr) {
                let words = self.read_guest_words(state_ptr, 32);
                let shader_state = self
                    .read_guest_u32(state_ptr.wrapping_add(0x60))
                    .unwrap_or(0);
                let hex: Vec<String> = words.iter().map(|w| format!("{:#010x}", w)).collect();
                info!(target: "EAPP_GL", "bind_material handle={:#x} state_ptr={:#010x} shader_st={:#010x} words=[{}]", handle, state_ptr, shader_state, hex.join(","));
            }
        }
        // Pointer handles are work-RAM addresses. Dump the object layout once
        // (via texgen verbose) only if not already dumped above.
        if texgen_verbose_enabled()
            && (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&handle)
            && self.dumped_pointer_handles.insert(handle)
        {
            self.live_dump_pointer_handle_object(handle, state_ptr);
        }
    }

    /// Ordinal 165: surface/context bind (Vortex/iQuiz/Texas Hold'em fix).
    ///
    /// Vortex crashes at `pc=0x18014d58` with null pointer writes. The game
    /// calls OpenGLES:165 with r0=0x18063ebc (container in mapped image).
    /// The container needs:
    ///   +0x54: count (should be 1)
    ///   +0x5c: array_ptr -> object
    ///   +0x04: object_ptr -> buffer (via ldr r0, [r0, #4] at crash site)
    ///
    /// The object at +0x04 needs:
    ///   +0x00: vtable
    ///   +0x04: buffer_ptr (this is what stmia writes to)
    fn live_handle_ordinal_165(&mut self, args: [u32; 4]) {
        let state_ptr = args[0];
        let r1 = args[1];
        info!(
            target: "EAPP_GL",
            "ordinal_165: state_ptr={:#010x} r1={:#010x} r2={:#010x} r3={:#010x}",
            state_ptr, r1, args[2], args[3]
        );

        // The direct PR #3 runner binds ordinal 165 to the ordinary
        // glLoadIdentity contract. Keep the older Vortex surface wiring
        // available for comparison, but allow an exact oracle-style A/B.
        if self.metadata.title == "12345" && fliwheel_var_os("VORTEX_PR3_GL165").is_some() {
            if state_ptr != 0 {
                for i in 0..16u32 {
                    let value = if i % 5 == 0 {
                        1.0f32.to_bits()
                    } else {
                        0
                    };
                    let _ = self.write_guest_u32(state_ptr.wrapping_add(i * 4), value);
                }
            }
            info!(
                target: "EAPP_GL",
                "ordinal_165: PR #3 glLoadIdentity state_ptr={:#010x}",
                state_ptr
            );
            return;
        }

        // Vortex-specific: container at 0x18063ebc needs full structure wiring
        if state_ptr == 0x18063ebc {
            // Check if already initialized (avoid double-alloc)
            let current_array = self
                .read_guest_u32(state_ptr.wrapping_add(0x5c))
                .unwrap_or(0);
            if current_array != 0 {
                info!(target: "EAPP_GL", "ordinal_165: Vortex container already initialized");
                return;
            }

            info!(target: "EAPP_GL", "ordinal_165: initializing Vortex surface structures");

            // Read current container state before modification
            let before_c4 = self
                .read_guest_u32(state_ptr.wrapping_add(4))
                .unwrap_or(0xdead);
            let before_c54 = self
                .read_guest_u32(state_ptr.wrapping_add(0x54))
                .unwrap_or(0xdead);
            let before_c5c = self
                .read_guest_u32(state_ptr.wrapping_add(0x5c))
                .unwrap_or(0xdead);
            info!(target: "EAPP_GL", "ordinal_165: container BEFORE: +4={:#010x} +54={:#010x} +5c={:#010x}", 
                  before_c4, before_c54, before_c5c);

            // Allocate work-RAM structures
            let surface_size = 320u32 * 240 * 2; // RGB565 framebuffer
            let surface_buf = self.alloc_zeroed(surface_size);
            let object = self.alloc_zeroed(0x40); // 64-byte object
            let array = self.alloc_zeroed(0x4); // 4-byte array (single pointer)

            if surface_buf == 0 || object == 0 || array == 0 {
                warn!(target: "EAPP_GL", "ordinal_165: Vortex allocation failed");
                return;
            }

            info!(target: "EAPP_GL", "ordinal_165: allocated object={:#010x} buffer={:#010x} array={:#010x}",
                  object, surface_buf, array);

            // Wire up the structure chain
            // Object layout: +0=vtable, +4=buffer_ptr (for ldr r0, [r0, #4])
            let w1 = self.write_guest_u32(object.wrapping_add(0), WORK_RAM_BASE + 0x1000);
            let w2 = self.write_guest_u32(object.wrapping_add(4), surface_buf);
            let w3 = self.write_guest_u32(object.wrapping_add(8), 1);
            info!(target: "EAPP_GL", "ordinal_165: object writes: vtable@+0={} buf@+4={} ref@+8={}", w1, w2, w3);

            // Array points to object
            let w4 = self.write_guest_u32(array, object);
            info!(target: "EAPP_GL", "ordinal_165: array[0] write: {}", w4);

            // Container fields at +0x04 (object ptr), +0x54 (count), +0x5c (array)
            // The crash function does: ldr r0, [r4, #4] then stmia r0!, {...}
            // So [container+4] must point to an object whose +4 is the buffer

            // For Vortex, the container at 0x18063ebc is in file-mapped region.
            // We need to check if writes actually succeed.
            let write_addrs = [
                (state_ptr.wrapping_add(4), "+4", object),
                (state_ptr.wrapping_add(0x54), "+54", 1),
                (state_ptr.wrapping_add(0x5c), "+5c", array),
            ];

            for (addr, name, val) in &write_addrs {
                let addr_in_image =
                    *addr >= FILE_VMA_BASE && *addr - FILE_VMA_BASE < self.bus.image_len;
                let addr_in_work =
                    *addr >= WORK_RAM_BASE && *addr - WORK_RAM_BASE < WORK_RAM_SIZE as u32;
                info!(target: "EAPP_GL", "ordinal_165: write {} at {:#010x}: in_image={} in_work={}", 
                      name, addr, addr_in_image, addr_in_work);
                let result = self.write_guest_u32(*addr, *val);
                info!(target: "EAPP_GL", "ordinal_165: write {} result={}", name, result);
            }

            // Verify all writes
            let v_object = self
                .read_guest_u32(state_ptr.wrapping_add(4))
                .unwrap_or(0xdead);
            let v_count = self
                .read_guest_u32(state_ptr.wrapping_add(0x54))
                .unwrap_or(0xdead);
            let v_array = self
                .read_guest_u32(state_ptr.wrapping_add(0x5c))
                .unwrap_or(0xdead);
            let v_buf = self
                .read_guest_u32(object.wrapping_add(4))
                .unwrap_or(0xdead);
            info!(target: "EAPP_GL", "ordinal_165: container AFTER: +4={:#010x} +54={:#010x} +5c={:#010x}", 
                  v_object, v_count, v_array);
            info!(target: "EAPP_GL", "ordinal_165: object buf@+4={:#010x}", v_buf);
            return;
        }

        // Generic case: simple buffer allocation at +4
        if state_ptr != 0 {
            let current_val = self.read_guest_u32(state_ptr.wrapping_add(4)).unwrap_or(0);
            if current_val == 0 {
                let surface_size = 320u32 * 240 * 2;
                if let Some(surface_buf) = self.alloc_surface_buffer(surface_size) {
                    let _ = self.write_guest_u32(state_ptr.wrapping_add(4), surface_buf);
                    info!(target: "EAPP_GL", "ordinal_165: allocated buffer={:#010x} at +4", surface_buf);
                }
            }
        }
    }

    /// Vortex (12345) surface container fix.
    ///
    /// The container at 0x18063ebc is in the file-mapped region with null object pointer.
    /// We redirect by patching all literal pool references to point to a work-RAM substitute.
    fn vortex_preallocate_surfaces(&mut self) {
        // Allocate work-RAM container substitute
        let work_container = self.alloc_zeroed(0x100); // 256 bytes for container + structures
        let surface_size = 320u32 * 240 * 2; // RGB565 framebuffer
        let surface_buf = self.alloc_zeroed(surface_size);
        let object = self.alloc_zeroed(0x40); // 64-byte object
        let array = self.alloc_zeroed(0x4); // 4-byte array
        let state_block = self.alloc_zeroed(0x200); // mutable GL/state block used by Vortex init helpers

        if work_container == 0 || surface_buf == 0 || object == 0 || array == 0 || state_block == 0
        {
            warn!(target: "EAPP_GL", "VORTEX: allocation failed for surface structures");
            return;
        }

        // Wire up structure chain at work_container
        self.write_guest_u32(object.wrapping_add(0), WORK_RAM_BASE + 0x1000); // vtable
        self.write_guest_u32(object.wrapping_add(4), surface_buf); // buffer pointer
        self.write_guest_u32(object.wrapping_add(8), 1); // refcount
        self.write_guest_u32(array, object);

        // Container layout
        self.write_guest_u32(work_container.wrapping_add(0), 0x3f800000);
        self.write_guest_u32(work_container.wrapping_add(4), object);
        self.write_guest_u32(work_container.wrapping_add(0x54), 1);
        self.write_guest_u32(work_container.wrapping_add(0x5c), array);

        info!(
            target: "EAPP_GL",
            "VORTEX: work_container={:#010x} object={:#010x} buffer={:#010x} state_block={:#010x}",
            work_container, object, surface_buf, state_block
        );

        // Store addresses for the Vortex exact-PC shim and ordinal_165 handler.
        self.write_guest_u32(WORK_RAM_BASE + 0xff0, work_container);
        self.write_guest_u32(WORK_RAM_BASE + 0xff4, surface_buf);
        self.write_guest_u32(WORK_RAM_BASE + 0xff8, object);
        self.write_guest_u32(WORK_RAM_BASE + 0xffc, state_block);
    }

    /// Helper to allocate a surface buffer of given size
    fn alloc_surface_buffer(&mut self, size: u32) -> Option<u32> {
        let addr = self.alloc_zeroed(size);
        if addr != 0 {
            Some(addr)
        } else {
            None
        }
    }

    /// Dump guest memory structures for a pointer-backed material handle.
    /// This is a diagnostic called once per unique handle value observed at
    /// ordinal-159, so we can trace the object layout without flooding logs.
    fn live_dump_words_with_float_views(&mut self, label: &str, addr: u32, count: usize) {
        let words = self.read_guest_words(addr, count);
        let rendered = words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let fx = decode_fixed_16_16(*w);
                let f = f32::from_bits(*w);
                format!(
                    "+{:#04x}={:#010x}/fixed({:.4})/float({:.4})",
                    i * 4,
                    w,
                    fx,
                    f
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        info!(target: "EAPP_GL", "{} addr={:#010x} [{}]", label, addr, rendered);
    }

    fn live_dump_pointer_handle_object(&mut self, handle: u32, state_ptr: u32) {
        // Dump the handle object itself (work-RAM pointer)
        let obj_words = self.read_guest_words(handle, 0x40);
        let obj_hex: Vec<String> = obj_words.iter().map(|w| format!("{:#010x}", w)).collect();
        info!(
            target: "EAPP_GL",
            "ptr_handle_object handle={:#010x} addr={:#010x} words=[{}]",
            handle, handle, obj_hex.join(",")
        );

        // Dump state_ptr (up to 0x40 words)
        let state_words = self.read_guest_words(state_ptr, 0x10);
        let state_hex: Vec<String> = state_words.iter().map(|w| format!("{:#010x}", w)).collect();
        info!(
            target: "EAPP_GL",
            "ptr_handle_state handle={:#010x} state_ptr={:#010x} words=[{}]",
            handle, state_ptr, state_hex.join(",")
        );

        // Follow any work-RAM pointers in the object with bounded depth.
        for (i, &word) in obj_words.iter().take(16).enumerate() {
            if (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&word)
                && word != handle
                && word != 0
            {
                let sub = self.read_guest_words(word, 16);
                // Quick check: is it likely pixel data (many nonzero bytes) or
                // a structure (mix of pointers, floats, small ints)?
                let nz = sub.iter().filter(|w| **w != 0).count();
                let sub_hex: Vec<String> = sub.iter().map(|w| format!("{:#010x}", w)).collect();
                info!(
                    target: "EAPP_GL",
                    "ptr_handle_follow handle={:#010x} obj[+{}]={:#010x} nz={}/16 words=[{}]",
                    handle, i * 4, word, nz, sub_hex.join(",")
                );
            }
        }
    }

    fn live_maybe_dump_texgen_stack_locals(&mut self) {
        let sp = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let text_obj = self.read_guest_u32(sp.wrapping_add(0x0c)).unwrap_or(0);
        let text_ptr = self.read_guest_u32(sp.wrapping_add(0x10)).unwrap_or(0);
        if text_obj != 0
            && (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&text_obj)
            && self.dumped_texgen_ptrs.insert(text_obj)
        {
            self.live_dump_words_with_float_views("texgen_text_obj", text_obj, 32);
            let font_obj = self
                .read_guest_u32(text_obj.wrapping_add(0x14))
                .unwrap_or(0);
            let state_obj = self
                .read_guest_u32(text_obj.wrapping_add(0x18))
                .unwrap_or(0);
            if font_obj != 0 {
                self.live_dump_words_with_float_views("texgen_font_obj", font_obj, 48);
                for off in [
                    0x0c_u32, 0x10, 0x5c, 0x60, 0x64, 0x68, 0x6c, 0x70, 0x74, 0x80, 0x84, 0x88,
                ] {
                    let ptr = self.read_guest_u32(font_obj.wrapping_add(off)).unwrap_or(0);
                    if ptr != 0
                        && (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&ptr)
                        && self.dumped_texgen_ptrs.insert(ptr)
                    {
                        self.live_dump_words_with_float_views(
                            &format!("texgen_font_obj_ptr_{:#x}", off),
                            ptr,
                            24,
                        );
                    }
                }
            }
            if state_obj != 0 {
                self.live_dump_words_with_float_views("texgen_text_state_obj", state_obj, 32);
            }
        }
        if text_ptr != 0
            && (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&text_ptr)
            && self.dumped_texgen_ptrs.insert(text_ptr)
        {
            let bytes = self.read_guest_bytes(text_ptr, 32).unwrap_or_default();
            let u16s = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>();
            info!(target: "EAPP_GL", "texgen_text_ptr addr={:#010x} u16={:?}", text_ptr, u16s);
            if let Some(&ch) = u16s.first() {
                let font_obj = self
                    .read_guest_u32(text_obj.wrapping_add(0x14))
                    .unwrap_or(0);
                let table_a = self
                    .read_guest_u32(font_obj.wrapping_add(0x0c))
                    .unwrap_or(0);
                let table_b = self
                    .read_guest_u32(font_obj.wrapping_add(0x10))
                    .unwrap_or(0);
                let lookup_a = if table_a != 0 {
                    self.read_guest_u32(table_a.wrapping_add((ch as u32) * 4))
                        .unwrap_or(0)
                } else {
                    0
                };
                let lookup_b = if table_b != 0 {
                    self.read_guest_u32(table_b.wrapping_add((ch as u32) * 4))
                        .unwrap_or(0)
                } else {
                    0
                };
                info!(
                    target: "EAPP_GL",
                    "texgen_char_lookup char={:#06x} table_a={:#010x} table_b={:#010x}",
                    ch,
                    lookup_a,
                    lookup_b
                );
            }
        }
    }

    fn live_handle_triangle_strip_draw(&mut self, args: [u32; 4]) {
        let first = args[1] as usize;
        let count = args[2] as usize;
        if first != 0 || count < 3 {
            warn!(target: "EAPP_GL", "live_draw skipped: unsupported triangle strip first={} count={}", first, count);
            self.live_finalize_draw(None);
            return;
        }

        if let Some(lg) = self.live_gl.as_mut() {
            if lg.continuous_capture && !lg.frame_active {
                warn!(target: "EAPP_GL", "triangle-strip draw outside active candidate frame; auto-beginning safely");
                lg.note_draw_outside_frame();
            }
        }

        let (
            handle,
            state_ptr,
            bound_tex_name,
            translation,
            modulate,
            pos_def,
            pos_enabled,
            enabled_arrays,
            draw_index,
            material_epoch,
            explicit_uv_def,
            explicit_uv_enabled,
            vertex_color_def,
            vertex_color_enabled,
        ) =
            {
                let lg = match self.live_gl.as_ref() {
                    Some(lg) => lg,
                    None => return,
                };
                let mut enabled_arrays: Vec<u32> = lg.enabled_arrays.iter().copied().collect();
                enabled_arrays.sort_unstable();
                let (explicit_uv_def, explicit_uv_enabled, vertex_color_def, vertex_color_enabled) =
                    Self::live_select_uv_and_color_arrays(lg);
                (
                    lg.current_handle,
                    lg.current_state_ptr,
                    lg.bound_tex_name,
                    lg.translation,
                    lg.modulate,
                    lg.arrays.get(&0).cloned(),
                    lg.enabled_arrays.contains(&0),
                    enabled_arrays,
                    lg.draws.len(),
                    lg.current_material_epoch,
                    explicit_uv_def,
                    explicit_uv_enabled,
                    vertex_color_def,
                    vertex_color_enabled,
                )
            };
        if modulate == Rgba8::rgba(0, 0, 0, 0) {
            if let Some(lg) = self.live_gl.as_mut() {
                lg.note_skipped_draw("zero colour register".to_string());
            }
            self.live_finalize_draw(None);
            return;
        }
        let state_words = self.read_guest_words(state_ptr, 16);
        let positions = match self.live_decode_positions_range(
            &pos_def,
            pos_enabled,
            translation,
            first,
            count,
        ) {
            Some(p) => p,
            None => {
                warn!(target: "EAPP_GL", "triangle-strip draw{} skipped: position array unusable handle={:#x}", draw_index + 1, handle);
                self.live_finalize_draw(None);
                return;
            }
        };
        // Vertex arrays are independent GL state that persists across texture
        // (material) binds. Observed in Texas Hold'em: arrays are defined at
        // one material epoch, then a `159` bind bumps the epoch before the
        // `37 mode=5` draw, so a strict material-epoch guard would reject
        // valid UVs. Use the epoch-agnostic decode, consistent with the
        // ordinal-38 DrawElements path. Tetris uses mode=7 quads (a separate
        // code path), so this does not affect the golden regression.
        let explicit = self.live_decode_uvs_range_any_epoch(
            &explicit_uv_def,
            explicit_uv_enabled,
            first,
            count,
        );
        let vertex_colors = self
            .live_decode_vertex_colors_range(
                &vertex_color_def,
                vertex_color_enabled,
                material_epoch,
                first,
                count,
            )
            .or_else(|| {
                self.live_decode_vertex_colors_range_any_epoch(
                    &vertex_color_def,
                    vertex_color_enabled,
                    first,
                    count,
                )
            });
        // Match the PR runner's default no-modulate policy for ordinary
        // texture draws. The guest register remains available for logging and
        // zero-register suppression; generated font tinting is handled below.
        let tint = Rgba8::rgba(255, 255, 255, 255);
        let mut record = match self.live_gl.as_mut() {
            Some(lg) => lg.rasterize_triangle_strip_record_with_vertex_colors(
                draw_index,
                handle,
                state_ptr,
                bound_tex_name,
                translation,
                &positions,
                explicit.as_deref(),
                tint,
                vertex_colors.as_deref(),
                Some(modulate),
            ),
            None => return,
        };
        record.position_array = pos_def;
        record.uv_array = explicit_uv_def;
        record.enabled_arrays = enabled_arrays;
        record.state_words = state_words;
        if let Some(reason) = record.skipped_reason.as_ref() {
            warn!(target: "EAPP_GL", "draw{} skipped: {}", draw_index + 1, reason);
        } else {
            info!(
                target: "EAPP_GL",
                "draw{} rasterized triangle-strip handle={:#x} vertices={} triangles={} cov={}",
                draw_index + 1,
                handle,
                count,
                count.saturating_sub(2),
                record.coverage
            );
        }
        if self
            .gl_trace_frames
            .is_some_and(|(start, end)| self.frame_counter >= start && self.frame_counter <= end)
        {
            self.live_log_draw_record(&record);
        }
        self.live_finalize_draws(vec![record]);
    }

    /// Ordinal 38: observed as indexed triangle strips in the
    /// Sims/Sudoku/Solitaire engine family and as indexed GL_QUADS batches in
    /// PAC-MAN-family titles. Decode the index stream using the currently
    /// enabled client arrays. Malformed pointers/types fail safely and record
    /// a skipped draw.
    fn live_handle_draw_elements(&mut self, args: [u32; 4]) {
        let mode = args[0];
        let count = args[1] as usize;
        let index_type = args[2];
        let indices_ptr = args[3];
        let indexed_quad_groups = live_gl::indexed_quad_group_count(mode, count);
        let valid_mode = if mode == live_gl::DRAW_MODE_TRIANGLE_STRIP {
            (3..=4096).contains(&count)
        } else {
            indexed_quad_groups.is_some() && count <= 4096
        };
        if !valid_mode || index_type != live_gl::GL_UNSIGNED_SHORT || indices_ptr == 0 {
            warn!(
                target: "EAPP_GL",
                "draw-elements skipped: unsupported mode={} count={} type={:#x} indices={:#010x}",
                mode,
                count,
                index_type,
                indices_ptr
            );
            self.live_finalize_draw(None);
            return;
        }
        let index_bytes = match self.read_guest_bytes(indices_ptr, count.saturating_mul(2)) {
            Some(bytes) if bytes.len() == count * 2 => bytes,
            _ => {
                warn!(
                    target: "EAPP_GL",
                    "draw-elements skipped: invalid index ptr {:#010x} count={}",
                    indices_ptr,
                    count
                );
                self.live_finalize_draw(None);
                return;
            }
        };
        let indices: Vec<usize> = index_bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
            .collect();

        if let Some(lg) = self.live_gl.as_mut() {
            if lg.continuous_capture && !lg.frame_active {
                // This engine family has no observed ordinal-158 begin; the
                // first DrawElements call is the practical frame begin and is
                // followed by ordinal-157 present. Treat it as a normal
                // implicit begin rather than an anomaly.
                if matches!(lg.begin_frame(), live_gl::BeginOutcome::DoubleBegin) {
                    warn!(target: "EAPP_GL", "draw-elements implicit begin hit an active frame");
                }
            }
        }

        let (
            handle,
            state_ptr,
            bound_tex_name,
            translation,
            modulate,
            pos_def,
            pos_enabled,
            enabled_arrays,
            draw_index,
            explicit_uv_def,
            explicit_uv_enabled,
            vertex_color_def,
            vertex_color_enabled,
        ) =
            {
                let lg = match self.live_gl.as_ref() {
                    Some(lg) => lg,
                    None => return,
                };
                let mut enabled_arrays: Vec<u32> = lg.enabled_arrays.iter().copied().collect();
                enabled_arrays.sort_unstable();
                let (explicit_uv_def, explicit_uv_enabled, vertex_color_def, vertex_color_enabled) =
                    Self::live_select_uv_and_color_arrays(lg);
                (
                    lg.current_handle,
                    lg.current_state_ptr,
                    lg.bound_tex_name,
                    lg.translation,
                    lg.modulate,
                    lg.arrays.get(&0).cloned(),
                    lg.enabled_arrays.contains(&0),
                    enabled_arrays,
                    lg.draws.len(),
                    explicit_uv_def,
                    explicit_uv_enabled,
                    vertex_color_def,
                    vertex_color_enabled,
                )
            };
        if modulate == Rgba8::rgba(0, 0, 0, 0) {
            if let Some(lg) = self.live_gl.as_mut() {
                lg.note_skipped_draw("zero colour register".to_string());
            }
            self.live_finalize_draw(None);
            return;
        }
        let state_words = self.read_guest_words(state_ptr, 16);
        let positions = match self.live_decode_positions_indices(
            &pos_def,
            pos_enabled,
            translation,
            &indices,
        ) {
            Some(p) => p,
            None => {
                warn!(
                    target: "EAPP_GL",
                    "draw{} skipped: indexed position array unusable handle={:#x}",
                    draw_index + 1,
                    handle
                );
                self.live_finalize_draw(None);
                return;
            }
        };
        // Ordinal-38 captures show array definitions before the material bind
        // (`137,40,137,40,4,159,149,38`). For this indexed path, accept the
        // enabled UV array regardless of material epoch; stale-epoch protection
        // remains in the ordinal-37 DrawArrays path where it was needed.
        let explicit =
            self.live_decode_uvs_indices(&explicit_uv_def, explicit_uv_enabled, &indices);
        let vertex_colors = self.live_decode_vertex_colors_indices(
            &vertex_color_def,
            vertex_color_enabled,
            &indices,
        );
        let tint = Rgba8::rgba(255, 255, 255, 255);

        if let Some(quad_groups) = indexed_quad_groups {
            let mut records = Vec::with_capacity(quad_groups);
            for quad_idx in 0..quad_groups {
                let base = quad_idx * 4;
                let positions = quad_from_slice(&positions[base..base + 4]);
                let (uvs, has_uv, active_uv_def) = if let Some(explicit) = explicit.as_ref() {
                    (
                        quad_from_slice(&explicit[base..base + 4]),
                        true,
                        explicit_uv_def.clone(),
                    )
                } else {
                    ([(0.0, 0.0); 4], false, explicit_uv_def.clone())
                };
                let mut record = match self.live_gl.as_mut() {
                    Some(lg) => lg.rasterize_draw_with_vertex_colors(
                        draw_index + quad_idx,
                        handle,
                        state_ptr,
                        bound_tex_name,
                        translation,
                        positions,
                        uvs,
                        has_uv,
                        None,
                        tint,
                        false,
                        vertex_colors.as_ref().and_then(|colors| {
                            let base = quad_idx * 4;
                            Some([
                                *colors.get(base)?,
                                *colors.get(base + 1)?,
                                *colors.get(base + 2)?,
                                *colors.get(base + 3)?,
                            ])
                        }),
                        Some(modulate),
                    ),
                    None => return,
                };
                record.position_array = pos_def.clone();
                record.uv_array = active_uv_def;
                record.enabled_arrays = enabled_arrays.clone();
                record.state_words = state_words.clone();
                self.live_log_draw_record(&record);
                records.push(record);
            }
            info!(
                target: "EAPP_GL",
                "draw{} rasterized draw-elements quads handle={:#x} indices={} quads={}",
                draw_index + 1,
                handle,
                count,
                quad_groups
            );
            self.live_finalize_draws(records);
            return;
        }

        let mut record = match self.live_gl.as_mut() {
            Some(lg) => lg.rasterize_triangle_strip_record_with_vertex_colors(
                draw_index,
                handle,
                state_ptr,
                bound_tex_name,
                translation,
                &positions,
                explicit.as_deref(),
                tint,
                vertex_colors.as_deref(),
                Some(modulate),
            ),
            None => return,
        };
        record.position_array = pos_def;
        record.uv_array = explicit_uv_def;
        record.enabled_arrays = enabled_arrays;
        record.state_words = state_words;
        if let Some(reason) = record.skipped_reason.as_ref() {
            warn!(target: "EAPP_GL", "draw{} skipped: {}", draw_index + 1, reason);
        } else {
            info!(
                target: "EAPP_GL",
                "draw{} rasterized draw-elements triangle-strip handle={:#x} indices={} triangles={} cov={}",
                draw_index + 1,
                handle,
                count,
                count.saturating_sub(2),
                record.coverage
            );
        }
        self.live_finalize_draws(vec![record]);
    }

    /// Ordinal 37: observed `DrawArrays(mode=7, first=0, count=4*N)`. Tetris
    /// uses the single-quad case; several sibling titles batch multiple quads.
    /// `mode=5` is also modeled as standard GL ES `GL_TRIANGLE_STRIP` for
    /// Texas Hold'em. Read the current arrays, apply the accumulated
    /// translation, and rasterize the guest primitives in order.
    fn live_handle_draw(&mut self, args: [u32; 4]) {
        let mode = args[0];
        let first = args[1] as usize;
        let count = args[2] as usize;
        if mode == live_gl::DRAW_MODE_TRIANGLE_STRIP {
            self.live_handle_triangle_strip_draw(args);
            return;
        }
        let Some(quad_groups) = live_gl::quad_group_count(mode, first, count) else {
            warn!(
                target: "EAPP_GL",
                "live_draw skipped: unsupported mode={} first={} count={}",
                mode, first, count
            );
            self.live_finalize_draw(None);
            return;
        };

        if let Some(lg) = self.live_gl.as_mut() {
            if lg.continuous_capture && !lg.frame_active {
                warn!(target: "EAPP_GL", "draw outside active candidate frame; auto-beginning safely");
                lg.note_draw_outside_frame();
            }
        }

        let (
            handle,
            state_ptr,
            bound_tex_name,
            translation,
            modulate,
            pos_def,
            pos_enabled,
            enabled_arrays,
            draw_index,
            material_epoch,
            explicit_uv_def,
            explicit_uv_enabled,
            vertex_color_def,
            vertex_color_enabled,
        ) =
            {
                let lg = match self.live_gl.as_ref() {
                    Some(lg) => lg,
                    None => return,
                };
                let mut enabled_arrays: Vec<u32> = lg.enabled_arrays.iter().copied().collect();
                enabled_arrays.sort_unstable();
                let (explicit_uv_def, explicit_uv_enabled, vertex_color_def, vertex_color_enabled) =
                    Self::live_select_uv_and_color_arrays(lg);
                (
                    lg.current_handle,
                    lg.current_state_ptr,
                    lg.bound_tex_name,
                    lg.translation,
                    lg.modulate,
                    lg.arrays.get(&0).cloned(),
                    lg.enabled_arrays.contains(&0),
                    enabled_arrays,
                    lg.draws.len(),
                    lg.current_material_epoch,
                    explicit_uv_def,
                    explicit_uv_enabled,
                    vertex_color_def,
                    vertex_color_enabled,
                )
            };
        if modulate == Rgba8::rgba(0, 0, 0, 0) {
            if let Some(lg) = self.live_gl.as_mut() {
                lg.note_skipped_draw("zero colour register".to_string());
            }
            self.live_finalize_draw(None);
            return;
        }
        let pointer_handle =
            (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&handle);
        // Decode the generated UVs before positions are translated.  Tetris
        // and sibling runtimes keep one base cursor translation on the first
        // glyph, then emit only the per-glyph delta (usually 8 or 16 pixels)
        // while keeping the same font texture bound.  The old HLE accumulated
        // this only for pointer-backed materials, so the ordinary small
        // texture handle used by Tetris collapsed every glyph after the first
        // back to the origin.
        let generated = if quad_groups == 1 {
            self.live_decode_generated_uvs(state_ptr)
        } else {
            None
        };
        let generated_text_material = generated.as_ref().is_some_and(|(uvs, has_generated_uv)| {
            *has_generated_uv
                && self
                    .live_gl
                    .as_ref()
                    .is_some_and(|lg| lg.is_font_atlas_for(handle, uvs))
        });
        let mut effective_translation = if pointer_handle {
            self.live_gl
                .as_ref()
                .and_then(|lg| {
                    (lg.pointer_text_carry_handle == Some(handle)).then_some((
                        lg.pointer_text_carry.0 + translation.0,
                        lg.pointer_text_carry.1 + translation.1,
                    ))
                })
                .unwrap_or(translation)
        } else if generated_text_material {
            self.live_gl
                .as_ref()
                .and_then(|lg| {
                    let same_handle = lg.pointer_text_carry_handle == Some(handle);
                    let small_delta = translation.0.abs() <= 24.0 && translation.1.abs() <= 24.0;
                    (same_handle && small_delta).then_some((
                        lg.pointer_text_carry.0 + translation.0,
                        lg.pointer_text_carry.1 + translation.1,
                    ))
                })
                .unwrap_or(translation)
        } else {
            translation
        };
        let state_words = self.read_guest_words(state_ptr, 16);
        if texgen_verbose_enabled() && pointer_handle {
            self.live_maybe_dump_texgen_stack_locals();
        }

        let vortex_mvp = (self.metadata.title == "12345"
            && fliwheel_var_os("VORTEX_PR3_GL165").is_some())
            .then(|| self.live_gl.as_ref().and_then(|lg| lg.mvp))
            .flatten();
        let decoded_positions = match vortex_mvp {
            Some(mvp) => self.live_decode_positions_range_mvp(
                &pos_def,
                pos_enabled,
                effective_translation,
                mvp,
                first,
                count,
            ),
            None => self.live_decode_positions_range(
                &pos_def,
                pos_enabled,
                effective_translation,
                first,
                count,
            ),
        };
        let mut positions = match decoded_positions {
            Some(p) => p,
            None => {
                let rec = live_gl::LiveDrawRecord {
                    draw_index,
                    handle,
                    state_ptr,
                    bound_tex_name,
                    translation: effective_translation,
                    positions: [(0.0, 0.0); 4],
                    uvs: [(0.0, 0.0); 4],
                    has_uv: false,
                    vertex_colors: None,
                    solid_color: None,
                    tint: Rgba8::rgba(255, 255, 255, 255),
                    used_generated_uvs: false,
                    position_array: pos_def.clone(),
                    uv_array: explicit_uv_def.clone(),
                    enabled_arrays: enabled_arrays.clone(),
                    state_words,
                    bounds: (0.0, 0.0, 0.0, 0.0),
                    coverage: 0,
                    selected_upload: None,
                    inferred_dim: None,
                    skipped_reason: Some("position array not enabled/valid/GL_FIXED".to_string()),
                };
                warn!(
                    target: "EAPP_GL",
                    "draw{} skipped: position array unusable handle={:#x}",
                    draw_index + 1, handle
                );
                self.live_finalize_draws(vec![rec]);
                return;
            }
        };

        // Vertex arrays are independent GL state that persists across texture
        // (material) binds — already established for `mode=5` triangle strips
        // (Texas Hold'em) where arrays are defined at one `159` epoch and the
        // draw runs at the next. The same pattern occurs for `mode=7` quads in
        // the PAC-MAN / Ms. PAC-MAN family: array 1 (UV) is defined and
        // explicitly enabled before a `159` bind bumps the material epoch, so
        // a strict `material_epoch` guard would reject the valid enabled UV
        // array as "stale". After the strict path fails, retry with the
        // epoch-agnostic decoder. Tuned titles (Tetris) always redefine
        // arrays at the current epoch immediately before each draw, so the
        // strict path already succeeds and the fallback never runs — no
        // regression versus the Tetris golden fingerprint.
        let explicit = self
            .live_decode_uvs_range(
                &explicit_uv_def,
                explicit_uv_enabled,
                material_epoch,
                first,
                count,
            )
            .or_else(|| {
                self.live_decode_uvs_range_any_epoch(
                    &explicit_uv_def,
                    explicit_uv_enabled,
                    first,
                    count,
                )
            });
        let vertex_colors = self
            .live_decode_vertex_colors_range(
                &vertex_color_def,
                vertex_color_enabled,
                material_epoch,
                first,
                count,
            )
            .or_else(|| {
                self.live_decode_vertex_colors_range_any_epoch(
                    &vertex_color_def,
                    vertex_color_enabled,
                    first,
                    count,
                )
            });

        // Tetris normally establishes the centered board origin before the
        // matrix draw. After a hard drop or pause/resume re-entry, the guest
        // submits a full 115x223 `matrix_565` quad at (0,0) while retaining
        // the same logical 320x240 scene. The incremental mino draws that
        // follow then inherit that bad origin unless the matrix is re-anchored
        // first. Keep this narrow: the 320x240 background also uses handle
        // 0x13, so handle plus texture dimensions are required evidence.
        if self
            .live_gl
            .as_ref()
            .is_some_and(|lg| lg.game_id == "66666")
            && handle == 0x13
            && quad_groups == 1
            && explicit.as_ref().is_some_and(|uvs| {
                if uvs.len() < 4 {
                    return false;
                }
                let (min_u, min_v, max_u, max_v) = uvs.iter().take(4).fold(
                    (
                        f32::INFINITY,
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                        f32::NEG_INFINITY,
                    ),
                    |acc, (u, v)| (acc.0.min(*u), acc.1.min(*v), acc.2.max(*u), acc.3.max(*v)),
                );
                (max_u - min_u).round() as usize == 115 && (max_v - min_v).round() as usize == 223
            })
        {
            let current_origin = positions
                .iter()
                .fold((f32::INFINITY, f32::INFINITY), |acc, (x, y)| {
                    (acc.0.min(*x), acc.1.min(*y))
                });
            let expected_origin = (102.0, 7.0);
            let correction = (
                expected_origin.0 - current_origin.0,
                expected_origin.1 - current_origin.1,
            );
            if correction.0.abs() > 0.5 || correction.1.abs() > 0.5 {
                for (x, y) in &mut positions {
                    *x += correction.0;
                    *y += correction.1;
                }
                effective_translation.0 += correction.0;
                effective_translation.1 += correction.1;
                info!(
                    target: "EAPP_GL",
                    "tetris_matrix_reanchor current=({:.1},{:.1}) expected=({:.1},{:.1}) delta=({:.1},{:.1})",
                    current_origin.0,
                    current_origin.1,
                    expected_origin.0,
                    expected_origin.1,
                    correction.0,
                    correction.1
                );
            }
        }

        let solid_color = if handle == 0x3 {
            self.live_decode_solid_color(
                &vertex_color_def,
                vertex_color_enabled,
                material_epoch,
            )
        } else {
            None
        };
        let tint = if generated.is_some() {
            self.live_decode_font_tint()
                .unwrap_or(Rgba8::rgba(255, 255, 255, 255))
        } else {
            Rgba8::rgba(255, 255, 255, 255)
        };

        let mut records = Vec::with_capacity(quad_groups);
        for quad_idx in 0..quad_groups {
            let base = quad_idx * 4;
            let positions = quad_from_slice(&positions[base..base + 4]);
            let (uvs, has_uv, used_generated_uvs, active_uv_def) = if quad_groups == 1 {
                if let Some((uvs, true)) = generated {
                    (uvs, true, true, None)
                } else if let Some(explicit) = explicit.as_ref() {
                    (
                        quad_from_slice(&explicit[base..base + 4]),
                        true,
                        false,
                        explicit_uv_def.clone(),
                    )
                } else {
                    ([(0.0, 0.0); 4], false, false, explicit_uv_def.clone())
                }
            } else if let Some(explicit) = explicit.as_ref() {
                (
                    quad_from_slice(&explicit[base..base + 4]),
                    true,
                    false,
                    explicit_uv_def.clone(),
                )
            } else {
                ([(0.0, 0.0); 4], false, false, explicit_uv_def.clone())
            };
            let vertex_colors = vertex_colors.as_ref().and_then(|colors| {
                Some([
                    *colors.get(base)?,
                    *colors.get(base + 1)?,
                    *colors.get(base + 2)?,
                    *colors.get(base + 3)?,
                ])
            });
            let solid_color = if handle == 0x3 {
                solid_color
            } else if has_uv {
                None
            } else {
                solid_color
            };

            let mut record = match self.live_gl.as_mut() {
                Some(lg) => lg.rasterize_draw_with_vertex_colors(
                    draw_index + quad_idx,
                    handle,
                    state_ptr,
                    bound_tex_name,
                    effective_translation,
                    positions,
                    uvs,
                    has_uv,
                    solid_color,
                    tint,
                    used_generated_uvs,
                    vertex_colors,
                    Some(modulate),
                ),
                None => return,
            };
            record.position_array = pos_def.clone();
            record.uv_array = active_uv_def;
            record.enabled_arrays = enabled_arrays.clone();
            record.state_words = state_words.clone();
            self.live_log_draw_record(&record);
            records.push(record);
        }

        if let Some(lg) = self.live_gl.as_mut() {
            if (pointer_handle || generated_text_material) && quad_groups == 1 {
                lg.pointer_text_carry_handle = Some(handle);
                lg.pointer_text_carry = effective_translation;
            } else {
                lg.pointer_text_carry_handle = None;
                lg.pointer_text_carry = (0.0, 0.0);
            }
        }

        self.live_finalize_draws(records);
    }

    fn live_decode_positions_indices(
        &mut self,
        def: &Option<live_gl::LiveArrayDef>,
        enabled: bool,
        translation: (f32, f32),
        indices: &[usize],
    ) -> Option<Vec<(f32, f32)>> {
        let def = def.as_ref()?;
        if !enabled || !def.valid || def.format != live_gl::GL_FIXED || def.component_count < 2 {
            return None;
        }
        let pts = self.read_fixed_array_indices(
            def.guest_ptr,
            def.component_count as usize,
            def.stride as usize,
            indices,
        )?;
        Some(
            pts.into_iter()
                .map(|(x, y)| (x + translation.0, y + translation.1))
                .collect(),
        )
    }

    /// Decode position vertices (array 0, GL_FIXED) and apply the current
    /// translation. Returns None if the array is not usable.
    fn live_decode_positions_range(
        &mut self,
        def: &Option<live_gl::LiveArrayDef>,
        enabled: bool,
        translation: (f32, f32),
        first: usize,
        count: usize,
    ) -> Option<Vec<(f32, f32)>> {
        let def = def.as_ref()?;
        if !enabled || !def.valid || def.format != live_gl::GL_FIXED || def.component_count < 2 {
            debug!(target: "EAPP_GL", "positions_range failed: enabled={} valid={} fmt={:#x} comps={}",
                enabled, def.valid, def.format, def.component_count);
            return None;
        }
        let pts = self.read_fixed_array_range(
            def.guest_ptr,
            def.component_count as usize,
            def.stride as usize,
            first,
            count,
        )?;
        Some(
            pts.into_iter()
                .map(|(x, y)| (x + translation.0, y + translation.1))
                .collect(),
        )
    }

    /// Decode Vortex position vertices and apply the current uploaded MVP.
    /// The direct runner applies the matrix in its vertex path; doing that
    /// after reading all four fixed-point components keeps the software HLE
    /// aligned with the oracle for rotated model-space glyphs.
    fn live_decode_positions_range_mvp(
        &mut self,
        def: &Option<live_gl::LiveArrayDef>,
        enabled: bool,
        translation: (f32, f32),
        mvp: [f32; 16],
        first: usize,
        count: usize,
    ) -> Option<Vec<(f32, f32)>> {
        let def = def.as_ref()?;
        if !enabled || !def.valid || def.format != live_gl::GL_FIXED || def.component_count < 2 {
            return None;
        }
        let components = def.component_count as usize;
        let tight_stride = components.checked_mul(4)?;
        let stride = if def.stride == 0 {
            tight_stride
        } else {
            (def.stride as usize).max(tight_stride)
        };
        let mut points = Vec::with_capacity(count);
        for vertex in first..first.checked_add(count)? {
            let offset = vertex.checked_mul(stride)? as u32;
            let bytes = self.read_guest_bytes(def.guest_ptr.wrapping_add(offset), tight_stride)?;
            let read = |component: usize| {
                let at = component * 4;
                decode_fixed_16_16(u32::from_le_bytes([
                    bytes[at],
                    bytes[at + 1],
                    bytes[at + 2],
                    bytes[at + 3],
                ]))
            };
            let x = read(0) + translation.0;
            let y = read(1) + translation.1;
            let z = (components >= 3).then(|| read(2)).unwrap_or(0.0);
            let w = (components >= 4).then(|| read(3)).unwrap_or(1.0);
            points.push(project_mvp_position(mvp, x, y, z, w));
        }
        Some(points)
    }

    fn live_decode_generated_uvs(&mut self, state_ptr: u32) -> Option<([(f32, f32); 4], bool)> {
        if state_ptr == 0 || self.read_guest_u32(state_ptr)? != 0x1802_3e24 {
            return None;
        }
        let mut out = [(0.0f32, 0.0f32); 4];
        for (idx, slot) in out.iter_mut().enumerate() {
            let base = state_ptr.wrapping_add(0x48 + (idx as u32) * 8);
            let u = f32::from_bits(self.read_guest_u32(base)?);
            let v = f32::from_bits(self.read_guest_u32(base.wrapping_add(4))?);
            if !u.is_finite() || !v.is_finite() {
                return None;
            }
            *slot = (u, v);
        }
        let (min_u, min_v, max_u, max_v) = out.iter().fold(
            (
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ),
            |acc, (u, v)| (acc.0.min(*u), acc.1.min(*v), acc.2.max(*u), acc.3.max(*v)),
        );
        if max_u > min_u && max_v > min_v {
            return Some((out, true));
        }
        self.live_decode_generated_text_uvs(state_ptr)
    }

    fn live_decode_generated_text_uvs(
        &mut self,
        state_ptr: u32,
    ) -> Option<([(f32, f32); 4], bool)> {
        let sp = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let text_obj = match self.read_guest_u32(sp.wrapping_add(0x0c)) {
            Some(ptr) if self.live_is_texgen_text_object(ptr, Some(state_ptr)) => ptr,
            _ => self.live_find_texgen_text_object()?,
        };
        // Prefer the per-frame recorded char sequence captured at the guest
        // `text_push_char` callsite. This is the authoritative per-glyph
        // selector for the scalar (register-computed) formatter path, which
        // never writes a UTF-16 buffer. Falls back to the cursor scan when no
        // char has been recorded (e.g. curser-advance UTF-16 path).
        let recorded_ch = self
            .live_gl
            .as_mut()
            .and_then(|lg| lg.take_text_char_for_draw(text_obj));
        // `text_ptr` is only used by the cursor-scan fallback; keep a
        // sentinel for logging when the recorded path is taken.
        let text_ptr = recorded_ch.map(|_| 0u32).or_else(|| {
            self.live_find_texgen_text_cursor(text_obj).or_else(|| {
                self.read_guest_u32(sp.wrapping_add(0x10))
                    .filter(|ptr| *ptr != 0)
            })
        })?;
        let font_obj = match self.read_guest_u32(text_obj.wrapping_add(0x14)) {
            Some(ptr) if ptr != 0 => ptr,
            _ => {
                if texgen_verbose_enabled() {
                    info!(
                        target: "EAPP_GL",
                        "texgen_generated_uvs_fail text_obj={:#010x} text_ptr={:#010x} state_ptr={:#010x} reason=missing_font_obj",
                        text_obj,
                        text_ptr,
                        state_ptr
                    );
                }
                return None;
            }
        };
        let table_a = match self.read_guest_u32(font_obj.wrapping_add(0x0c)) {
            Some(ptr) if ptr != 0 => ptr,
            _ => {
                if texgen_verbose_enabled() {
                    info!(
                        target: "EAPP_GL",
                        "texgen_generated_uvs_fail text_obj={:#010x} text_ptr={:#010x} font_obj={:#010x} state_ptr={:#010x} reason=missing_table_a",
                        text_obj,
                        text_ptr,
                        font_obj,
                        state_ptr
                    );
                }
                return None;
            }
        };
        let ch = if let Some(ch) = recorded_ch {
            ch
        } else {
            match (
                self.read_guest_u8(text_ptr),
                self.read_guest_u8(text_ptr.wrapping_add(1)),
            ) {
                (Some(lo), Some(hi)) => u16::from_le_bytes([lo, hi]) as u32,
                _ => {
                    if texgen_verbose_enabled() {
                        info!(
                            target: "EAPP_GL",
                            "texgen_generated_uvs_fail text_obj={:#010x} text_ptr={:#010x} font_obj={:#010x} table_a={:#010x} state_ptr={:#010x} reason=missing_text_bytes",
                            text_obj,
                            text_ptr,
                            font_obj,
                            table_a,
                            state_ptr
                        );
                    }
                    return None;
                }
            }
        };
        if ch == 0 || !Self::is_plausible_texgen_char(ch as u16) {
            if texgen_verbose_enabled() {
                info!(
                    target: "EAPP_GL",
                    "texgen_generated_uvs_fail text_obj={:#010x} text_ptr={:#010x} font_obj={:#010x} table_a={:#010x} ch={:#06x} state_ptr={:#010x} reason=unsupported_text_char",
                    text_obj,
                    text_ptr,
                    font_obj,
                    table_a,
                    ch,
                    state_ptr
                );
            }
            return None;
        }
        let glyph_index = match self.read_guest_u32(table_a.wrapping_add(ch.wrapping_mul(4))) {
            Some(idx) => idx,
            None => {
                if texgen_verbose_enabled() {
                    info!(
                        target: "EAPP_GL",
                        "texgen_generated_uvs_fail text_obj={:#010x} text_ptr={:#010x} font_obj={:#010x} table_a={:#010x} ch={:#06x} state_ptr={:#010x} reason=missing_glyph_index",
                        text_obj,
                        text_ptr,
                        font_obj,
                        table_a,
                        ch,
                        state_ptr
                    );
                }
                return None;
            }
        };
        let cell_w = f32::from_bits(self.read_guest_u32(state_ptr.wrapping_add(0x28))?);
        let cell_h = f32::from_bits(self.read_guest_u32(state_ptr.wrapping_add(0x1c))?);
        if !cell_w.is_finite() || !cell_h.is_finite() || cell_w <= 0.0 || cell_h <= 0.0 {
            if texgen_verbose_enabled() {
                info!(
                    target: "EAPP_GL",
                    "texgen_generated_uvs_fail text_obj={:#010x} text_ptr={:#010x} font_obj={:#010x} table_a={:#010x} ch={:#06x} glyph_index={} state_ptr={:#010x} cell_w={:.3} cell_h={:.3} reason=bad_cell_metrics",
                    text_obj,
                    text_ptr,
                    font_obj,
                    table_a,
                    ch,
                    glyph_index,
                    state_ptr,
                    cell_w,
                    cell_h
                );
            }
            return None;
        }
        let columns = self.live_guess_font_columns(font_obj).unwrap_or(98);
        if columns == 0 {
            if texgen_verbose_enabled() {
                info!(
                    target: "EAPP_GL",
                    "texgen_generated_uvs_fail text_obj={:#010x} text_ptr={:#010x} font_obj={:#010x} table_a={:#010x} ch={:#06x} glyph_index={} state_ptr={:#010x} cell_w={:.3} cell_h={:.3} reason=no_columns",
                    text_obj,
                    text_ptr,
                    font_obj,
                    table_a,
                    ch,
                    glyph_index,
                    state_ptr,
                    cell_w,
                    cell_h
                );
            }
            return None;
        }
        let col = (glyph_index % columns) as f32;
        let row = (glyph_index / columns) as f32;
        let left = col * cell_w + 0.5;
        let top = row * cell_h + 0.5;
        let right = (col + 1.0) * cell_w - 0.5;
        let bottom = (row + 1.0) * cell_h - 0.5;
        let uvs = [(left, bottom), (left, top), (right, top), (right, bottom)];
        if texgen_verbose_enabled() {
            info!(
                target: "EAPP_GL",
                "texgen_generated_uvs text_obj={:#010x} text_ptr={:#010x} font_obj={:#010x} table_a={:#010x} ch={:#06x} glyph_index={} state_ptr={:#010x} columns={} cell_w={:.3} cell_h={:.3} uvs=[({:.1},{:.1}),({:.1},{:.1}),({:.1},{:.1}),({:.1},{:.1})]",
                text_obj,
                text_ptr,
                font_obj,
                table_a,
                ch,
                glyph_index,
                state_ptr,
                columns,
                cell_w,
                cell_h,
                uvs[0].0,
                uvs[0].1,
                uvs[1].0,
                uvs[1].1,
                uvs[2].0,
                uvs[2].1,
                uvs[3].0,
                uvs[3].1,
            );
        }
        Some((uvs, true))
    }

    fn live_is_texgen_text_object(&mut self, ptr: u32, expected_state_ptr: Option<u32>) -> bool {
        if ptr == 0 || !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&ptr) {
            return false;
        }
        let Some(font_ptr) = self.read_guest_u32(ptr.wrapping_add(0x14)) else {
            return false;
        };
        let Some(state_ptr) = self.read_guest_u32(ptr.wrapping_add(0x18)) else {
            return false;
        };
        if font_ptr == 0
            || state_ptr == 0
            || !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&font_ptr)
            || !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&state_ptr)
        {
            return false;
        }
        if expected_state_ptr.is_some_and(|expected| expected != state_ptr) {
            return false;
        }
        if self.read_guest_u32(state_ptr).unwrap_or(0) != 0x1802_3e24 {
            return false;
        }
        matches!(
            self.read_guest_u32(font_ptr.wrapping_add(0x0c)),
            Some(table_ptr)
                if table_ptr != 0
                    && (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&table_ptr)
        )
    }

    fn live_find_texgen_text_object(&mut self) -> Option<u32> {
        let sp = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let mut best: Option<(u32, usize)> = None;
        for off in [
            0x0c_u32, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c, 0x40,
            0x44, 0x48, 0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60, 0x64, 0x68, 0x6c, 0x70, 0x74, 0x78,
            0x7c,
        ] {
            let Some(ptr) = self.read_guest_u32(sp.wrapping_add(off)) else {
                continue;
            };
            if ptr == 0 || !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&ptr) {
                continue;
            }
            let Some(font_ptr) = self.read_guest_u32(ptr.wrapping_add(0x14)) else {
                continue;
            };
            let Some(state_ptr) = self.read_guest_u32(ptr.wrapping_add(0x18)) else {
                continue;
            };
            if font_ptr == 0
                || state_ptr == 0
                || !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&font_ptr)
                || !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&state_ptr)
                || self.read_guest_u32(state_ptr).unwrap_or(0) != 0x1802_3e24
            {
                continue;
            }
            let mut score = 0usize;
            for sub_off in [
                0x0c_u32, 0x10, 0x5c, 0x60, 0x64, 0x68, 0x6c, 0x70, 0x74, 0x80, 0x84, 0x88,
            ] {
                let Some(sub_ptr) = self.read_guest_u32(font_ptr.wrapping_add(sub_off)) else {
                    continue;
                };
                if (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&sub_ptr)
                    && sub_ptr != ptr
                {
                    score += 1;
                }
            }
            if best
                .as_ref()
                .map_or(true, |(_, best_score)| score > *best_score)
            {
                best = Some((ptr, score));
            }
        }
        if let Some((ptr, score)) = best {
            if texgen_verbose_enabled() {
                info!(
                    target: "EAPP_GL",
                    "texgen_text_obj_candidate ptr={:#010x} score={}",
                    ptr,
                    score
                );
            }
            Some(ptr)
        } else {
            None
        }
    }

    fn live_find_texgen_text_cursor(&mut self, text_obj: u32) -> Option<u32> {
        let sp = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let mut best: Option<(&'static str, u32, u32, u32, usize, usize)> = None;
        let mut candidates: Vec<(&'static str, u32, u32, u32)> = Vec::new();

        for off in [
            0x10_u32, 0x14, 0x18, 0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c, 0x40, 0x44,
            0x48, 0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60, 0x64, 0x68, 0x6c, 0x70, 0x74, 0x78, 0x7c,
            0x80, 0x84, 0x88, 0x8c, 0x90, 0x94, 0x98, 0x9c, 0xa0, 0xa4, 0xa8, 0xac, 0xb0, 0xb4,
            0xb8, 0xbc, 0xc0, 0xc4, 0xc8, 0xcc, 0xd0, 0xd4, 0xd8, 0xdc, 0xe0, 0xe4, 0xe8, 0xec,
            0xf0, 0xf4, 0xf8, 0xfc,
        ] {
            if let Some(ptr) = self.read_guest_u32(sp.wrapping_add(off)) {
                candidates.push(("stack", sp, off, ptr));
            }
        }

        for off in (0_u32..0x400).step_by(4) {
            if let Some(ptr) = self.read_guest_u32(text_obj.wrapping_add(off)) {
                candidates.push(("text_obj", text_obj, off, ptr));
            }
        }

        for (source, source_base, off, ptr) in candidates {
            let inline = source_base.wrapping_add(off);
            for seed in [ptr, inline] {
                if seed == 0
                    || seed == text_obj
                    || !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&seed)
                {
                    continue;
                }
                for delta in [0_u32, 2, 4, 6, 8, 12, 16] {
                    let cursor = seed.wrapping_add(delta);
                    if cursor == 0
                        || cursor == text_obj
                        || cursor % 2 != 0
                        || !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&cursor)
                    {
                        continue;
                    }
                    let Some(bytes) = self.read_guest_bytes(cursor, 32) else {
                        continue;
                    };
                    let u16s = bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect::<Vec<_>>();
                    let mut score = 0usize;
                    let mut printable = 0usize;
                    let first_is_plausible = u16s
                        .first()
                        .is_some_and(|ch| *ch != 0 && Self::is_plausible_texgen_char(*ch));
                    for &ch in &u16s {
                        if ch == 0 {
                            break;
                        }
                        if Self::is_plausible_texgen_char(ch) {
                            printable += 1;
                            score += if ch <= 0x007f { 2 } else { 1 };
                        } else {
                            score = score.saturating_sub(2);
                        }
                    }
                    if !first_is_plausible || printable < 2 {
                        if texgen_verbose_enabled() && self.dumped_texgen_ptrs.insert(cursor) {
                            self.live_dump_words_with_float_views(
                                "texgen_cursor_probe",
                                cursor,
                                16,
                            );
                        }
                        continue;
                    }
                    if texgen_verbose_enabled() && self.dumped_texgen_ptrs.insert(cursor) {
                        self.live_dump_words_with_float_views("texgen_cursor_probe", cursor, 16);
                    }
                    if best
                        .as_ref()
                        .map_or(true, |(_, _, _, _, best_score, _)| score > *best_score)
                    {
                        best = Some((source, source_base, off, cursor, score, printable));
                    }
                }
            }
        }

        if let Some((source, source_base, off, cursor, score, printable)) = best {
            if texgen_verbose_enabled() {
                info!(
                    target: "EAPP_GL",
                    "texgen_text_cursor_candidate text_obj={:#010x} source={} source_base={:#010x} off={:#x} ptr={:#010x} score={} printable={}",
                    text_obj,
                    source,
                    source_base,
                    off,
                    cursor,
                    score,
                    printable
                );
            }
            Some(cursor)
        } else {
            None
        }
    }

    fn is_plausible_texgen_char(ch: u16) -> bool {
        matches!(
            ch,
            0x0020..=0x007e // ASCII printable
                | 0x00a0..=0x00ff
                | 0x0390..=0x03ff // Greek uppercase/lowercase used by the menu text
        )
    }

    fn live_guess_font_columns(&mut self, font_obj: u32) -> Option<u32> {
        let mut counts: HashMap<u32, usize> = HashMap::new();
        for off in [0x60_u32, 0x64, 0x68, 0x6c, 0x70, 0x74, 0x80, 0x84, 0x88] {
            let ptr = self.read_guest_u32(font_obj.wrapping_add(off))?;
            if ptr == 0 || !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&ptr) {
                continue;
            }
            let words = self.read_guest_words(ptr, 24);
            for &word in &words {
                if (8..=256).contains(&word) {
                    *counts.entry(word).or_default() += 1;
                }
            }
        }
        counts
            .into_iter()
            .filter(|(value, hits)| *value >= 32 && *hits >= 2)
            .max_by_key(|(value, hits)| (*hits, *value))
            .map(|(value, _)| value)
    }

    fn live_decode_font_tint(&mut self) -> Option<Rgba8> {
        let sp = self.cpu.reg_get(self.cpu.mode(), reg::SP);
        let text_obj = self.read_guest_u32(sp.wrapping_add(0x0c))?;
        let font_obj = self.read_guest_u32(text_obj.wrapping_add(0x14))?;
        let to_u8 =
            |word: u32| -> u8 { (f32::from_bits(word).clamp(0.0, 1.0) * 255.0).round() as u8 };
        Some(Rgba8::rgba(
            to_u8(self.read_guest_u32(font_obj.wrapping_add(0x18))?),
            to_u8(self.read_guest_u32(font_obj.wrapping_add(0x1c))?),
            to_u8(self.read_guest_u32(font_obj.wrapping_add(0x20))?),
            to_u8(self.read_guest_u32(font_obj.wrapping_add(0x24))?),
        ))
    }

    /// Decode a GL_FIXED 2-component UV array. Tetris also binds 4-component
    /// arrays in slot 1 for color/tint-like data; those are not texture
    /// coordinates. Epoch matching avoids reusing stale client arrays after a
    /// later material bind that only redefines array 0.
    fn live_decode_uvs_range(
        &mut self,
        def: &Option<live_gl::LiveArrayDef>,
        enabled: bool,
        material_epoch: u64,
        first: usize,
        count: usize,
    ) -> Option<Vec<(f32, f32)>> {
        let def = def.as_ref()?;
        if !enabled
            || !def.valid
            || def.material_epoch != material_epoch
            || def.format != live_gl::GL_FIXED
            || def.component_count != 2
        {
            return None;
        }
        self.read_fixed_array_range(
            def.guest_ptr,
            def.component_count as usize,
            def.stride as usize,
            first,
            count,
        )
    }

    fn live_decode_uvs_range_any_epoch(
        &mut self,
        def: &Option<live_gl::LiveArrayDef>,
        enabled: bool,
        first: usize,
        count: usize,
    ) -> Option<Vec<(f32, f32)>> {
        let def = def.as_ref()?;
        if !enabled || !def.valid || def.format != live_gl::GL_FIXED || def.component_count != 2 {
            return None;
        }
        self.read_fixed_array_range(
            def.guest_ptr,
            def.component_count as usize,
            def.stride as usize,
            first,
            count,
        )
    }

    fn live_decode_uvs_indices(
        &mut self,
        def: &Option<live_gl::LiveArrayDef>,
        enabled: bool,
        indices: &[usize],
    ) -> Option<Vec<(f32, f32)>> {
        let def = def.as_ref()?;
        if !enabled || !def.valid || def.format != live_gl::GL_FIXED || def.component_count != 2 {
            return None;
        }
        self.read_fixed_array_indices(
            def.guest_ptr,
            def.component_count as usize,
            def.stride as usize,
            indices,
        )
    }

    fn live_decode_vertex_colors_range(
        &mut self,
        def: &Option<live_gl::LiveArrayDef>,
        enabled: bool,
        material_epoch: u64,
        first: usize,
        count: usize,
    ) -> Option<Vec<[f32; 4]>> {
        self.live_decode_vertex_colors_range_impl(
            def,
            enabled,
            Some(material_epoch),
            first,
            count,
        )
    }

    fn live_decode_vertex_colors_range_any_epoch(
        &mut self,
        def: &Option<live_gl::LiveArrayDef>,
        enabled: bool,
        first: usize,
        count: usize,
    ) -> Option<Vec<[f32; 4]>> {
        self.live_decode_vertex_colors_range_impl(def, enabled, None, first, count)
    }

    fn live_decode_vertex_colors_range_impl(
        &mut self,
        def: &Option<live_gl::LiveArrayDef>,
        enabled: bool,
        material_epoch: Option<u64>,
        first: usize,
        count: usize,
    ) -> Option<Vec<[f32; 4]>> {
        let def = def.as_ref()?;
        if !enabled
            || !def.valid
            || material_epoch.is_some_and(|epoch| def.material_epoch != epoch)
            || def.format != live_gl::GL_FIXED
            || def.component_count < 3
        {
            return None;
        }
        let components = def.component_count.min(4) as usize;
        let tight_stride = def.component_count as usize * 4;
        let stride = if def.stride == 0 {
            tight_stride
        } else {
            def.stride.max(tight_stride as u32) as usize
        };
        let mut colors = Vec::with_capacity(count);
        for vertex in 0..count {
            let offset = first
                .checked_add(vertex)
                .and_then(|index| index.checked_mul(stride))?;
            let base = def.guest_ptr.wrapping_add(offset as u32);
            let mut color = [1.0f32; 4];
            for (component, slot) in color.iter_mut().enumerate().take(components) {
                *slot = decode_fixed_16_16(self.read_guest_u32(
                    base.wrapping_add((component * 4) as u32),
                )?)
                .clamp(0.0, 1.0);
            }
            colors.push(color);
        }
        Some(colors)
    }

    fn live_decode_vertex_colors_indices(
        &mut self,
        def: &Option<live_gl::LiveArrayDef>,
        enabled: bool,
        indices: &[usize],
    ) -> Option<Vec<[f32; 4]>> {
        let def = def.as_ref()?;
        if !enabled
            || !def.valid
            || def.format != live_gl::GL_FIXED
            || def.component_count < 3
        {
            return None;
        }
        let components = def.component_count.min(4) as usize;
        let tight_stride = def.component_count as usize * 4;
        let stride = if def.stride == 0 {
            tight_stride
        } else {
            def.stride.max(tight_stride as u32) as usize
        };
        let mut colors = Vec::with_capacity(indices.len());
        for &index in indices {
            let offset = index.checked_mul(stride)?;
            let base = def.guest_ptr.wrapping_add(offset as u32);
            let mut color = [1.0f32; 4];
            for (component, slot) in color.iter_mut().enumerate().take(components) {
                *slot = decode_fixed_16_16(self.read_guest_u32(
                    base.wrapping_add((component * 4) as u32),
                )?)
                .clamp(0.0, 1.0);
            }
            colors.push(color);
        }
        Some(colors)
    }

    /// Decode a 4-component GL_FIXED color/tint array as a conservative solid
    /// color. Tetris uses this shape for handle-3 fade/fill quads that do not
    /// provide a 2-component texcoord array. We average the four vertex colors;
    /// observed startup quads use uniform values.
    fn live_decode_solid_color(
        &mut self,
        def: &Option<live_gl::LiveArrayDef>,
        enabled: bool,
        material_epoch: u64,
    ) -> Option<Rgba8> {
        let def = def.as_ref()?;
        if !enabled
            || !def.valid
            || def.material_epoch != material_epoch
            || def.format != live_gl::GL_FIXED
            || def.component_count < 3
        {
            return None;
        }
        let stride = if def.stride == 0 {
            def.component_count as usize * 4
        } else {
            def.stride as usize
        };
        let mut acc = [0.0f32; 4];
        let components = def.component_count.min(4) as usize;
        for vertex in 0..4usize {
            let base = def.guest_ptr.wrapping_add((vertex * stride) as u32);
            for (component, slot) in acc.iter_mut().enumerate().take(components) {
                let word = self.read_guest_u32(base.wrapping_add((component * 4) as u32))?;
                *slot += decode_fixed_16_16(word).clamp(0.0, 1.0);
            }
        }
        if components < 4 {
            acc[3] = 4.0;
        }
        let to_u8 = |v: f32| ((v / 4.0) * 255.0).round().clamp(0.0, 255.0) as u8;
        Some(Rgba8::rgba(
            to_u8(acc[0]),
            to_u8(acc[1]),
            to_u8(acc[2]),
            to_u8(acc[3]),
        ))
    }

    fn live_log_draw_record(&mut self, record: &live_gl::LiveDrawRecord) {
        let handle = record.handle;
        let draw_index = record.draw_index;
        let quad_dump_frame = fliwheel_var_os("GL_QUAD_DUMP_FRAME")
            .and_then(|value| value.to_string_lossy().parse::<u64>().ok());
        let quad_dump_handle = fliwheel_var_os("GL_QUAD_DUMP_HANDLE").and_then(|value| {
            let value = value.to_string_lossy();
            if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                value.parse::<u32>().ok()
            }
        });
        if record.has_uv
            && quad_dump_frame == Some(self.frame_counter)
            && quad_dump_handle.map_or(true, |wanted| wanted == handle)
        {
            let (upload, fingerprint) = record
                .selected_upload
                .map(|index| {
                    (
                        Some(index),
                        self.live_gl
                            .as_ref()
                            .and_then(|live| live.upload_pixel_fingerprint(index)),
                    )
                })
                .unwrap_or((None, None));
            info!(
                target: "EAPP_GL",
                "quad_dump frame={} draw={} handle={:#x} bound_tex={:?} upload={:?} fingerprint={:?} pos={:?} uv={:?} vertex_colors={:?}",
                self.frame_counter,
                draw_index + 1,
                handle,
                record.bound_tex_name,
                upload,
                fingerprint.map(|value| format!("{value:#018x}")),
                record.positions,
                record.uvs,
                record.vertex_colors
            );
        }
        if let Some(reason) = record.skipped_reason.clone() {
            if let Some(lg) = self.live_gl.as_mut() {
                lg.note_skipped_draw(reason.clone());
            }
            let key = (handle, reason.clone());
            if self.skipped_draw_warnings.insert(key) {
                warn!(
                    target: "EAPP_GL",
                    "draw{} skipped: {} handle={:#x} (first occurrence; further same-reason skips suppressed)",
                    draw_index + 1,
                    reason,
                    handle
                );
            }
        } else if let Some(sel) = record.selected_upload {
            info!(
                target: "EAPP_GL",
                "draw{} rasterized handle={:#x} inferred_upload={} dim={:?} bounds=({:.1},{:.1})-({:.1},{:.1}) cov={}",
                draw_index + 1,
                handle,
                sel,
                record.inferred_dim,
                record.bounds.0,
                record.bounds.1,
                record.bounds.2,
                record.bounds.3,
                record.coverage
            );
        } else if let Some(color) = record.solid_color {
            info!(
                target: "EAPP_GL",
                "draw{} rasterized solid handle={:#x} color=rgba({},{},{},{}) bounds=({:.1},{:.1})-({:.1},{:.1}) cov={}",
                draw_index + 1,
                handle,
                color.r,
                color.g,
                color.b,
                color.a,
                record.bounds.0,
                record.bounds.1,
                record.bounds.2,
                record.bounds.3,
                record.coverage
            );
        }
    }

    /// Reset normal per-draw translation, increment the draw counter, and capture the
    /// first complete candidate frame (after the known steady-state four
    /// ordinal-37 draws) unless continuous capture is enabled.
    fn live_finalize_draw(&mut self, record: Option<live_gl::LiveDrawRecord>) {
        self.live_finalize_draws(record.into_iter().collect());
    }

    fn live_finalize_draws(&mut self, records: Vec<live_gl::LiveDrawRecord>) {
        let should_capture;
        if let Some(lg) = self.live_gl.as_mut() {
            let increment = records.len().max(1);
            if lg.game_id == "66666" {
                if let Some(matrix) = records
                    .iter()
                    .find(|record| record.handle == 0x13 && record.inferred_dim == Some((115, 223)))
                {
                    lg.frame_base_translation = matrix.translation;
                    lg.board_base_translation_valid = true;
                }
            }
            let preserve_translation = lg.use_incremental_translation
                && records
                    .iter()
                    .all(|record| matches!(record.handle, 0x13 | 0x19));
            lg.draws.extend(records);
            if !preserve_translation {
                lg.translation = (0.0, 0.0);
            }
            lg.draw_count_in_frame += increment;
            if lg.continuous_capture {
                return;
            }
            let four_draws = lg.draw_count_in_frame == 4;
            if !four_draws {
                return;
            }
            let current_handles: Vec<u32> = lg.draws.iter().map(|d| d.handle).collect();
            let steady = matches!(&lg.prev_draw_handles, Some(prev) if *prev == current_handles);
            lg.prev_draw_handles = Some(current_handles);
            should_capture = steady && !lg.captured_first_frame;
        } else {
            return;
        }
        if should_capture {
            self.live_capture_frame();
        }
    }

    /// Gate A: write internal + presented PPMs, print hashes, and run the
    /// structural comparison against the offline replay. Gate B: copy the
    /// presented buffer to the desktop render state when `FLIWHEEL_GL_GATE_B=1`.
    fn live_capture_frame(&mut self) {
        let gate_b;
        {
            let lg = match self.live_gl.as_mut() {
                Some(lg) => lg,
                None => return,
            };
            lg.candidate_frames += 1;
            lg.captured_first_frame = true;
            let internal = lg.internal_hash();
            let presented = lg.presented_hash();
            let wrote = lg.write_diagnostic_ppms(
                std::path::Path::new(&format!("/tmp/{}_live_gl_hle_internal.ppm", lg.game_id)),
                std::path::Path::new(&format!("/tmp/{}_live_gl_hle_presented.ppm", lg.game_id)),
            );
            lg.presented = Some(lg.present());
            info!(
                target: "EAPP_GL",
                "live_capture frame={} draws={} internal_hash={:#018x} presented_hash={:#018x} present_vflip={} wrote_ppms={}",
                lg.last_frame_counter, lg.draw_count_in_frame, internal, presented, lg.present_vflip, wrote
            );
            gate_b = lg.gate_b;
        }

        self.live_compare_to_offline();

        // Gate B: present to the desktop window only when explicitly enabled.
        if gate_b {
            self.live_present_to_window();
        }
    }

    /// Bounded diagnostics for completed continuous frames (first 120 by
    /// default). Reports candidate begin/end ordering, hashes, repeated-frame
    /// count, skipped draws, and whether the frame was presented or discarded.
    fn live_log_completed_frame(&mut self, frame: &live_gl::CompletedFrame, presented: bool) {
        let Some(lg) = self.live_gl.as_ref() else {
            return;
        };
        if frame.index as usize > lg.diagnostics_budget {
            if lg.first_changed_frame == Some(frame.index) {
                info!(
                    target: "EAPP_GL",
                    "frame_hash_changed first_change_frame={} presented_hash={:#018x}",
                    frame.index,
                    frame.presented_hash
                );
            }
            return;
        }
        let begin_seq = lg
            .ordinal_trace
            .iter()
            .position(|(ord, _)| *ord == lg.candidate_begin_ordinal)
            .map(|idx| idx + 1);
        let present_seq = lg
            .ordinal_trace
            .iter()
            .rposition(|(ord, _)| *ord == lg.candidate_present_ordinal)
            .map(|idx| idx + 1);
        let signature = frame
            .handle_signature
            .iter()
            .map(|h| format!("{:#x}", h))
            .collect::<Vec<_>>()
            .join(",");
        info!(
            target: "EAPP_GL",
            "frame_diag idx={} begin={}@{:?} end={}@{:?} draws={} sig=[{}] internal={:#018x} presented={:#018x} repeated={} skipped={} unique_hashes={} status={}",
            frame.index,
            lg.candidate_begin_ordinal,
            begin_seq,
            lg.candidate_present_ordinal,
            present_seq,
            frame.draw_count,
            signature,
            frame.internal_hash,
            frame.presented_hash,
            lg.repeated_presented_count,
            frame.skipped_draws,
            lg.unique_presented_hashes.len(),
            if presented { "presented" } else { "discarded" }
        );
        if !lg.frame_anomalies.is_empty() && frame.index as usize <= 12 {
            info!(
                target: "EAPP_GL",
                "frame_diag anomalies_so_far={} latest={}",
                lg.frame_anomalies.len(),
                lg.frame_anomalies.last().unwrap()
            );
        }
        if lg.first_changed_frame == Some(frame.index) {
            info!(
                target: "EAPP_GL",
                "frame_hash_changed first_change_frame={} presented_hash={:#018x}",
                frame.index,
                frame.presented_hash
            );
        }
    }

    /// Emit a bounded, detailed draw report the first time a completed-frame
    /// signature appears. This is for visual-accuracy triage, not rendering.
    fn live_log_signature_detail(&mut self, frame: &live_gl::CompletedFrame) {
        let key = frame
            .handle_signature
            .iter()
            .map(|h| format!("{:#x}", h))
            .collect::<Vec<_>>()
            .join(",");
        let key = format!("draws={} [{}]", frame.draw_count, key);
        if !self.startup_signature_reports.insert(key.clone()) {
            return;
        }
        let Some(lg) = self.live_gl.as_ref() else {
            return;
        };
        info!(
            target: "EAPP_GL",
            "frame_signature_detail guest_frame={} completed_idx={} {} internal={:#018x} presented={:#018x}",
            self.frame_counter,
            frame.index,
            key,
            frame.internal_hash,
            frame.presented_hash
        );
        for draw in &lg.draws {
            let pos = array_summary(draw.position_array.as_ref());
            let uv = array_summary(draw.uv_array.as_ref());
            let upload = draw
                .selected_upload
                .and_then(|idx| lg.uploads.get(idx).map(|u| upload_summary(u)))
                .unwrap_or_else(|| "upload=<none>".to_string());
            let state_words = draw
                .state_words
                .iter()
                .take(12)
                .map(|w| format!("{:#010x}", w))
                .collect::<Vec<_>>()
                .join(",");
            let uvs = draw
                .uvs
                .iter()
                .map(|(u, v)| format!("({:.1},{:.1})", u, v))
                .collect::<Vec<_>>()
                .join(",");
            let positions = draw
                .positions
                .iter()
                .map(|(x, y)| format!("({:.4},{:.4})", x, y))
                .collect::<Vec<_>>()
                .join(",");
            let color = draw
                .solid_color
                .map(|c| format!("solid=rgba({},{},{},{})", c.r, c.g, c.b, c.a))
                .unwrap_or_else(|| "solid=<none>".to_string());
            let tint = format!(
                "tint=rgba({},{},{},{}) texgen={}",
                draw.tint.r, draw.tint.g, draw.tint.b, draw.tint.a, draw.used_generated_uvs
            );
            let vertex_colors = draw
                .vertex_colors
                .map(|colors| {
                    let values = colors
                        .iter()
                        .map(|color| {
                            format!(
                                "({:.3},{:.3},{:.3},{:.3})",
                                color[0], color[1], color[2], color[3]
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("vertex_colors=[{}]", values)
                })
                .unwrap_or_else(|| "vertex_colors=<none>".to_string());
            info!(
                target: "EAPP_GL",
                "draw_detail guest_frame={} draw={} handle={:#x} state_ptr={:#010x} bound_tex={:?} enabled={:?} pos_arr={} uv_arr={} translation=({:.2},{:.2}) bounds=({:.1},{:.1})-({:.1},{:.1}) positions=[{}] uvs=[{}] inferred_dim={:?} {} {} {} {} coverage={} status={} state_words=[{}]",
                self.frame_counter,
                draw.draw_index + 1,
                draw.handle,
                draw.state_ptr,
                draw.bound_tex_name,
                draw.enabled_arrays,
                pos,
                uv,
                draw.translation.0,
                draw.translation.1,
                draw.bounds.0,
                draw.bounds.1,
                draw.bounds.2,
                draw.bounds.3,
                positions,
                uvs,
                draw.inferred_dim,
                upload,
                color,
                tint,
                vertex_colors,
                draw.coverage,
                draw.skipped_reason.as_deref().unwrap_or("rasterized"),
                state_words
            );
        }
    }

    /// Optional startup capture (`FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/...`). Writes
    /// a chronological TSV manifest for completed frames, and dumps PPMs for
    /// every presented framebuffer hash change plus periodic samples. Setting
    /// `FLIWHEEL_STARTUP_CAPTURE_DUMP_START_FRAME` keeps the manifest from
    /// frame zero but defers PPM output until the requested guest frame.
    fn capture_startup_completed_frame(&mut self, frame: &live_gl::CompletedFrame) {
        if !self.startup_capture.enabled {
            return;
        }
        if self.startup_capture.manifest_rows >= self.startup_capture.max_frames {
            return;
        }
        let host_us = self.host_start.elapsed().as_micros() as u64;
        let guest_time_current = self
            .read_guest_u32(self.app_object.wrapping_add(4))
            .unwrap_or(0);
        let guest_time_delta = self
            .read_guest_u32(self.app_object.wrapping_add(8))
            .unwrap_or(0);
        let hash_changed = self.startup_capture.last_hash != Some(frame.presented_hash);
        let periodic = self.frame_counter % self.startup_capture.periodic_interval == 0;
        let dump_allowed = self.frame_counter >= self.startup_capture.dump_start_frame;
        let target = self
            .startup_capture
            .target_frames
            .binary_search(&self.frame_counter)
            .is_ok();
        let targeted = !self.startup_capture.target_frames.is_empty();
        let reason = if dump_allowed && target {
            "target"
        } else if dump_allowed && !targeted && hash_changed {
            "hash_change"
        } else if dump_allowed && !targeted && periodic {
            "periodic"
        } else {
            ""
        };

        let mut output_path = String::new();
        if !reason.is_empty() && self.startup_capture.dump_count < self.startup_capture.max_dumps {
            let filename = format!(
                "startup_g{:06}_host{:012}_hash{:016x}.ppm",
                self.frame_counter, host_us, frame.presented_hash
            );
            let path = self.startup_capture.dir.join(filename);
            if let Some(fb) = self.live_gl.as_ref().map(|lg| lg.presented_buffer.clone()) {
                framebuffer_to_ppm(&path, &fb, live_gl::FB_WIDTH, live_gl::FB_HEIGHT);
                output_path = path.display().to_string();
                self.startup_capture.dump_count += 1;
            }
        }
        let handles = frame
            .handle_signature
            .iter()
            .map(|h| format!("{:#x}", h))
            .collect::<Vec<_>>()
            .join(",");
        let row = format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{:#018x}\t{:#018x}\t{}\t{}\n",
            self.frame_counter,
            host_us,
            guest_time_current,
            guest_time_delta,
            frame.draw_count,
            handles,
            frame.internal_hash,
            frame.presented_hash,
            reason,
            output_path
        );
        if let Ok(mut file) = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.startup_capture.manifest_path)
        {
            let _ = file.write_all(row.as_bytes());
        }
        self.startup_capture.manifest_rows += 1;
        self.startup_capture.last_hash = Some(frame.presented_hash);
    }

    /// Optional continuous frame dumping (`FLIWHEEL_GL_DUMP_FRAMES=N`). Writes
    /// only the first N completed presented frames.
    fn live_dump_completed_frame(&mut self) {
        let (path, fb) = {
            let Some(lg) = self.live_gl.as_mut() else {
                return;
            };
            if lg.dump_remaining == 0 {
                return;
            }
            let path = format!("/tmp/{}_live_frame_{:04}.ppm", lg.game_id, lg.dump_counter);
            lg.dump_counter += 1;
            lg.dump_remaining -= 1;
            (path, lg.presented_buffer.clone())
        };
        framebuffer_to_ppm(
            std::path::Path::new(&path),
            &fb,
            live_gl::FB_WIDTH,
            live_gl::FB_HEIGHT,
        );
        info!(target: "EAPP_GL", "dumped_completed_frame path={}", path);
    }

    /// Gate B for continuous rendering: publish the most recent completed
    /// presented frame to the desktop window under the render-state mutex.
    fn live_present_completed_to_window(&mut self) {
        let presented = match self.live_gl.as_ref() {
            Some(lg) => lg.presented_buffer.clone(),
            None => return,
        };
        let mut frame = self.render_state.lock().unwrap();
        for (dst, src) in frame.iter_mut().zip(presented.iter()) {
            *dst = ((src.r as u32) << 16) | ((src.g as u32) << 8) | (src.b as u32);
        }
    }

    /// Print a bounded structural comparison between the live candidate and
    /// the known offline replay expectations. Hash equality is NOT required;
    /// only structural parity (draw count, bounds, formats, composition).
    fn live_compare_to_offline(&mut self) {
        let summary = self.live_gl.as_ref().map(|lg| {
            let mut lines = String::new();
            lines.push_str(&format!("\n  live draws observed: {}", lg.draws.len()));
            for d in &lg.draws {
                let dim = d
                    .inferred_dim
                    .map(|(w, h)| format!("{}x{}", w, h))
                    .unwrap_or_else(|| "?".into());
                let reason = d.skipped_reason.as_deref().unwrap_or("rasterized");
                lines.push_str(&format!(
                    "\n    draw{} handle={:#x} dim={} upload={:?} bounds=({:.0},{:.0})-({:.0},{:.0}) cov={} {}",
                    d.draw_index + 1, d.handle, dim, d.selected_upload, d.bounds.0, d.bounds.1,
                    d.bounds.2, d.bounds.3, d.coverage, reason
                ));
            }
            lines.push_str(&format!("\n  uploads: {}", lg.uploads.len()));
            for u in &lg.uploads {
                lines.push_str(&format!(
                    "\n    upload{} {}x{} format={:?} src={:#010x}",
                    u.index, u.width, u.height, u.format, u.source_ptr
                ));
            }
            lines.push_str("\n  offline reference: 4 draws; bg 320x240 RGB565, logo 250x162 RGBA4444, ea 50x50 RGBA5551, overlay handle 3 (unresolved)");
            lines
        });
        if let Some(s) = summary {
            info!(target: "EAPP_GL", "live_vs_offline summary:{}", s);
        }

        // Optional pixel-diff against the offline presented PPM if present.
        self.live_pixel_diff_against_offline();
    }

    /// Best-effort pixel-diff of the live frame against the offline replay
    /// reference PPM, if that artifact exists on disk. We compare the INTERNAL
    /// (unflipped) buffer against the offline draws-1-3 reference
    /// (`tetris_frame4_real_draws_1_3.ppm`), since both intentionally skip the
    /// unresolved handle-3 overlay. Exact hash equality is not required, but is
    /// expected here. Skipped silently if the reference is absent.
    fn live_pixel_diff_against_offline(&mut self) {
        let internal = match self.live_gl.as_ref() {
            Some(lg) => lg.framebuffer.clone(),
            None => return,
        };
        let reference = match read_ppm_p6(std::path::Path::new(
            "/tmp/tetris_frame4_real_draws_1_3.ppm",
        )) {
            Some(bytes) => bytes,
            None => {
                info!(
                    target: "EAPP_GL",
                    "pixel_diff skipped: no offline reference PPM at /tmp/tetris_frame4_real_draws_1_3.ppm"
                );
                return;
            }
        };
        if reference.len() != internal.len() {
            info!(
                target: "EAPP_GL",
                "pixel_diff skipped: size mismatch live={} ref={}",
                internal.len(),
                reference.len()
            );
            return;
        }
        let diff = internal
            .iter()
            .zip(reference.iter())
            .filter(|(a, b)| {
                // Reference PPM is opaque RGB (a=255); compare RGB only.
                a.r != b.r || a.g != b.g || a.b != b.b
            })
            .count();
        info!(
            target: "EAPP_GL",
            "pixel_diff_vs_offline(internal vs draws_1_3) differing_pixels={} / {} ({:.4}%)",
            diff,
            internal.len(),
            100.0 * diff as f32 / internal.len() as f32
        );
        if diff == 0 {
            info!(
                target: "EAPP_GL",
                "pixel_diff_vs_offline EXACT MATCH with offline draws_1_3 (unflipped)"
            );
        }
    }

    /// Gate B: copy the presented framebuffer to the shared desktop render
    /// state. Keeps the internal and presented buffers conceptually separate;
    /// the internal framebuffer is never mutated by presentation.
    fn live_present_to_window(&mut self) {
        let presented = match self.live_gl.as_ref() {
            Some(lg) => lg.presented.clone(),
            None => return,
        };
        let Some(presented) = presented else {
            return;
        };
        let mut frame = self.render_state.lock().unwrap();
        for (dst, src) in frame.iter_mut().zip(presented.iter()) {
            *dst = ((src.r as u32) << 16) | ((src.g as u32) << 8) | (src.b as u32);
        }
        info!(target: "EAPP_GL", "gate_b presented live framebuffer to eapp window");
    }

    /// Best-effort decode of a GL surface/swap handle. We do not yet know the
    /// exact encoding, so we try several interpretations and log each result.
    fn decode_surface_handle(&mut self, ordinal: u32, handle: u32) {
        // Interpretation 1: direct guest pointer into work RAM.
        if (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&handle) {
            info!(
                target: "EAPP_GL",
                "GL:{} handle {:#010x} is a work-ram pointer; first 8 words:",
                ordinal, handle
            );
            for off in (0..32).step_by(4) {
                let v = self
                    .read_guest_u32(handle.wrapping_add(off))
                    .unwrap_or(0xdeadbeef);
                info!(target: "EAPP_GL", "  +{:#04x}: {:#010x}", off, v);
            }
        }
        // Interpretation 2: small-integer name indexing a GL object table.
        // The high bits of 0x0003f001 may encode type; low bits an index.
        let idx = handle & 0xffff;
        let tag = handle >> 16;
        info!(
            target: "EAPP_GL",
            "GL:{} handle {:#010x} as name: tag={:#06x} idx={}",
            ordinal, handle, tag, idx
        );
    }

    fn handle_misc_import(&mut self, ordinal: u32, args: [u32; 4]) -> u32 {
        match ordinal {
            0 => {
                // Runtime allocator. Guest wrappers pass the requested size in
                // r0; other arg registers are caller scratch and may contain
                // unrelated values. Using max(r0, r1) made iQuiz's malloc(0xa0)
                // fail when r1 held a stale large pointer/value. Keep these
                // LOST follows the direct runner's separate RetailOS-style
                // heap. Keep the established low work-RAM allocator for the
                // other titles, whose existing HLE contracts include their
                // current object addresses.
                let len = args[0].max(0x10);
                if matches!(self.metadata.title.as_str(), "1B200" | "12345") {
                    self.alloc_game_heap(len)
                } else {
                    self.alloc_zeroed(len)
                }
            }
            9 => {
                // Candidate monotonic tick API. Tetris calls this with r0
                // pointing at app_object+4 and app_object+8, then computes a
                // frame delta from the values stored there. The splash timeout
                // thresholds in the guest are 4_000_000 and 2_000_000, matching
                // microsecond units, so expose host monotonic microseconds.
                self.handle_misc9_time_api(args)
            }
            2 if self.metadata.title == "12345" => {
                // PR #3 binds miscTBD:2 as realloc(old, size). Vortex grows
                // its localization strings one append at a time; returning
                // zero leaves every key null and the parser never reaches
                // the next async asset. Preserve the old prefix and extend
                // the top allocation in place when possible.
                let old = args[0];
                let want = args[1];
                let result = match (old, want) {
                    (0, 0) => 0,
                    (0, size) => self.alloc_zeroed(size),
                    (_, 0) => 0,
                    (ptr, size) => {
                        let old_size = self.alloc_sizes.get(&ptr).copied().unwrap_or(0);
                        let rounded = (size + 0xf) & !0xf;
                        let old_end = ptr.saturating_add(old_size);
                        let new_end = ptr.saturating_add(rounded);
                        if old_size >= rounded {
                            ptr
                        } else if old_end == self.next_alloc
                            && new_end <= WORK_RAM_BASE + WORK_RAM_SIZE as u32
                        {
                            self.next_alloc = new_end;
                            self.alloc_sizes.insert(ptr, rounded);
                            ptr
                        } else {
                            let new_ptr = self.alloc_zeroed(size);
                            if new_ptr != 0 && old_size != 0 {
                                if let Some(bytes) = self.read_guest_bytes(ptr, old_size as usize) {
                                    let _ = self.write_guest_bytes(
                                        new_ptr,
                                        &bytes[..bytes.len().min(size as usize)],
                                    );
                                }
                            }
                            new_ptr
                        }
                    }
                };
                debug!(
                    target: "EAPP_IMPORT",
                    "miscTBD:2 Vortex realloc old={:#010x} size={} -> {:#010x}",
                    old,
                    want,
                    result
                );
                result
            }
            6 => {
                // Lost (1B200) calls this with r0=2 (command type?),
                // r1=pointer into rserver data region (0x10012038 =
                // rserver+0x11000), r2/r3=string pointers.
                // Could be a render-server communication channel.
                // Return value might affect game's draw decision.
                let ret = fliwheel_var("MISCTBD6_RET")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(0);
                info!(target: "EAPP_IMPORT", "miscTBD:6 cmd={:#x} data_ptr={:#010x} r2={:#010x} r3={:#010x} ret={:#x}", args[0], args[1], args[2], args[3], ret);
                if args[0] == 2 && args[1] != 0 {
                    if let Some(bytes) = self.read_guest_bytes(args[1], 0x1000) {
                        let blocks = UsseProgram::scan_runtime_blocks(args[1], &bytes);
                        if !blocks.is_empty() {
                            let summary = blocks
                                .iter()
                                .map(|b| b.summary())
                                .collect::<Vec<_>>()
                                .join("; ");
                            info!(target: "EAPP_GL", "rserver_blocks: {}", summary);
                        }
                    }
                }
                ret
            }
            12 => self.handle_misc12_local_time(args),
            _ => 0,
        }
    }

    fn handle_input_events_import(&mut self, ordinal: u32, args: [u32; 4]) -> u32 {
        let state = self.effective_input_state();
        match ordinal {
            // Observed Tetris callsite passes two stack pointers and then reads
            // back [r1] after the import returns. Return the compact bitfield
            // for callers that use r0, but also write it through both pointer
            // args so pointer-output ABI users actually see host input.
            0 => {
                // The reversed-register Sudoku/Solitaire wrapper polls once
                // after the guest has consumed the previous list head. Clear
                // that old node now, rather than during the same import that
                // created it (which makes the guest miss the edge).
                if self.input_event_reversed_clear_pending {
                    if let Some(owner) = self.input_event_reversed_owner {
                        self.write_guest_u32(owner.wrapping_add(0x30), 0);
                    }
                    self.input_event_reversed_clear_pending = false;
                }
                let vortex_direct = self.metadata.title == "12345"
                    && fliwheel_var_os("VORTEX_PR3_GL165").is_some();
                let button_bits = match self.metadata.title.as_str() {
                    "55555" => {
                        // Bejeweled uses the four clickwheel quadrants as a
                        // touch direction for swapping, not as ordinary D-pad
                        // buttons. Keep action/menu available, but let the
                        // title-specific packet carry the directional tap
                        // contract.
                        Self::input_state_bits(&state) & !0x0f
                    }
                    "1B200" => {
                        // LOST's direct runner has no flags-word address: its
                        // button state arrives through the linked event node,
                        // while InputEvents:0 supplies only the wheel packet.
                        // Returning the action bit as well changes the title's
                        // compact input state relative to that oracle.
                        0
                    }
                    "12345" if vortex_direct => {
                        // PR #3's Vortex runner writes only the wheel sample
                        // through InputEvents:0. Select/Menu are carried by
                        // the title-local flags word below, not this generic
                        // logical-button return value.
                        0
                    }
                    _ => Self::input_state_bits(&state),
                };
                let wheel_bits = if matches!(self.metadata.title.as_str(), "1B200" | "12345") {
                    self.lost_wheel_poll_bits(state.wheel_delta)
                } else {
                    self.wheel_input_bits(state.wheel_delta)
                };
                let event_state = if self.metadata.title == "55555" {
                    // In Bejeweled the directional keys represent a
                    // clickwheel touch quadrant. Do not also expose them as
                    // ordinary linked button events, or the shared event
                    // consumer moves the selection before the touch handler
                    // can perform the swap.
                    let mut event_state = state.clone();
                    event_state.up = false;
                    event_state.down = false;
                    event_state.left = false;
                    event_state.right = false;
                    event_state
                } else {
                    state.clone()
                };
                // Vortex has a flags-word button ABI in the direct oracle,
                // so do not allocate or publish the generic linked event
                // nodes. Still track the logical edge so a Select/Menu press
                // can queue the same current wheel sample that the oracle
                // queues alongside the flag transition.
                let vortex_pressed = if vortex_direct {
                    let current = self.input_event_id_mask(&event_state);
                    let previous = self.input_event_prev_mask;
                    self.input_event_prev_mask = current;
                    current & !previous != 0
                } else {
                    false
                };
                let event_list = if vortex_direct {
                    0
                } else {
                    self.build_input_event_list(&event_state)
                };
                let event_wheel_bits = if vortex_direct && vortex_pressed {
                    self.lost_wheel_event_bits()
                } else if matches!(self.metadata.title.as_str(), "1B200" | "12345")
                    && event_list != 0
                {
                    // RetailOS queues the current wheel sample alongside every
                    // LOST/Vortex button press/release. Both games only enter
                    // their linked-event dispatchers after seeing bit 30 from
                    // this poll, so an event node without this packet is
                    // ignored. PR #3's Vortex runner uses raw position zero
                    // here when a button is pressed without wheel motion.
                    self.lost_wheel_event_bits()
                } else {
                    0
                };
                let bits = button_bits
                    | self.env_input_script_bits()
                    | wheel_bits
                    | event_wheel_bits
                    | self.bejeweled_tap_bits(&state);
                // Vortex's shared input wrapper also keeps a separate
                // physical-button word in the image. The PR #3 oracle finds
                // this word at 0x18063e5c and writes Select as bit 0x01 and
                // Menu as bit 0x10. Preserve the generic packet above for
                // the rest of the runtime, but mirror the two measured
                // controls into Vortex's title-local ABI so a Select edge can
                // leave the animated title screen.
                if self.metadata.title == "12345" {
                    let mut vortex_flags = self.env_input_script_bits() & 0x1f;
                    if state.action {
                        vortex_flags |= 0x01;
                    }
                    if state.menu {
                        vortex_flags |= 0x10;
                    }
                    let _ = self.write_guest_u32(VORTEX_INPUT_FLAGS, vortex_flags);
                }
                if args[0] != 0 && !vortex_direct {
                    self.write_guest_u32(args[0], bits);
                }
                if args[1] != 0 {
                    self.write_guest_u32(args[1], bits);
                }
                let mode = self.cpu.mode();
                let r4 = self.cpu.reg_get(mode, 4);
                let r5 = self.cpu.reg_get(mode, 5);
                // Most titles pass the event-list owner in r4 and a context in
                // r5 (Tetris is the reference shape). The Sudoku/Solitaire
                // family reverses those registers: r5 is the owner and r4 is
                // empty. Select the work-RAM register rather than assuming a
                // single caller convention so the linked event list reaches
                // both guest consumers.
                let r4_is_work_ram =
                    (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&r4);
                let r5_is_work_ram =
                    (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&r5);
                let input_obj = if r4_is_work_ram {
                    r4
                } else if r5_is_work_ram {
                    r5
                } else {
                    0
                };
                let input_ctx = if input_obj == r4 { r5 } else { r4 };
                let reversed_owner = !r4_is_work_ram && r5_is_work_ram;
                if self.metadata.title == "1B200" && self.app_object != 0 {
                    // LOST's frame manager keeps the event-list head directly in
                    // its game-heap context. It does not pass that context in a
                    // work-RAM owner register, so the generic owner inference
                    // above otherwise leaves the freshly built list unreachable.
                    self.write_guest_u32(self.app_object.wrapping_add(0x30), event_list);
                } else if input_obj != 0 {
                    // Tetris' post-import wrapper passes [input_obj+0x30] as
                    // the event-list head to the event consumer. input_ctx+0x20
                    // is a filter/state mask, not the list pointer. Always
                    // overwrite the head, including zero, so a one-frame press
                    // cannot be re-consumed forever as a stale event node.
                    self.write_guest_u32(input_obj.wrapping_add(0x30), event_list);
                    if reversed_owner {
                        self.input_event_reversed_owner = Some(input_obj);
                    }
                } else if event_list != 0 {
                    // Once the wrapper drops the owner register, retain the
                    // same guest-visible ABI by linking the new edge through
                    // the last reversed owner. The guest will consume it at
                    // the start of its next frame, and the next poll clears
                    // it through the deferred path above.
                    if let Some(owner) = self.input_event_reversed_owner {
                        self.write_guest_u32(owner.wrapping_add(0x30), event_list);
                        self.input_event_reversed_clear_pending = true;
                    }
                }
                if input_obj != 0 && reversed_owner && event_list != 0 {
                    self.input_event_reversed_clear_pending = true;
                }
                if bits != 0 || event_list != 0 {
                    info!(
                        target: "EAPP_INPUT",
                        "InputEvents:0 frame={} bits={:#010x} event_list={:#010x} input_obj={:#010x} input_ctx={:#010x} args=[{:#010x},{:#010x},{:#010x},{:#010x}] state={:?}",
                        self.frame_counter,
                        bits,
                        event_list,
                        input_obj,
                        input_ctx,
                        args[0],
                        args[1],
                        args[2],
                        args[3],
                        state
                    );
                }
                // The direct InputPoll stub returns zero; the wheel packet is
                // observed through the second out-pointer instead.
                if vortex_direct { 0 } else { bits }
            }
            1 => self.alloc_zeroed(0x40),
            _ => 0,
        }
    }

    fn effective_input_state(&mut self) -> EappInputState {
        let mut live_state = self.input_state.lock().unwrap();
        let mut state = live_state.clone();
        // Wheel callbacks deliver relative motion. Consume it once when the
        // guest polls InputEvents so a single host scroll cannot be replayed
        // on every subsequent frame.
        live_state.wheel_delta = 0.0;
        drop(live_state);
        let apply_script_wheel = self.input_script_wheel_frame != Some(self.frame_counter);
        if apply_script_wheel {
            self.input_script_wheel_frame = Some(self.frame_counter);
        }
        self.apply_env_input_script(&mut state, apply_script_wheel);
        state
    }

    fn input_state_bits(state: &EappInputState) -> u32 {
        let mut bits = 0u32;
        if state.up {
            bits |= 1 << 0;
        }
        if state.down {
            bits |= 1 << 1;
        }
        if state.left {
            bits |= 1 << 2;
        }
        if state.right {
            bits |= 1 << 3;
        }
        if state.action {
            bits |= 1 << 4;
        }
        if state.menu {
            bits |= 1 << 5;
        }
        bits
    }

    /// Convert a relative host scroll amount into the compact InputEvents
    /// wheel packet used by the decrypted games. The hardware has 96
    /// detents, but the direct EAPP import ABI exposes a normalized 0..255
    /// ring. The real firmware performs that normalization between the
    /// hardware register and the application; fliwheel bypasses that layer,
    /// so retain the physical position here and encode the guest-facing
    /// position explicitly.
    const CLICKWHEEL_DETENTS: u32 = 96;
    const EAPP_WHEEL_UNITS: u32 = 256;

    fn normalized_wheel_position(position: u8) -> u8 {
        ((position as u32 * Self::EAPP_WHEEL_UNITS) / Self::CLICKWHEEL_DETENTS) as u8
    }

    fn wheel_frame_bits(position: &mut u8, delta: f32) -> u32 {
        let clicks = (-delta * 2.0) as i16;
        if clicks == 0 {
            return 0;
        }
        *position = ((*position as i16 + clicks).rem_euclid(Self::CLICKWHEEL_DETENTS as i16)) as u8;
        (1 << 30) | Self::normalized_wheel_position(*position) as u32
    }

    fn wheel_input_bits(&mut self, delta: f32) -> u32 {
        Self::wheel_frame_bits(&mut self.wheel_position, delta)
    }

    /// LOST and Vortex's InputEvents consumers use the RetailOS encoder
    /// recovered in the PR #3 direct runner. It exposes a 120-step raw ring
    /// through an inverted 8-bit transform rather than the normalized
    /// 96-detent packet used by the other title families.
    const LOST_WHEEL_DETENTS: i16 = 120;

    fn lost_wheel_byte(raw: u8) -> u8 {
        ((((0x77i32 - raw as i32) * 8) / 3) & 0xff) as u8
    }

    fn lost_wheel_packet(raw: u8) -> u32 {
        (1 << 30) | Self::lost_wheel_byte(raw) as u32
    }

    fn lost_wheel_poll_bits(&mut self, delta: f32) -> u32 {
        // The PR runner queues one absolute sample per raw detent and the
        // guest consumes one sample per InputEvents poll. Preserve that queue
        // behavior for both scripted and live relative motion.
        if delta.is_finite() {
            self.lost_wheel_remainder += delta;
        }
        let mut added = 0usize;
        while self.lost_wheel_remainder.abs() >= 1.0
            && added < Self::LOST_WHEEL_DETENTS as usize
        {
            let step = if self.lost_wheel_remainder > 0.0 { 1 } else { -1 };
            self.lost_wheel_remainder -= step as f32;
            self.lost_wheel_raw = ((self.lost_wheel_raw as i16 + step)
                .rem_euclid(Self::LOST_WHEEL_DETENTS)) as u8;
            self.lost_wheel_pending.push_back(self.lost_wheel_raw);
            added += 1;
        }
        self.lost_wheel_pending
            .pop_front()
            .map(Self::lost_wheel_packet)
            .unwrap_or(0)
    }

    fn lost_wheel_event_bits(&mut self) -> u32 {
        // The oracle appends the current sample after any already queued wheel
        // reports. If the queue is empty, the current button poll consumes it
        // immediately; otherwise retain it behind the pending reports.
        if self.lost_wheel_pending.is_empty() {
            Self::lost_wheel_packet(self.lost_wheel_raw)
        } else {
            self.lost_wheel_pending.push_back(self.lost_wheel_raw);
            0
        }
    }

    /// Bejeweled treats a clickwheel-side tap as a touch packet whose angle
    /// selects the adjacent gem. The desktop frontend's arrow keys are the
    /// natural equivalent of those four physical quadrants. This stays
    /// title-scoped because the same keys are ordinary buttons elsewhere.
    fn bejeweled_tap_bits(&self, state: &EappInputState) -> u32 {
        if self.metadata.title != "55555" {
            return 0;
        }
        Self::bejeweled_tap_angle(state)
            .map(|angle| (1 << 30) | angle as u32)
            .unwrap_or(0)
    }

    fn bejeweled_tap_angle(state: &EappInputState) -> Option<u8> {
        if state.up {
            Some(0x30)
        } else if state.left {
            Some(0x70)
        } else if state.down {
            Some(0xb0)
        } else if state.right {
            Some(0xf0)
        } else {
            None
        }
    }

    /// Headless input smoke-test helper. Format:
    /// `FLIWHEEL_EAPP_INPUT_SCRIPT="menu:190-200,menu:230-240,action:260-270"`.
    /// Wheel detents can be scripted with `wheelup`, `wheeldown`, or an
    /// explicit relative delta such as `wheel=-2:300-305`.
    /// Raw masks can also be injected for ABI discovery, e.g.
    /// `bits=0x40000001:190-195`. This intentionally layers on top of live host
    /// input and is ignored when unset, so normal headed input remains
    /// controlled by minifb callbacks.
    fn apply_env_input_script(&self, state: &mut EappInputState, apply_wheel: bool) {
        let Ok(script) = fliwheel_var("EAPP_INPUT_SCRIPT") else {
            return;
        };
        for entry in script.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let Some((key, range)) = entry.split_once(':') else {
                continue;
            };
            let Some((start, end)) = range.split_once('-') else {
                continue;
            };
            let (Ok(start), Ok(end)) = (start.trim().parse::<u64>(), end.trim().parse::<u64>())
            else {
                continue;
            };
            if self.frame_counter < start || self.frame_counter > end {
                continue;
            }
            let key = key.trim().to_ascii_lowercase();
            match key.as_str() {
                "up" => state.up = true,
                "down" => state.down = true,
                "left" => state.left = true,
                "right" => state.right = true,
                "action" | "select" | "enter" => state.action = true,
                "menu" | "m" => state.menu = true,
                // A positive GUI scroll delta is the same direction as a
                // wheel-down event. The sign is preserved in the packet
                // conversion below, matching the desktop callback path.
                "wheel" | "wheeldown" | "scroll" if apply_wheel => state.wheel_delta += 1.0,
                "wheelup" if apply_wheel => state.wheel_delta -= 1.0,
                _ => {
                    if apply_wheel {
                        if let Some(raw) = key.strip_prefix("wheel=") {
                            if let Ok(delta) = raw.parse::<f32>() {
                                state.wheel_delta += delta;
                            }
                        }
                    }
                }
            }
        }
    }

    fn env_input_script_bits(&self) -> u32 {
        let mut bits = 0u32;
        for (key, _range) in self.active_env_input_script_entries() {
            let Some(raw) = key
                .strip_prefix("bits=")
                .or_else(|| key.strip_prefix("raw="))
            else {
                continue;
            };
            let parsed = u32::from_str_radix(raw.trim_start_matches("0x"), 16)
                .or_else(|_| raw.parse::<u32>());
            if let Ok(mask) = parsed {
                bits |= mask;
            }
        }
        bits
    }

    fn build_input_event_list(&mut self, state: &EappInputState) -> u32 {
        let current = self.input_event_id_mask(state);
        let previous = self.input_event_prev_mask;
        self.input_event_prev_mask = current;

        let pressed = current & !previous;
        let released = previous & !current;
        if pressed == 0 && released == 0 {
            return 0;
        }

        let mut events = Vec::new();
        // Most of the games we have traced use byte 1 = 2 for press and 1 for
        // release. LOST's event wrapper follows the PR runner's convention:
        // 1 is down and 2 is up. Emit only edges; titles that use compact
        // button state keep that state available through ordinal 0 as well.
        let (pressed_kind, released_kind) = if self.metadata.title == "1B200" {
            (1u8, 2u8)
        } else {
            (2u8, 1u8)
        };
        for id in 1..=5u8 {
            let bit = 1u8 << id;
            if pressed & bit != 0 {
                events.push((id, pressed_kind));
            }
            if released & bit != 0 {
                events.push((id, released_kind));
            }
        }

        let mut next = 0u32;
        for (id, kind) in events.into_iter().rev() {
            let node = self.alloc_zeroed(0x10);
            let _ = self.write_guest_bytes(node, &[id, kind]);
            let _ = self.write_guest_u32(node.wrapping_add(4), self.frame_counter as u32);
            let _ = self.write_guest_u32(node.wrapping_add(8), next);
            next = node;
        }
        next
    }

    fn input_event_id_mask(&self, state: &EappInputState) -> u8 {
        let mut mask = Self::input_event_id_mask_for_state(state);
        for (key, _range) in self.active_env_input_script_entries() {
            if let Some(raw) = key.strip_prefix("event=") {
                if let Ok(id) = raw.parse::<u8>() {
                    if (1..=5).contains(&id) {
                        mask |= 1 << id;
                    }
                }
            }
        }
        mask
    }

    fn input_event_id_mask_for_state(state: &EappInputState) -> u8 {
        // The decrypted title probe confirms the five event-list IDs used by
        // the shared game runtime: 1=menu/back, 2=action/select, 3=left,
        // 4=right, and 5=up/down. This is intentionally separate from
        // input_state_bits(), which exposes the compact packet bits directly.
        let mut mask = 0u8;
        if state.menu {
            mask |= 1 << 1;
        }
        if state.action {
            mask |= 1 << 2;
        }
        if state.left {
            mask |= 1 << 3;
        }
        if state.right {
            mask |= 1 << 4;
        }
        if state.up || state.down {
            mask |= 1 << 5;
        }
        mask
    }

    fn active_env_input_script_entries(&self) -> Vec<(String, String)> {
        let Ok(script) = fliwheel_var("EAPP_INPUT_SCRIPT") else {
            return Vec::new();
        };
        let mut active = Vec::new();
        for entry in script.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let Some((key, range)) = entry.split_once(':') else {
                continue;
            };
            let Some((start, end)) = range.split_once('-') else {
                continue;
            };
            let (Ok(start), Ok(end)) = (start.trim().parse::<u64>(), end.trim().parse::<u64>())
            else {
                continue;
            };
            if self.frame_counter < start || self.frame_counter > end {
                continue;
            }
            active.push((key.trim().to_ascii_lowercase(), range.trim().to_string()));
        }
        active
    }

    fn handle_settings_import(&mut self, ordinal: u32, args: [u32; 4]) -> u32 {
        match ordinal {
            // Settings:0 is a scalar setting query. Tetris calls it as
            //   Settings:0(key_cstr, out_value_ptr, size_ptr)
            // e.g. key="Language", with *size_ptr initialized to 4, then treats
            // return >= 0 as success and reads *out_value_ptr. Leaving the out
            // value untouched makes the guest use stack garbage as a language
            // index, which corrupts localization string selection.
            0 => {
                let key = self
                    .try_read_c_string(args[0], 64)
                    .unwrap_or_else(|| "<unknown>".to_string());
                let value = match key.as_str() {
                    // Guest language enum (not yet proven to be the same as
                    // Strings.dta column order). Default to 0, but allow quick
                    // RE/brute-force from the environment.
                    "Language" => fliwheel_var("EAPP_LANGUAGE")
                        .ok()
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(0),
                    // Conservative default for other scalar settings until
                    // their exact value domains are reverse-engineered.
                    _ => 0u32,
                };
                if args[1] != 0 {
                    self.write_guest_u32(args[1], value);
                }
                if args[2] != 0 {
                    // Preserve/confirm the caller-provided scalar byte size.
                    self.write_guest_u32(args[2], 4);
                }
                info!(
                    target: "EAPP_IMPORT",
                    "Settings:0 key={} value={} out={:#010x} size={:#010x}",
                    key,
                    value,
                    args[1],
                    args[2]
                );
                0
            }
            // Commonly-polled region / time-format values. Return default 0.
            1 => 0,
            2 => 0,
            _ => 0,
        }
    }

    fn handle_audio_import(&mut self, ordinal: u32, pc: u32, lr: u32, args: [u32; 4]) -> u32 {
        self.trace_audio_call(ordinal, pc, lr, args);

        // Audio:52 (r0=rserver base) and Audio:51 (r0=prev_ret, r3=shared_ptr)
        // are part of Lost's render-server init. The game divides:
        //   result = -Audio51_ret / Audio52_ret
        // Keep the measured non-zero returns for those two init shapes.
        if ordinal == 52 && args[0] >= 0x1000_0000 {
            info!(
                target: "EAPP_GL",
                "audio_52_rserver: r0={:#010x} ret=1",
                args[0]
            );
            return 1;
        }
        if ordinal == 52 && self.metadata.title == "12345" {
            // Vortex uses Audio:52 as its 0xff divisor/format default. The
            // PR #3 runner returns 0xff for its normal call (r0=0x10001);
            // only the LOST render-server shape above returns 1.
            return 0xff;
        }
        if ordinal == 51 && args[3] >= 0x1000_0000 {
            info!(
                target: "EAPP_GL",
                "audio_51_rserver: r0={:#010x} ret=1",
                args[0]
            );
            return 1;
        }

        match ordinal {
            0 => {
                // Tetris calls Audio:0 as (active_channel, slot_index,
                // table_base, 0), then stores the returned source handle in
                // table_base + type*0x100 + slot_index*4. The guest later
                // passes that exact handle to Audio:23 and Audio:1. A
                // non-negative synthetic identity is enough for this first
                // ABI layer; staged PCM binding remains a separate step.
                // Vortex's 47 descriptors use the zero-based slot returned by the
                // PR #3 runner, including slot 0. The other title families use
                // the existing one-based synthetic handle convention.
                let vortex_zero_based = self.metadata.title == "12345"
                    && fliwheel_var_os("VORTEX_PR3_GL165").is_some();
                let handle = if vortex_zero_based {
                    self.next_audio_handle.saturating_sub(1)
                } else {
                    self.next_audio_handle.max(1)
                };
                self.next_audio_handle = self.next_audio_handle.wrapping_add(1).max(1);
                let mut source = AudioHandle {
                    source_type: args[0],
                    slot_index: args[1],
                    table_base: args[2],
                    host_path: None,
                };
                if self.uses_wav_audio_source_abi() {
                    self.audio_pending_asset_handle = Some(handle);
                } else if self.uses_pacman_wav_audio_source_abi() {
                    self.audio_pacman_pending_handles.push_back(handle);
                } else if self.uses_mspacman_wav_audio_source_abi() {
                    // Ms. PAC-MAN's persistent source is allocated after its
                    // matching temporary header source has been released.
                    // Bind the path captured during that header read here.
                    if let Some(position) = self
                        .audio_mspacman_pending_asset_paths
                        .iter()
                        .position(|(source_type, slot_index, table_base, _)| {
                            *source_type == source.source_type
                                && *slot_index == source.slot_index
                                && *table_base == source.table_base
                        })
                    {
                        let (_, _, _, host_path) = self
                            .audio_mspacman_pending_asset_paths
                            .remove(position)
                            .expect("audio Ms. PAC-MAN pending path position");
                        source.host_path = Some(host_path.clone());
                        if std::env::var_os("EAPP_AUDIO_EVENT_TRACE").is_some() {
                            let resource_index = source.table_base;
                            info!(
                                target: "EAPP_AUDIO",
                                "AudioSourceMap handle={:#010x} type={} slot={} path={}",
                                handle,
                                source.source_type,
                                resource_index,
                                host_path.display()
                            );
                        }
                        self.audio_pending_asset_handle = None;
                    } else {
                        // This is the temporary source whose header read will
                        // identify the next persistent source allocation.
                        self.audio_pending_asset_handle = Some(handle);
                    }
                }
                self.audio_handles.insert(handle, source.clone());
                if fliwheel_var_os("AUDIO_TRACE").is_some() {
                    info!(
                        target: "EAPP_AUDIO",
                        "AudioAlloc:0 handle={:#010x} slot={} table={:#010x}",
                        handle,
                        source.slot_index,
                        source.table_base
                    );
                }
                handle
            }
            2 => {
                // PopCap titles configure a source with Audio:13/14/15 and
                // commit the trigger with Audio:2. The source handle is the
                // value returned by Audio:0, so resolve the previously bound
                // WAV only at this final trigger point.
                if self.uses_wav_audio_source_abi()
                    || self.uses_pacman_wav_audio_source_abi()
                    || self.uses_mspacman_wav_audio_source_abi()
                {
                    if let Some(source) = self.audio_handles.get(&args[0]).cloned() {
                        if let Some(host_path) = source.host_path {
                            let resource_index = if self.uses_mspacman_wav_audio_source_abi() {
                                source.table_base
                            } else {
                                source.slot_index
                            };
                            self.queue_audio_event(
                                source.source_type,
                                resource_index,
                                Some(host_path),
                            );
                        }
                    }
                }
                0
            }
            1 | 23 => {
                let removed = self.audio_handles.remove(&args[0]);
                if fliwheel_var_os("AUDIO_TRACE").is_some() {
                    info!(
                        target: "EAPP_AUDIO",
                        "AudioRelease:{} handle={:#010x} known={}",
                        ordinal,
                        args[0],
                        removed.is_some()
                    );
                }
                0
            }
            _ => 0,
        }
    }

    /// Env-gated (`FLIWHEEL_AUDIO_TRACE=1`) diagnostic that dumps, for each
    /// `Audio:*` import call, the register args plus a short byte preview of
    /// any arg that looks like a guest pointer (work-RAM or file VMA). When
    /// the pointed region begins with a RIFF/WAVE header, the parsed format
    /// (channels, sample rate, bits, data offset/length) is logged too.
    ///
    /// This is purely an investigation aid for deriving the Audio runtime
    /// ABI across titles. It does not change guest-visible behavior.
    fn trace_audio_call(&mut self, ordinal: u32, pc: u32, lr: u32, args: [u32; 4]) {
        if fliwheel_var_os("AUDIO_TRACE").is_none() {
            return;
        }
        let work_end = WORK_RAM_BASE.saturating_add(WORK_RAM_SIZE as u32);
        let image_end = FILE_VMA_BASE.saturating_add(self.bus.image_len);
        let is_ptr = |v: u32| {
            (WORK_RAM_BASE..work_end).contains(&v) || (FILE_VMA_BASE..image_end).contains(&v)
        };
        let mut detail = String::new();
        for (i, arg) in args.iter().copied().enumerate() {
            if !is_ptr(arg) || arg == 0 {
                continue;
            }
            let audio_path = (0..=3u32).find_map(|offset| {
                let addr = arg.wrapping_add(offset);
                let text = self.try_read_c_string(addr, 256)?;
                let lower = text.to_ascii_lowercase();
                let is_audio_asset = lower.ends_with(".wav")
                    || lower.ends_with(".m4a")
                    || lower.ends_with(".aac")
                    || lower.ends_with(".blob");
                is_audio_asset.then(|| (offset, addr, text))
            });
            let preview = match self.read_guest_bytes(arg, 64) {
                Some(b) => b,
                None => continue,
            };
            let hex: String = preview
                .iter()
                .take(16)
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(" ");
            if !detail.is_empty() {
                detail.push_str(" ");
            }
            if let Some((offset, string_addr, path)) = audio_path {
                let host = self
                    .resolve_bundle_path(&path)
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<missing>".to_string());
                detail.push_str(&format!(
                    "r{}={:#010x}->AUDIO_PATH+{}={:?} host={}",
                    i, string_addr, offset, path, host
                ));
                continue;
            }
            // RIFF/WAVE sniff: "RIFF" <len:le32> "WAVE" "fmt " ...
            let riff =
                preview.len() >= 12 && &preview[0..4] == b"RIFF" && &preview[8..12] == b"WAVE";
            if riff {
                let wav = self.describe_wav_at(arg);
                detail.push_str(&format!(
                    "r{}={:#010x}->WAV[{}]",
                    i,
                    arg,
                    wav.unwrap_or_else(|| "unparsed".to_string())
                ));
            } else {
                detail.push_str(&format!("r{}={:#010x}->[{}]", i, arg, hex));
            }
        }
        info!(
            target: "EAPP_AUDIO",
            "Audio:{} frame={} pc={:#010x} lr={:#010x} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} {}",
            ordinal,
            self.frame_counter,
            pc,
            lr,
            args[0],
            args[1],
            args[2],
            args[3],
            detail
        );
    }

    /// Best-effort WAV header parse at a guest pointer. Returns a compact
    /// one-line description: `ch=N rate=N bits=N data=@off=off,len=N`. Used
    /// by the audio tracer; not yet wired to playback.
    fn describe_wav_at(&mut self, addr: u32) -> Option<String> {
        let header = self.read_guest_bytes(addr, 44)?;
        if header.len() < 12 || &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
            return None;
        }
        // Walk chunks from offset 12.
        let mut pos = 12usize;
        let mut fmt = None::<(u16, u32, u16)>; // channels, rate, bits
        let mut data = None::<(usize, u32)>; // offset, len
        let mut guard = 0usize;
        while pos + 8 <= header.len() && guard < 8 {
            guard += 1;
            let chunk_id = &header[pos..pos + 4];
            let chunk_len = u32::from_le_bytes([
                header[pos + 4],
                header[pos + 5],
                header[pos + 6],
                header[pos + 7],
            ]) as usize;
            match chunk_id {
                b"fmt " => {
                    // standard 16-byte PCM fmt chunk
                    if pos + 8 + 16 <= header.len() {
                        let audio_format = u16::from_le_bytes([header[pos + 8], header[pos + 9]]);
                        let channels = u16::from_le_bytes([header[pos + 10], header[pos + 11]]);
                        let rate = u32::from_le_bytes([
                            header[pos + 12],
                            header[pos + 13],
                            header[pos + 14],
                            header[pos + 15],
                        ]);
                        let bits = u16::from_le_bytes([header[pos + 22], header[pos + 23]]);
                        if audio_format == 1 {
                            fmt = Some((channels, rate, bits));
                        }
                    }
                }
                b"data" => {
                    data = Some((pos + 8, chunk_len as u32));
                    break;
                }
                _ => {}
            }
            pos += 8 + chunk_len + (chunk_len & 1); // pad to even
        }
        let (ch, rate, bits) = fmt?;
        let (off, len) = data?;
        Some(format!(
            "fmt=PCM ch={} rate={} bits={} data=@{}=0x{:x},len={}",
            ch, rate, bits, off, off, len
        ))
    }

    fn handle_async_file_io_import(&mut self, ordinal: u32, args: [u32; 4]) -> u32 {
        // Direct file-handle read path. Tetris reaches this after a Menu input:
        // wrapper 0x609c calls AsyncFileIO:14(handle, buffer, len) after
        // wrapper 0x6068 opened `prefs.sav` with ordinal 12. Earlier emulation
        // only returned `len`, which meant the guest buffer stayed stale even
        // though the call looked successful. Copy the tracked host bytes into
        // the guest buffer and zero-fill short reads so successful reads do not
        // expose uninitialized guest memory.
        if ordinal == 14 {
            let handle = args[0];
            let buffer = args[1];
            let len = args[2] as usize;
            if handle == u32::MAX || buffer == 0 {
                warn!(target: "EAPP_IMPORT", "AsyncFileIO:14 called with invalid args=[{:#010x},{:#010x},{:#010x},{:#010x}]", args[0], args[1], args[2], args[3]);
                return 0;
            }
            let Some(host_path) = self.async_open_files.get(&handle).cloned() else {
                warn!(target: "EAPP_IMPORT", "AsyncFileIO:14 unknown handle={} buffer={:#010x} len={}", handle, buffer, len);
                return 0;
            };
            let bytes = fs::read(&host_path).unwrap_or_else(|e| {
                warn!(target: "EAPP_IMPORT", "AsyncFileIO:14 read error for {}: {}", host_path.display(), e);
                Vec::new()
            });
            let n = bytes.len().min(len);
            let mut out = vec![0u8; len];
            out[..n].copy_from_slice(&bytes[..n]);
            let delivered = self.write_guest_bytes(buffer, &out);
            info!(
                target: "EAPP_IMPORT",
                "AsyncFileIO:14 handle={} path={} buffer={:#010x} len={} file_bytes={} delivered={}",
                handle,
                host_path.display(),
                buffer,
                len,
                bytes.len(),
                delivered
            );
            return if delivered { n as u32 } else { 0 };
        }

        // Direct file-handle status poll. Wrapper 0x603c calls this when the
        // open status in `[file+4]` is 0 or 5. Returning 1 keeps the guest's
        // status compatible with the earlier accidental behavior (open return
        // value was handle 1) while still separating status from `[file+0]`.
        if ordinal == 16 {
            let handle = args[0];
            let known = self.async_open_files.contains_key(&handle);
            info!(target: "EAPP_IMPORT", "AsyncFileIO:16 handle={} known={}", handle, known);
            return if known { 1 } else { 0 };
        }

        if ordinal == 1 {
            // No-path owner/read path used by Tetris initiator C (0x1801fd74)
            // after AsyncFileIO:2 has advanced the audio-stream entry. Ms.
            // PAC-MAN uses the same transient-owner shape, but its firmware
            // completion trampoline is the guest's 0x180168f8 rather than the
            // Tetris-specific 0x1801fbfc helper. Keep the completion
            // title-scoped; Ms. PAC-MAN's measured contract is now the normal
            // path while the other title probes remain opt-in.
            let complete = fliwheel_var("EAPP_ASYNC3_COMPLETE")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false);
            let is_tetris = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("66666"));
            let is_texas = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("33333"));
            let is_sudoku = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("50513"));
            let is_sims = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("1500C") || p.contains("1500E"));
            let is_royal = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("50514"));
            let is_mspacman = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("14004"));
            let is_lost = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("1B200"));
            let is_vortex = self.metadata.title == "12345";
            let owner = args[0];
            let req = self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0);
            let req_cb = self.read_guest_u32(req.wrapping_add(0x0c)).unwrap_or(0);
            let req_ctx = self.read_guest_u32(req.wrapping_add(0x10)).unwrap_or(0);
            let owner_cb = self.read_guest_u32(owner.wrapping_add(0x0c)).unwrap_or(0);
            let owner_ctx = self.read_guest_u32(owner.wrapping_add(0x10)).unwrap_or(0);
            let owner_internal_cb = self.read_guest_u32(owner.wrapping_add(0x34)).unwrap_or(0);
            let owner_internal_ctx = self.read_guest_u32(owner.wrapping_add(0x38)).unwrap_or(0);
            let texas_complete = is_texas
                && std::env::var("EAPP_TEXAS_ASYNC1_COMPLETE")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
            let sudoku_complete = is_sudoku
                && std::env::var("EAPP_SUDOKU_ASYNC1_COMPLETE")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
            let sims_complete = is_sims
                && fliwheel_var("EAPP_SIMS_ASYNC1_COMPLETE")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
            let royal_complete = is_royal
                && fliwheel_var("EAPP_ROYAL_ASYNC1_COMPLETE")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
            let mspacman_complete = is_mspacman
                && fliwheel_var("EAPP_MSPACMAN_ASYNC1_COMPLETE")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(true);
            info!(
                target: "EAPP_IMPORT",
                "AsyncFileIO:1 owner={:#010x} req={:#010x} req_cb={:#010x} req_ctx={:#010x} owner_cb={:#010x} owner_ctx={:#010x} internal_cb={:#010x} internal_ctx={:#010x} complete={}",
                owner,
                req,
                req_cb,
                req_ctx,
                owner_cb,
                owner_ctx,
                owner_internal_cb,
                owner_internal_ctx,
                (complete && is_tetris
                    || texas_complete
                    || sudoku_complete
                    || sims_complete
                    || royal_complete
                    || mspacman_complete
                    || is_lost) as u8
            );
            if is_vortex && owner != 0 {
                // PR #3's direct runner treats AsyncFileIO:1 as the generic
                // AsyncOp(request=r0). The request callback is stored on the
                // request itself and receives (request, request+0x38); this
                // is the retirement step after Vortex's #2 transfer.
                let callback_pc = self.read_guest_u32(owner.wrapping_add(0x34)).unwrap_or(0);
                let callback_ctx = self.read_guest_u32(owner.wrapping_add(0x38)).unwrap_or(0);
                self.write_guest_u32(owner.wrapping_add(0x20), 0);
                if callback_pc != 0 {
                    self.async_callback_queued_count =
                        self.async_callback_queued_count.wrapping_add(1);
                    self.pending_guest_calls.push_back(PendingGuestCall {
                        pc: callback_pc,
                        arg0: owner,
                        arg1: callback_ctx,
                        arg2: 0,
                        arg3: 0,
                    });
                }
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:1 Vortex completion req={:#010x} cb_pc={:#010x} cb_ctx={:#010x}",
                    owner,
                    callback_pc,
                    callback_ctx
                );
                return 1;
            }
            if is_sims {
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:1 Sims owner_fields owner={:#010x} op={} offset={:#010x} mode={:#010x} dest={:#010x} want={:#010x} status={:#010x} bytes={:#010x} result={:#010x} payload={:#010x}",
                    owner,
                    self.read_guest_u8(owner.wrapping_add(0x04)).unwrap_or(0),
                    self.read_guest_u32(owner.wrapping_add(0x0c)).unwrap_or(0),
                    self.read_guest_u32(owner.wrapping_add(0x10)).unwrap_or(0),
                    self.read_guest_u32(owner.wrapping_add(0x14)).unwrap_or(0),
                    self.read_guest_u32(owner.wrapping_add(0x18)).unwrap_or(0),
                    self.read_guest_u32(owner.wrapping_add(0x20)).unwrap_or(0),
                    self.read_guest_u32(owner.wrapping_add(0x24)).unwrap_or(0),
                    self.read_guest_u32(owner.wrapping_add(0x28)).unwrap_or(0),
                    self.read_guest_u32(owner.wrapping_add(0x2c)).unwrap_or(0),
                );
            }
            if complete && is_tetris && owner != 0 && req != 0 {
                // Ordinal 1 is a no-path/control completion used by initiator C
                // after the audio-stream secondary owner path. Unlike ordinal 0,
                // the linked request object is immediately returned to the load
                // manager's free list and may be reused for the next wav header.
                // The shared 0x1801fc30 helper clears [request+4] only when the
                // forwarded status is non-zero; status 0 leaves [request+4]=2,
                // which makes the next 0x1801fe28 see a busy request and stalls
                // on MoveFail.wav. Use status=1 here to model the control/event
                // completion and clear the in-flight byte before the request is
                // reused. The downstream callback (0x1801d424) ignores r1/r2.
                self.write_guest_u32(owner.wrapping_add(0x20), 1);
                self.write_guest_u32(owner.wrapping_add(0x24), 0);
                self.async_callback_queued_count = self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: 0x1801_fbfc,
                    arg0: owner,
                    arg1: 0,
                    arg2: 0,
                    arg3: 0,
                });
                if self.startup_progress.enabled {
                    info!(
                        target: "EAPP_PROGRESS",
                        "async1_callback_queued frame={} queued={} owner={:#010x} req={:#010x} req_cb={:#010x} pending_async={}",
                        self.frame_counter,
                        self.async_callback_queued_count,
                        owner,
                        req,
                        req_cb,
                        self.async_pending_requests.len()
                    );
                }
            }
            if mspacman_complete && owner != 0 && req != 0 {
                // Ms. PAC-MAN's 0x180169fc path allocates a transient 0x3c
                // owner, links it to the stream request, stores
                // 0x180168f8 as the request callback, and then calls
                // AsyncFileIO:1. The guest trampoline reads status/byte-count
                // from [owner+0x20/0x24], transitions [owner+4], and forwards
                // the completion to the stream callback at [owner+0x0c].
                // Model only that observed ABI; do not complete ordinal 1 for
                // unrelated titles by default.
                let status = fliwheel_var("EAPP_MSPACMAN_ASYNC1_STATUS")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(0);
                let byte_count = fliwheel_var("EAPP_MSPACMAN_ASYNC1_BYTES")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(0);
                self.write_guest_u32(owner.wrapping_add(0x20), status);
                self.write_guest_u32(owner.wrapping_add(0x24), byte_count);
                self.async_callback_queued_count = self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: 0x1801_68f8,
                    arg0: owner,
                    arg1: 0,
                    arg2: 0,
                    arg3: 0,
                });
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:1 MsPAC-MAN completion owner={:#010x} req={:#010x} cb_pc={:#010x} status={} bytes={}",
                    owner,
                    req,
                    0x1801_68f8u32,
                    status,
                    byte_count
                );
            }
            if sudoku_complete && owner != 0 && owner_internal_cb != 0 {
                let status = std::env::var("EAPP_SUDOKU_ASYNC1_STATUS")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(0);
                let byte_count = std::env::var("EAPP_SUDOKU_ASYNC1_BYTES")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(0);
                self.write_guest_u32(owner.wrapping_add(0x20), status);
                self.write_guest_u32(owner.wrapping_add(0x24), byte_count);
                let _ = self.write_guest_bytes(owner.wrapping_add(0x1c), &[0]);
                self.async_callback_queued_count = self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: owner_internal_cb,
                    arg0: owner,
                    arg1: owner_internal_ctx,
                    arg2: 0,
                    arg3: 0,
                });
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:1 Sudoku completion owner={:#010x} cb_pc={:#010x} status={} bytes={}",
                    owner,
                    owner_internal_cb,
                    status,
                    byte_count
                );
            }
            if sims_complete && owner != 0 && owner_internal_cb != 0 {
                let status = fliwheel_var("EAPP_SIMS_ASYNC1_STATUS")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(1);
                let byte_count = fliwheel_var("EAPP_SIMS_ASYNC1_BYTES")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(0);
                self.write_guest_u32(owner.wrapping_add(0x20), status);
                self.write_guest_u32(owner.wrapping_add(0x24), byte_count);
                let _ = self.write_guest_bytes(owner.wrapping_add(0x1c), &[0]);
                self.async_callback_queued_count = self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: owner_internal_cb,
                    arg0: owner,
                    arg1: owner_internal_ctx,
                    arg2: 0,
                    arg3: 0,
                });
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:1 Sims completion owner={:#010x} cb_pc={:#010x} status={} bytes={}",
                    owner,
                    owner_internal_cb,
                    status,
                    byte_count
                );
            }
            if is_lost && owner != 0 && owner_internal_cb != 0 {
                // PR #3's direct HLE treats AsyncFileIO:1 as the generic
                // AsyncOp(request=r0): it publishes status zero and drains
                // request+0x34 with (request, request+0x38). LOST's owner is
                // that request object, so preserve the same ABI here rather
                // than inventing a title-specific trampoline.
                self.write_guest_u32(owner.wrapping_add(0x20), 0);
                self.write_guest_u32(owner.wrapping_add(0x24), 0);
                self.async_callback_queued_count = self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: owner_internal_cb,
                    arg0: owner,
                    arg1: owner_internal_ctx,
                    arg2: 0,
                    arg3: 0,
                });
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:1 LOST completion owner={:#010x} cb_pc={:#010x} status=0 bytes=0",
                    owner,
                    owner_internal_cb
                );
            }
            if royal_complete && owner != 0 && owner_internal_cb != 0 {
                let status = fliwheel_var("EAPP_ROYAL_ASYNC1_STATUS")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(0);
                let byte_count = fliwheel_var("EAPP_ROYAL_ASYNC1_BYTES")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or_else(|| self.read_guest_u32(owner.wrapping_add(0x24)).unwrap_or(0));
                self.write_guest_u32(owner.wrapping_add(0x20), status);
                self.write_guest_u32(owner.wrapping_add(0x24), byte_count);
                let _ = self.write_guest_bytes(owner.wrapping_add(0x1c), &[0]);
                self.async_callback_queued_count = self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: owner_internal_cb,
                    arg0: owner,
                    arg1: status,
                    arg2: byte_count,
                    arg3: owner_internal_ctx,
                });
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:1 Royal completion owner={:#010x} cb_pc={:#010x} status={} bytes={}",
                    owner,
                    owner_internal_cb,
                    status,
                    byte_count
                );
            }
            if texas_complete && owner != 0 && req != 0 && req_cb != 0 {
                // 0x1802c2d0 stores the request callback/context at
                // request+0x0c/+0x10, not on the transient owner. The
                // completion trampoline at 0x1802fcf0 operates on that
                // request object, transitions request+4, and forwards
                // (request, status, byte-count, context) to Hold'em's
                // 0x180054fc callback.
                let status = std::env::var("EAPP_TEXAS_ASYNC1_STATUS")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(0);
                let byte_count = std::env::var("EAPP_TEXAS_ASYNC1_BYTES")
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(0);
                self.async_callback_queued_count = self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: 0x1802_fcf0,
                    arg0: req,
                    arg1: status,
                    arg2: byte_count,
                    arg3: 0,
                });
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:1 Texas completion owner={:#010x} req={:#010x} cb={:#010x} ctx={:#010x} status={} bytes={}",
                    owner,
                    req,
                    req_cb,
                    req_ctx,
                    status,
                    byte_count
                );
            }
            return 1;
        }

        if ordinal == 2 {
            // No-path async/control-object path used by Tetris' audio stream
            // manager after AsyncFileIO:0 has opened a wav. Wrapper 0x1801fe08
            // calls the import with r0/r2 = owner object and the guest has
            // already populated:
            //   [owner+0x34] = completion callback PC
            //   [owner+0x38] = completion callback context (the stream entry)
            // Firmware completion helper 0x18020070 clears the in-flight byte
            // and tail-calls that callback as (owner, context). Queue the same
            // callback only for the env-gated parsed-resource RE path.
            let complete = fliwheel_var("EAPP_ASYNC3_COMPLETE")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false);
            let is_tetris = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("66666"));
            let is_texas = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("33333"));
            let is_sudoku = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("50513"));
            let is_sims = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("1500C") || p.contains("1500E"));
            let is_royal = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("50514"));
            let is_mspacman = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("14004"));
            let is_lost = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("1B200"));
            let is_vortex = self.metadata.title == "12345";
            let owner = args[0];
            let callback_pc = self.read_guest_u32(owner.wrapping_add(0x34)).unwrap_or(0);
            let callback_ctx = self.read_guest_u32(owner.wrapping_add(0x38)).unwrap_or(0);
            if self.metadata.title == "12345" && fliwheel_var_os("EAPP_REQ_DUMP").is_some() {
                let words = (0..16u32)
                    .map(|index| format!("{:08x}", self.read_guest_u32(owner.wrapping_add(index * 4)).unwrap_or(0)))
                    .collect::<Vec<_>>()
                    .join(" ");
                info!(target: "EAPP_IMPORT", "AsyncFileIO:2 Vortex req={:#010x} words={}", owner, words);
            }
            let texas_complete = is_texas
                && std::env::var("EAPP_TEXAS_ASYNC2_COMPLETE")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
            let sudoku_complete = is_sudoku
                && std::env::var("EAPP_SUDOKU_ASYNC2_COMPLETE")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
            let sims_complete = is_sims
                && fliwheel_var("EAPP_SIMS_ASYNC2_COMPLETE")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
            let royal_complete = is_royal
                && fliwheel_var("EAPP_ROYAL_ASYNC2_COMPLETE")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
            let mspacman_complete = is_mspacman
                && fliwheel_var("EAPP_MSPACMAN_ASYNC2_COMPLETE")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(true);
            info!(
                target: "EAPP_IMPORT",
                "AsyncFileIO:2 owner={:#010x} cb_pc={:#010x} cb_ctx={:#010x} complete={}",
                owner,
                callback_pc,
                callback_ctx,
                (complete && is_tetris
                    || texas_complete
                    || sudoku_complete
                    || sims_complete
                    || royal_complete
                    || mspacman_complete
                    || is_lost) as u8
            );
            if is_lost && owner != 0 && callback_pc != 0 {
                // PR #3 maps AsyncFileIO:2 to AsyncRead(request=r0). The
                // request points at the shared file object and the staged
                // payload created by LOST's preceding #0 open is keyed by
                // that object. Reproduce the direct runner's copy, result,
                // and generic (request, context) completion ABI here.
                let req = owner;
                let object = self.read_guest_u32(req.wrapping_add(0x08)).unwrap_or(0);
                let destination = self.read_guest_u32(req.wrapping_add(0x14)).unwrap_or(0);
                let requested = self.read_guest_u32(req.wrapping_add(0x18)).unwrap_or(0) as usize;
                let stream = self.read_guest_u32(req.wrapping_add(0x2c)).unwrap_or(0);
                let staged_range = self.staged_files.get(&object).and_then(|staged| {
                    Some((
                        staged.payload_addr,
                        staged.len as usize,
                        staged.offset,
                        staged.host_path.clone(),
                    ))
                });
                let mut delivered = 0usize;
                if destination != 0 && requested != 0 {
                    if let Some((payload_addr, payload_len, file_offset, host_path)) = staged_range {
                        let source_offset = file_offset.min(payload_len);
                        let available = payload_len.saturating_sub(source_offset);
                        let count = requested.min(available);
                        if let Some(payload) = self.read_guest_bytes(
                            payload_addr.wrapping_add(source_offset as u32),
                            count,
                        ) {
                            if self.write_guest_bytes(destination, &payload) {
                                delivered = count;
                                if let Some(staged) = self.staged_files.get_mut(&object) {
                                    staged.offset = staged.offset.saturating_add(count);
                                }
                                info!(
                                    target: "EAPP_IMPORT",
                                    "AsyncFileIO:2 LOST staged read req={:#010x} object={:#010x} stream={:#010x} source={:#010x}+{} dest={:#010x} requested={} delivered={} path={}",
                                    req,
                                    object,
                                    stream,
                                    payload_addr,
                                    source_offset,
                                    destination,
                                    requested,
                                    count,
                                    host_path.display()
                                );
                            }
                        }
                    }
                }
                self.write_guest_u32(req.wrapping_add(0x24), delivered as u32);
                self.write_guest_u32(req.wrapping_add(0x28), delivered as u32);
                if object != 0 {
                    self.write_guest_u32(object.wrapping_add(0x08), delivered as u32);
                }
                let ok = requested == 0 || delivered == requested;
                self.write_guest_u32(req.wrapping_add(0x20), if ok { 0 } else { u32::MAX });
                self.async_callback_queued_count = self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: callback_pc,
                    arg0: req,
                    arg1: callback_ctx,
                    arg2: 0,
                    arg3: 0,
                });
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:2 LOST completion req={:#010x} cb_pc={:#010x} status={} bytes={}",
                    req,
                    callback_pc,
                    if ok { 0 } else { u32::MAX },
                    delivered
                );
                return 1;
            }
            if is_vortex && owner != 0 && callback_pc != 0 {
                // Vortex uses the same request-object ABI as PR #3's direct
                // AsyncRead, but its shipped request marks the operation as
                // op 3 even though this path is a read. Resolve the staged
                // file by the shared file object, copy the requested bytes,
                // publish both result fields, and drain the request callback.
                let req = owner;
                let object = self.read_guest_u32(req.wrapping_add(0x08)).unwrap_or(0);
                let destination = self.read_guest_u32(req.wrapping_add(0x14)).unwrap_or(0);
                let requested_raw = self.read_guest_u32(req.wrapping_add(0x18)).unwrap_or(0);
                let stream = self.read_guest_u32(req.wrapping_add(0x2c)).unwrap_or(0);
                let staged_range = self.staged_files.get(&object).map(|staged| {
                    (
                        staged.payload_addr,
                        staged.len as usize,
                        staged.offset,
                        staged.host_path.clone(),
                    )
                });
                let mut delivered = 0usize;
                let mut available = 0usize;
                if destination != 0 {
                    if let Some((payload_addr, payload_len, file_offset, host_path)) = staged_range {
                        let source_offset = file_offset.min(payload_len);
                        available = payload_len.saturating_sub(source_offset);
                        let requested = if requested_raw == u32::MAX {
                            available
                        } else {
                            requested_raw as usize
                        };
                        let count = requested.min(available);
                        if let Some(payload) = self.read_guest_bytes(
                            payload_addr.wrapping_add(source_offset as u32),
                            count,
                        ) {
                            if self.write_guest_bytes(destination, &payload) {
                                delivered = count;
                                if let Some(staged) = self.staged_files.get_mut(&object) {
                                    staged.offset = staged.offset.saturating_add(count);
                                }
                                info!(
                                    target: "EAPP_IMPORT",
                                    "AsyncFileIO:2 Vortex staged read req={:#010x} object={:#010x} stream={:#010x} source={:#010x}+{} dest={:#010x} requested_raw={:#010x} delivered={} path={}",
                                    req,
                                    object,
                                    stream,
                                    payload_addr,
                                    source_offset,
                                    destination,
                                    requested_raw,
                                    count,
                                    host_path.display()
                                );
                            }
                        }
                    }
                }
                let requested = if requested_raw == u32::MAX {
                    available
                } else {
                    requested_raw as usize
                };
                let ok = requested == 0 || delivered == requested;
                self.write_guest_u32(req.wrapping_add(0x24), delivered as u32);
                self.write_guest_u32(req.wrapping_add(0x28), delivered as u32);
                if object != 0 {
                    self.write_guest_u32(object.wrapping_add(0x08), delivered as u32);
                }
                self.write_guest_u32(req.wrapping_add(0x20), if ok { 0 } else { u32::MAX });
                self.async_callback_queued_count = self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: callback_pc,
                    arg0: req,
                    arg1: callback_ctx,
                    arg2: 0,
                    arg3: 0,
                });
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:2 Vortex completion req={:#010x} cb_pc={:#010x} status={} bytes={}",
                    req,
                    callback_pc,
                    if ok { 0 } else { u32::MAX },
                    delivered
                );
                return 1;
            }
            if complete && is_tetris && owner != 0 && callback_pc != 0 {
                let _ = self.write_guest_bytes(owner.wrapping_add(0x1c), &[0]);
                self.async_callback_queued_count = self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: callback_pc,
                    arg0: owner,
                    arg1: callback_ctx,
                    arg2: 0,
                    arg3: 0,
                });
                if self.startup_progress.enabled {
                    info!(
                        target: "EAPP_PROGRESS",
                        "async2_callback_queued frame={} queued={} owner={:#010x} cb_pc={:#010x} cb_ctx={:#010x} pending_async={}",
                        self.frame_counter,
                        self.async_callback_queued_count,
                        owner,
                        callback_pc,
                        callback_ctx,
                        self.async_pending_requests.len()
                    );
                }
            }
            let title_async2_complete =
                texas_complete
                    || sudoku_complete
                    || sims_complete
                    || royal_complete
                    || mspacman_complete;
            if title_async2_complete && owner != 0 && callback_pc != 0 {
                // Hold'em's resource callback at 0x18004a14 is entered as
                // (owner, entry). The two pointers are the same object for
                // this AsyncFileIO:2 wrapper; it compares them and then
                // consumes owner+0x20 as the completion status.
                let owner_op = self.read_guest_u8(owner.wrapping_add(0x04)).unwrap_or(0);
                if (sudoku_complete || sims_complete) && owner_op == 5 {
                    // Sudoku's 0x1801b14c path is a seek before the following
                    // type-3 payload read. The guest passes the absolute RLB
                    // position in owner+0x0c; keep the staged host image at
                    // that position so resource reads select the same bytes
                    // as the retail stream manager.
                    let req = self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0);
                    let handle = self.read_guest_u32(owner.wrapping_add(0x2c)).unwrap_or(0);
                    let seek_offset =
                        self.read_guest_u32(owner.wrapping_add(0x0c)).unwrap_or(0) as usize;
                    if let Some(staged) = self.staged_files.get_mut(&req) {
                        if staged.payload_addr == handle {
                            staged.offset = seek_offset.min(staged.len as usize);
                            info!(
                                target: "EAPP_IMPORT",
                                "AsyncFileIO:2 {} seek owner={:#010x} handle={:#010x} offset={}",
                                if sudoku_complete { "Sudoku" } else { "Sims" },
                                owner,
                                handle,
                                staged.offset
                            );
                        }
                    }
                }
                if owner_op == 3 {
                    let destination = self.read_guest_u32(owner.wrapping_add(0x14)).unwrap_or(0);
                    let requested =
                        self.read_guest_u32(owner.wrapping_add(0x18)).unwrap_or(0) as usize;
                    let handle = self.read_guest_u32(owner.wrapping_add(0x2c)).unwrap_or(0);
                    let req = self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0);
                    let staged_range = self.staged_files.get(&req).and_then(|staged| {
                        (staged.payload_addr == handle).then(|| {
                            (
                                staged.payload_addr,
                                staged.len,
                                staged.offset,
                                staged.host_path.clone(),
                            )
                        })
                    });
                    if destination != 0 && requested != 0 {
                        if let Some((payload_addr, payload_len, file_offset, host_path)) =
                            staged_range
                        {
                            let source_offset = file_offset.min(payload_len as usize);
                            let source = payload_addr.wrapping_add(source_offset as u32);
                            let available = (payload_len as usize).saturating_sub(source_offset);
                            let count = requested.min(available);
                            let payload = self.read_guest_bytes(source, count);
                            let delivered = payload
                                .as_deref()
                                .map(|bytes| self.write_guest_bytes(destination, bytes))
                                .unwrap_or(false);
                            if delivered {
                                if let Some(staged) = self.staged_files.get_mut(&req) {
                                    staged.offset = staged.offset.saturating_add(count);
                                }
                                self.write_guest_u32(
                                    callback_ctx.wrapping_add(0x14c),
                                    count as u32,
                                );
                                info!(
                                    target: "EAPP_IMPORT",
                                    "AsyncFileIO:2 staged read owner={:#010x} entry={:#010x} handle={:#010x} source={:#010x}+{} dest={:#010x} requested={} delivered={} path={}",
                                    owner,
                                    callback_ctx,
                                    handle,
                                    source,
                                    source_offset,
                                    destination,
                                    requested,
                                    count,
                                    host_path.display()
                                );
                            }
                        }
                    }
                }
                self.async_callback_queued_count = self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: callback_pc,
                    arg0: owner,
                    arg1: callback_ctx,
                    arg2: 0,
                    arg3: 0,
                });
                info!(
                    target: "EAPP_IMPORT",
                    "AsyncFileIO:2 {} completion owner={:#010x} cb_pc={:#010x}",
                    if sudoku_complete {
                        "Sudoku"
                    } else if sims_complete {
                        "Sims"
                    } else if royal_complete {
                        "Royal"
                    } else if mspacman_complete {
                        "MsPAC-MAN"
                    } else {
                        "Texas"
                    },
                    owner,
                    callback_pc
                );
            }
            return 1;
        }

        let path = self
            .try_read_c_string(args[0], 256)
            .or_else(|| self.try_read_c_string(args[1], 256));
        if let Some(path) = path {
            info!(target: "EAPP_IMPORT", "AsyncFileIO:{} path={}", ordinal, path);
            self.fill_framebuffer(HLE_INFO_FRAMEBUFFER);

            if ordinal == 12 {
                // Direct open-like call used by wrapper 0x6068:
                //   r0 = mode/flags, r1 = path, r2 = file object / out-handle.
                // The wrapper stores the import return in `[file+4]` (status)
                // and passes `[file+0]` to ordinal 14. Therefore the handle
                // belongs in `*r2`, while the return value is a status code.
                if let Some(host_path) = self.resolve_or_create_host_path(&path) {
                    let handle = self.next_async_file_handle;
                    self.next_async_file_handle =
                        self.next_async_file_handle.wrapping_add(1).max(1);
                    if args[2] != 0 {
                        self.write_guest_u32(args[2], handle);
                    }
                    self.async_open_files.insert(handle, host_path.clone());
                    info!(target: "EAPP_IMPORT", "AsyncFileIO:12 opened {} -> handle {} status=0", host_path.display(), handle);
                    return 0;
                }
                warn!(target: "EAPP_IMPORT", "AsyncFileIO:12 missing host path {}", path);
                return 8;
            }

            if ordinal == 0 {
                // Streaming/open-with-owner path used by Tetris' proper parsed
                // boot flow for wav resources. Guest initiator `0x1801fcc8`
                // allocates a 0x3c-byte owner, links it to the request object,
                // then calls AsyncFileIO:0(..., owner=r3). The owner callback
                // at `0x1801fbfc` reads:
                //   [owner+0x20] = status (0 means success for this path)
                //   [owner+0x24] = byte_count
                //   [owner+0x08] = linked request
                // and forwards those values to the request callback. The
                // callback is supplied by the guest in owner+0x34; it is not a
                // fixed address because every EAPP binary has its own code
                // layout. Keep whole-file completion title-scoped: Tetris,
                // Texas Hold'em, Royal Solitaire, Sudoku, and The Sims remain
                // opt-in, while Ms. PAC-MAN's measured contract is enabled by
                // default. LOST follows the PR #3 direct-runner contract too:
                // its options.sav probe completes and invokes the owner
                // callback before the render loop can advance to the next
                // asynchronous asset.
                let owner = args[3];
                let is_lost = self
                    .metadata
                    .bundle_dir
                    .to_str()
                    .map_or(false, |p| p.contains("1B200"));
                let is_tetris = self
                    .metadata
                    .bundle_dir
                    .to_str()
                    .map_or(false, |p| p.contains("66666"));
                let is_texas = self
                    .metadata
                    .bundle_dir
                    .to_str()
                    .map_or(false, |p| p.contains("33333"));
                let is_sudoku = self
                    .metadata
                    .bundle_dir
                    .to_str()
                    .map_or(false, |p| p.contains("50513"));
                let is_sims = self
                    .metadata
                    .bundle_dir
                    .to_str()
                    .map_or(false, |p| p.contains("1500C") || p.contains("1500E"));
                let is_royal = self
                    .metadata
                    .bundle_dir
                    .to_str()
                    .map_or(false, |p| p.contains("50514"));
                let is_mspacman = self
                    .metadata
                    .bundle_dir
                    .to_str()
                    .map_or(false, |p| p.contains("14004"));
                let is_vortex = self.metadata.title == "12345";
                let tetris_complete = is_tetris
                    && fliwheel_var("EAPP_ASYNC3_COMPLETE")
                        .map(|v| v == "1" || v == "true")
                        .unwrap_or(false);
                let texas_complete = is_texas
                    && std::env::var("EAPP_TEXAS_ASYNC0_COMPLETE")
                        .map(|v| v == "1" || v == "true")
                        .unwrap_or(false);
                let sudoku_complete = is_sudoku
                    && std::env::var("EAPP_SUDOKU_ASYNC0_COMPLETE")
                        .map(|v| v == "1" || v == "true")
                        .unwrap_or(false);
                let sims_complete = is_sims
                    && fliwheel_var("EAPP_SIMS_ASYNC0_COMPLETE")
                        .map(|v| v == "1" || v == "true")
                        .unwrap_or(false);
                let royal_complete = is_royal
                    && fliwheel_var("EAPP_ROYAL_ASYNC0_COMPLETE")
                        .map(|v| v == "1" || v == "true")
                        .unwrap_or(false);
                let mspacman_complete = is_mspacman
                    && fliwheel_var("EAPP_MSPACMAN_ASYNC0_COMPLETE")
                        .map(|v| v == "1" || v == "true")
                        .unwrap_or(true);
                let complete = is_lost
                    || is_vortex
                    || tetris_complete
                    || texas_complete
                    || sudoku_complete
                    || sims_complete
                    || royal_complete
                    || mspacman_complete;
                let callback_pc = self.read_guest_u32(owner.wrapping_add(0x34)).unwrap_or(0);
                if let Some(host_path) = self.resolve_or_create_host_path(&path) {
                    self.note_audio_asset_path(&host_path);
                    let lost_handle = if is_lost {
                        let handle = self.next_async_file_handle;
                        self.next_async_file_handle =
                            self.next_async_file_handle.wrapping_add(1).max(1);
                        self.async_open_files.insert(handle, host_path.clone());
                        Some(handle)
                    } else {
                        None
                    };
                    let vortex_handle = if is_vortex {
                        let handle = self.next_async_file_handle;
                        self.next_async_file_handle =
                            self.next_async_file_handle.wrapping_add(1).max(1);
                        self.async_open_files.insert(handle, host_path.clone());
                        Some(handle)
                    } else {
                        None
                    };
                    let bytes = fs::read(&host_path).unwrap_or_else(|e| {
                        warn!(
                            target: "EAPP_IMPORT",
                            "AsyncFileIO:0 read error for {}: {}",
                            host_path.display(),
                            e
                        );
                        Vec::new()
                    });
                    let n = bytes.len() as u32;
                    info!(
                        target: "EAPP_IMPORT",
                        "AsyncFileIO:0 stream path={} owner={:#010x} bytes={} complete={} callback={:#010x}",
                        host_path.display(),
                        owner,
                        n,
                        complete as u8,
                        callback_pc
                    );
                    if complete && owner != 0 && callback_pc != 0 {
                        // AsyncFileIO:0 is a whole-file load, not just a
                        // completion notification. The guest owner callback
                        // copies [owner+0x2c] into [request+0] before it calls
                        // the request callback; that pointer is the loaded
                        // RLB/WAV payload. Keep the payload in synthetic app
                        // RAM for the guest to parse after completion.
                        // Parsed-resource startup can reopen the same RLB for
                        // several stream objects. Reuse an existing staged
                        // image instead of consuming another 16 MiB guest-RAM
                        // region for every open.
                        let existing_stage = self
                            .staged_files
                            .values()
                            .filter(|staged| staged.len == n && staged.host_path == host_path)
                            .max_by_key(|staged| staged.generation)
                            .map(|staged| (staged.payload_addr, staged.offset));
                        let payload_addr = existing_stage
                            .map(|(payload_addr, _)| payload_addr)
                            .unwrap_or_else(|| self.alloc_zeroed(n));
                        let payload_offset = existing_stage.map(|(_, offset)| offset).unwrap_or(0);
                        let payload_written = existing_stage.is_some()
                            || (payload_addr != 0 && self.write_guest_bytes(payload_addr, &bytes));
                        if payload_written {
                            // LOST uses the same generic AsyncOpen contract as
                            // PR #3: +0x2c is the small stream handle. Keep the
                            // staged payload separately for its later #2 read;
                            // the guest completion callback copies +0x2c into
                            // the shared file object's descriptor slot.
                            self.write_guest_u32(
                                owner.wrapping_add(0x2c),
                                lost_handle
                                    .or(vortex_handle)
                                    .unwrap_or(payload_addr),
                            );
                            let req = self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0);
                            if req != 0 {
                                self.staged_file_generation =
                                    self.staged_file_generation.wrapping_add(1);
                                self.staged_files.insert(
                                    req,
                                    StagedFile {
                                        generation: self.staged_file_generation,
                                        payload_addr,
                                        len: n,
                                        offset: payload_offset,
                                        host_path: host_path.clone(),
                                    },
                                );
                            }
                            info!(
                                target: "EAPP_IMPORT",
                                "AsyncFileIO:0 staged {} bytes at {:#010x}",
                                n,
                                payload_addr
                            );
                        } else {
                            warn!(
                                target: "EAPP_IMPORT",
                                "AsyncFileIO:0 could not stage {} bytes for {}",
                                n,
                                host_path.display()
                            );
                        }
                        if is_lost {
                            // PR #3's AsyncOpen publishes the operation result
                            // in the file object. For a bufferless open this is
                            // the file size, while the stream handle remains in
                            // the request's +0x2c field above.
                            let object = self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0);
                            let buf = self.read_guest_u32(owner.wrapping_add(0x14)).unwrap_or(0);
                            let len = self.read_guest_u32(owner.wrapping_add(0x18)).unwrap_or(0);
                            if object != 0 {
                                self.write_guest_u32(object.wrapping_add(0x08), n);
                            }
                            if buf == 0 || len == 0 {
                                self.write_guest_u32(owner.wrapping_add(0x24), 0);
                            }
                            info!(
                                target: "EAPP_IMPORT",
                                "AsyncFileIO:0 LOST generic open owner={:#010x} handle={:#010x} result_bytes={} object={:#010x}",
                                owner,
                                lost_handle.unwrap_or(0),
                                n,
                                object
                            );
                        }
                        if is_vortex {
                            // Bufferless Vortex opens publish the stream id in
                            // the request and the file size as the operation
                            // result in the shared file object. The guest open
                            // completion then builds its next #2 request from
                            // those fields. PR #3 invokes this callback as
                            // (request, context), with status/count remaining
                            // in the request object rather than in r1/r2.
                            let object = self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0);
                            if object != 0 {
                                self.write_guest_u32(object.wrapping_add(0x08), n);
                            }
                            self.write_guest_u32(owner.wrapping_add(0x24), 0);
                            info!(
                                target: "EAPP_IMPORT",
                                "AsyncFileIO:0 Vortex generic open owner={:#010x} handle={:#010x} size={} object={:#010x}",
                                owner,
                                vortex_handle.unwrap_or(0),
                                n,
                                object
                            );
                        }
                        if let Some(result_spec) = fliwheel_var_os("EAPP_ASYNC0_RESULT") {
                            let result_spec = result_spec.to_string_lossy();
                            let result = if result_spec == "length" {
                                Some(n)
                            } else if let Some(hex) = result_spec.strip_prefix("0x") {
                                u32::from_str_radix(hex, 16).ok()
                            } else {
                                result_spec.parse::<u32>().ok()
                            };
                            if let (Some(result), Some(req)) =
                                (result, self.read_guest_u32(owner.wrapping_add(0x08)))
                            {
                                if req != 0 {
                                    self.write_guest_u32(req.wrapping_add(0x08), result);
                                    info!(
                                        target: "EAPP_IMPORT",
                                        "AsyncFileIO:0 experimental result={} req={:#010x}",
                                        result,
                                        req
                                    );
                                }
                            }
                        }
                        // The Sims preload reuses the owner/context object that
                        // handled the initial save-file probe. Retail clears most
                        // of that object's request fields before opening
                        // gameLib.rlb, but the observed callback reads +0x174 as
                        // the resource result. Keep a title-scoped override for
                        // testing the pointer-valued resource handle without
                        // changing the default HLE path.
                        if sims_complete {
                            if let Some(result_spec) =
                                fliwheel_var_os("EAPP_SIMS_ASYNC0_OWNER_RESULT")
                            {
                                let result_spec = result_spec.to_string_lossy();
                                let result = if result_spec == "length" {
                                    Some(n)
                                } else if result_spec == "payload" {
                                    Some(payload_addr)
                                } else if let Some(hex) = result_spec.strip_prefix("0x") {
                                    u32::from_str_radix(hex, 16).ok()
                                } else {
                                    result_spec.parse::<u32>().ok()
                                };
                                let request =
                                    self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0);
                                let resource_context = request.checked_sub(0x16c).unwrap_or(0);
                                if let (Some(result), true) = (result, resource_context != 0) {
                                    self.write_guest_u32(
                                        resource_context.wrapping_add(0x174),
                                        result,
                                    );
                                    info!(
                                        target: "EAPP_IMPORT",
                                        "AsyncFileIO:0 Sims owner result override owner={:#010x} context={:#010x} result={:#010x}",
                                        owner,
                                        resource_context,
                                        result
                                    );
                                }
                            }
                        }
                        // Royal Solitaire's RLB preload uses the same
                        // manager callback shape, but its guest callback
                        // reads the payload pointer from context+0x174 before
                        // the manager callback copies it to +0x11c. The
                        // retail request initializes that field to -1; keep
                        // the pointer handoff opt-in until the post-RLB scene
                        // is independently verified.
                        if royal_complete {
                            if let Some(result_spec) =
                                fliwheel_var_os("EAPP_ROYAL_ASYNC0_OWNER_RESULT")
                            {
                                let result_spec = result_spec.to_string_lossy();
                                let result = if result_spec == "length" {
                                    Some(n)
                                } else if result_spec == "payload" {
                                    Some(payload_addr)
                                } else if let Some(hex) = result_spec.strip_prefix("0x") {
                                    u32::from_str_radix(hex, 16).ok()
                                } else {
                                    result_spec.parse::<u32>().ok()
                                };
                                let request =
                                    self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0);
                                let resource_context = request.checked_sub(0x16c).unwrap_or(0);
                                if let (Some(result), true) = (result, resource_context != 0) {
                                    self.write_guest_u32(
                                        resource_context.wrapping_add(0x174),
                                        result,
                                    );
                                    info!(
                                        target: "EAPP_IMPORT",
                                        "AsyncFileIO:0 Royal owner result override owner={:#010x} context={:#010x} result={:#010x}",
                                        owner,
                                        resource_context,
                                        result
                                    );
                                }
                            }
                        }
                        // Hold'em uses the same ordinal for its first-run save probe and
                        // ordinary resource loads.  A missing save should report an error,
                        // while a resource such as constant.blob must still report success;
                        // applying one status to every ordinal-0 request makes the latter
                        // look missing and leaves its object tables uninitialized.
                        let texas_missing_save = host_path.to_str().map_or(false, |p| {
                            p.contains(SAVE_DIR) || p.contains(LEGACY_SAVE_DIR)
                        });
                        let status = if texas_complete && texas_missing_save {
                            std::env::var("EAPP_TEXAS_ASYNC0_STATUS")
                                .ok()
                                .and_then(|v| v.parse::<u32>().ok())
                                .unwrap_or(0)
                        } else {
                            0
                        };
                        self.write_guest_u32(owner.wrapping_add(0x20), status);
                        self.write_guest_u32(owner.wrapping_add(0x24), n);
                        let req = self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0);
                        self.async_callback_queued_count =
                            self.async_callback_queued_count.wrapping_add(1);
                        self.pending_guest_calls.push_back(PendingGuestCall {
                            pc: callback_pc,
                            arg0: owner,
                            arg1: if is_lost || is_vortex { 0 } else { status },
                            arg2: if is_lost || is_vortex { 0 } else { n },
                            arg3: 0,
                        });
                        if self.startup_progress.enabled {
                            info!(
                                target: "EAPP_PROGRESS",
                                "async0_callback_queued frame={} queued={} owner={:#010x} req={:#010x} bytes={} cb_pc={:#010x} pending_async={}",
                                self.frame_counter,
                                self.async_callback_queued_count,
                                owner,
                                req,
                                n,
                                callback_pc,
                                self.async_pending_requests.len()
                            );
                        }
                    }
                    return 1;
                }
                warn!(target: "EAPP_IMPORT", "AsyncFileIO:0 missing host path {}", path);
                return 0;
            }

            if ordinal == 3 {
                let req = args[2];
                self.async_request_count = self.async_request_count.wrapping_add(1);
                if req != 0 {
                    self.async_pending_requests.insert(req);
                }
                self.dump_request_object(req);
                if self.metadata.title == "12345" && req != 0 {
                    // PR #3 uses AsyncFileIO:3 as the same generic async-open
                    // service as #0. Vortex supplies a destination buffer for
                    // these requests and, with its load-on-open default, the
                    // whole asset is delivered before the callback runs. The
                    // request still needs a stream handle at +0x2c and the
                    // operation result (bytes loaded) in both +0x24 and the
                    // shared file object at +0x08.
                    let object = self.read_guest_u32(req.wrapping_add(0x08)).unwrap_or(0);
                    let dest = self.read_guest_u32(req.wrapping_add(0x14)).unwrap_or(0);
                    let want = self.read_guest_u32(req.wrapping_add(0x18)).unwrap_or(0);
                    let callback_pc = self.read_guest_u32(req.wrapping_add(0x34)).unwrap_or(0);
                    let callback_ctx = self.read_guest_u32(req.wrapping_add(0x38)).unwrap_or(0);
                    if let Some(host_path) = self.resolve_or_create_host_path(&path) {
                        self.note_audio_asset_path(&host_path);
                        let handle = self.next_async_file_handle;
                        self.next_async_file_handle =
                            self.next_async_file_handle.wrapping_add(1).max(1);
                        self.async_open_files.insert(handle, host_path.clone());
                        self.write_guest_u32(req.wrapping_add(0x2c), handle);
                        match fs::read(&host_path) {
                            Ok(bytes) => {
                                let requested_len = if want != 0 {
                                    want as usize
                                } else {
                                    bytes.len()
                                };
                                let n = bytes.len().min(requested_len);
                                let delivered = dest != 0 && self.write_guest_bytes(dest, &bytes[..n]);
                                let got = if delivered { n as u32 } else { 0 };
                                if dest != 0 && want != 0 {
                                    self.write_guest_u32(req.wrapping_add(0x24), got);
                                    if object != 0 {
                                        self.write_guest_u32(object.wrapping_add(0x08), got);
                                    }
                                } else {
                                    // Keep bufferless #3 opens usable by a later
                                    // Vortex #2 read, matching the #0 path.
                                    let payload_addr = self.alloc_zeroed(bytes.len() as u32);
                                    if payload_addr != 0 && self.write_guest_bytes(payload_addr, &bytes) {
                                        self.staged_file_generation =
                                            self.staged_file_generation.wrapping_add(1);
                                        self.staged_files.insert(
                                            object,
                                            StagedFile {
                                                generation: self.staged_file_generation,
                                                payload_addr,
                                                len: bytes.len() as u32,
                                                offset: 0,
                                                host_path: host_path.clone(),
                                            },
                                        );
                                    }
                                    if object != 0 {
                                        self.write_guest_u32(object.wrapping_add(0x08), bytes.len() as u32);
                                    }
                                    self.write_guest_u32(req.wrapping_add(0x24), 0);
                                }
                                self.write_guest_u32(req.wrapping_add(0x20), 0);
                                info!(
                                    target: "EAPP_IMPORT",
                                    "AsyncFileIO:3 Vortex open path={} req={:#010x} object={:#010x} handle={:#010x} buf={:#010x} want={} got={} cb_pc={:#010x}",
                                    host_path.display(),
                                    req,
                                    object,
                                    handle,
                                    dest,
                                    want,
                                    got,
                                    callback_pc
                                );
                            }
                            Err(e) => {
                                self.write_guest_u32(req.wrapping_add(0x20), u32::MAX);
                                self.write_guest_u32(req.wrapping_add(0x24), 0);
                                warn!(
                                    target: "EAPP_IMPORT",
                                    "AsyncFileIO:3 Vortex read error for {}: {}",
                                    host_path.display(),
                                    e
                                );
                            }
                        }
                        if callback_pc != 0 {
                            self.async_callback_queued_count =
                                self.async_callback_queued_count.wrapping_add(1);
                            self.pending_guest_calls.push_back(PendingGuestCall {
                                pc: callback_pc,
                                arg0: req,
                                arg1: callback_ctx,
                                arg2: 0,
                                arg3: 0,
                            });
                        } else {
                            self.async_pending_requests.remove(&req);
                        }
                        return 1;
                    }
                    self.async_pending_requests.remove(&req);
                    warn!(target: "EAPP_IMPORT", "AsyncFileIO:3 Vortex missing host path {}", path);
                    return 0;
                }
                if let Some(host_path) = self.resolve_or_create_host_path(&path) {
                    self.note_audio_asset_path(&host_path);
                    // Request-object protocol (observed):
                    //   [req+0x14] = guest-provided destination buffer
                    //   [req+0x18] = expected byte count
                    //   [req+0x34] = completion callback pc
                    //   [req+0x38] = completion callback context
                    // We are the I/O layer, so we fill the guest's buffer.
                    let dest = self.read_guest_u32(req.wrapping_add(0x14)).unwrap_or(0);
                    let want = self.read_guest_u32(req.wrapping_add(0x18)).unwrap_or(0);
                    let callback_pc = self.read_guest_u32(req.wrapping_add(0x34)).unwrap_or(0);
                    let callback_ctx = self.read_guest_u32(req.wrapping_add(0x38)).unwrap_or(0);
                    if self.startup_progress.enabled {
                        info!(
                            target: "EAPP_PROGRESS",
                            "async_request frame={} count={} req={:#010x} dest={:#010x} want={} cb_pc={:#010x} cb_ctx={:#010x} path={}",
                            self.frame_counter,
                            self.async_request_count,
                            req,
                            dest,
                            want,
                            callback_pc,
                            callback_ctx,
                            path
                        );
                    }
                    match fs::read(&host_path) {
                        Ok(bytes) => {
                            let requested_len = if want != 0 {
                                want as usize
                            } else {
                                bytes.len()
                            };
                            let n = bytes.len().min(requested_len);
                            let is_sims = self
                                .metadata
                                .bundle_dir
                                .to_str()
                                .map_or(false, |p| p.contains("1500C") || p.contains("1500E"));
                            let zero_fill_sims_rserver = is_sims
                                && fliwheel_var_os("EAPP_SIMS_ZERO_FILL_RSERVER").is_some()
                                && host_path
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .map_or(false, |name| name == "rserver.bin");
                            // Request-object reads provide a destination and a capacity; the
                            // firmware copies the bytes actually read and reports the byte count.
                            // Do NOT zero-fill the full requested capacity here. Texas Hold'em
                            // requests large font buffers whose capacity overlaps the following
                            // heap request object; memset-style short-read filling wipes [req+8]
                            // and the callback PC before the completion trampoline runs.
                            let should_deliver = if fliwheel_var_os("EAPP_SKIP_RSERVER").is_some() {
                                // When set, skip loading rserver.bin so the game keeps its
                                // original code at 0x10001038 (used by Lost to test if the
                                // built-in fixed-function engine renders).
                                !host_path
                                    .to_str()
                                    .map_or(false, |p| p.contains("rserver.bin"))
                            } else {
                                true
                            };
                            let bytes_delivered = should_deliver
                                && dest != 0
                                && self.write_guest_bytes(dest, &bytes[..n]);
                            if bytes_delivered && zero_fill_sims_rserver && requested_len > n {
                                let padding = vec![0u8; requested_len - n];
                                let padding_delivered = self.write_guest_bytes(
                                    dest.wrapping_add(n as u32),
                                    &padding,
                                );
                                info!(
                                    target: "EAPP_IMPORT",
                                    "AsyncFileIO:3 Sims rserver zero-fill requested={} file_bytes={} padding={} delivered={}",
                                    requested_len,
                                    n,
                                    padding.len(),
                                    padding_delivered
                                );
                            }
                            let delivered = dest != 0 && (bytes_delivered || !should_deliver);
                            if delivered {
                                // The async completion callback `0x1801fc68`
                                // is a thin trampoline: it reads
                                //   r5 = [req+0x20]  (status)
                                //   r6 = [req+0x24]  (byte_count)
                                //   r4 = [req+0x08]  (OWNER object)
                                // and tail-calls `0x1801fc94`, which marks the
                                // owner done ([owner+8]=-1, [owner+4]=0) and
                                // then tail-calls the OWNER's own callback
                                //   [owner+0x0c](owner, status, byte_count, [owner+0x10])
                                // It is the owner callback that consumes
                                // status/byte_count.
                                //
                                // Writing (status=1, byte_count=N) here made
                                // Tetris parse `Strings.dta` (labels
                                // materialized) but stalled the loader on the
                                // legal/loading screen (iteration 10
                                // regression, reverted in iteration 11). The
                                // full owner-callback ABI is still being
                                // reversed; default stays at 0 (golden). Set
                                // FLIWHEEL_EAPP_ASYNC3_COMPLETE=1 to re-enable
                                // the iteration-10 behavior for RE.
                                let is_tetris = self
                                    .metadata
                                    .bundle_dir
                                    .to_str()
                                    .map_or(false, |p| p.contains("66666"));
                                let is_texas = self
                                    .metadata
                                    .bundle_dir
                                    .to_str()
                                    .map_or(false, |p| p.contains("33333"));
                                let is_popcap =
                                    self.metadata.bundle_dir.to_str().map_or(false, |p| {
                                        p.contains("44444") || p.contains("55555")
                                    });
                                let is_mspacman = self
                                    .metadata
                                    .bundle_dir
                                    .to_str()
                                    .map_or(false, |p| p.contains("14004"));
                                let is_pacman = self
                                    .metadata
                                    .bundle_dir
                                    .to_str()
                                    .map_or(false, |p| p.contains("AAAAA"));
                                let tetris_complete = is_tetris
                                    && fliwheel_var("EAPP_ASYNC3_COMPLETE")
                                        .map(|v| v == "1" || v == "true")
                                        .unwrap_or(false);
                                let texas_complete = is_texas
                                    && std::env::var("EAPP_TEXAS_ASYNC3_COMPLETE")
                                        .map(|v| v == "1" || v == "true")
                                        .unwrap_or(false);
                                // PopCap's resource table derives the final
                                // asset length from the byte count delivered
                                // to the async callback. Its .ro reads are a
                                // normal successful request (status=0), so
                                // report the actual short-read length by
                                // default. Keep this scoped to PopCap .ro
                                // resources: audio and preference requests
                                // still use their separately proven contracts.
                                let popcap_resource_read = is_popcap
                                    && host_path.extension().map_or(false, |ext| ext == "ro");
                                let mspacman_resource_read = is_mspacman
                                    && fliwheel_var("EAPP_MSPACMAN_ASYNC3_COMPLETE")
                                        .map(|v| v == "1" || v == "true")
                                        .unwrap_or(true);
                                // PAC-MAN's texture completion trampoline reads the
                                // request status and byte count before handing the
                                // loaded TGA to its resource owner. The measured
                                // contract reaches the guest's own TGA parser and
                                // the first playable maze, so enable it for the
                                // title's normal path.
                                let pacman_texture_read = is_pacman
                                    && host_path.extension().map_or(false, |ext| ext == "tga");
                                let complete =
                                    tetris_complete
                                        || texas_complete
                                        || popcap_resource_read
                                        || mspacman_resource_read
                                        || pacman_texture_read;
                                // LOST uses the same request completion callback as the
                                // reference runner's direct HLE path, but its callback consumes
                                // the transfer count through the owner object rather than through
                                // the generic resource cases above.  The subsequent render-server
                                // setup passes that value as OpenGLES:164's length argument.  If
                                // it is left at the request initializer's 0xffffffff sentinel,
                                // the game reaches the parser but treats the server image as
                                // invalid and never enters its ordinary draw path.
                                let lost_rserver_read = self.metadata.title == "1B200"
                                    && host_path
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .map_or(false, |name| name == "rserver.bin");
                                if complete {
                                    let status = if texas_complete {
                                        std::env::var("EAPP_TEXAS_ASYNC3_STATUS")
                                            .ok()
                                            .and_then(|v| v.parse::<u32>().ok())
                                            .unwrap_or(0)
                                    } else if popcap_resource_read
                                        || mspacman_resource_read
                                        || pacman_texture_read
                                    {
                                        0
                                    } else {
                                        1
                                    };
                                    self.write_guest_u32(req.wrapping_add(0x20), status);
                                    self.write_guest_u32(req.wrapping_add(0x24), n as u32);
                                }
                                if lost_rserver_read {
                                    self.write_guest_u32(req.wrapping_add(0x20), 0);
                                    self.write_guest_u32(req.wrapping_add(0x24), n as u32);
                                    let owner = self.read_guest_u32(req.wrapping_add(0x08)).unwrap_or(0);
                                    if owner != 0 {
                                        // LOST's 0x1803c814 completion callback returns the
                                        // owner pointer in r0 and the byte count in r2, while
                                        // the owner itself is read by the game's loader as the
                                        // completed operation result at +8.
                                        self.write_guest_u32(owner.wrapping_add(0x08), n as u32);
                                    }
                                    info!(
                                        target: "EAPP_IMPORT",
                                        "AsyncFileIO:3 LOST rserver result req={:#010x} owner={:#010x} status=0 bytes={}",
                                        req,
                                        owner,
                                        n
                                    );
                                }
                                info!(
                                    target: "EAPP_IMPORT",
                                    "AsyncFileIO:3 loaded {} ({} bytes, requested {}) -> guest dest {:#010x}",
                                    host_path.display(),
                                    n,
                                    requested_len,
                                    dest
                                );
                                self.staged_file_generation =
                                    self.staged_file_generation.wrapping_add(1);
                                self.staged_files.insert(
                                    req,
                                    StagedFile {
                                        generation: self.staged_file_generation,
                                        payload_addr: dest,
                                        len: n as u32,
                                        offset: 0,
                                        host_path: host_path.clone(),
                                    },
                                );
                            } else {
                                warn!(
                                    target: "EAPP_IMPORT",
                                    "AsyncFileIO:3 no dest buffer for {} (want {} bytes)",
                                    host_path.display(),
                                    want
                                );
                            }
                        }
                        Err(e) => {
                            warn!(
                                target: "EAPP_IMPORT",
                                "AsyncFileIO:3 read error for {}: {}",
                                host_path.display(),
                                e
                            );
                        }
                    }
                    info!(target: "EAPP_IMPORT", "AsyncFileIO:3 resolved={}", host_path.display());
                    if callback_pc != 0 {
                        self.async_callback_queued_count =
                            self.async_callback_queued_count.wrapping_add(1);
                        self.pending_guest_calls.push_back(PendingGuestCall {
                            pc: callback_pc,
                            arg0: req,
                            arg1: callback_ctx,
                            arg2: 0,
                            arg3: 0,
                        });
                        if self.startup_progress.enabled {
                            info!(
                                target: "EAPP_PROGRESS",
                                "async_callback_queued frame={} queued={} req={:#010x} cb_pc={:#010x} pending_async={}",
                                self.frame_counter,
                                self.async_callback_queued_count,
                                req,
                                callback_pc,
                                self.async_pending_requests.len()
                            );
                        }
                    } else {
                        self.async_pending_requests.remove(&req);
                    }
                    return 1;
                }
                self.async_pending_requests.remove(&req);
                warn!(target: "EAPP_IMPORT", "AsyncFileIO:3 missing host path {}", path);
                return 0;
            }

            if ordinal == 7 {
                // Directory enumeration: list subdirectories of the given path.
                // Uses the same request-object protocol as ordinal 3:
                //   r3 = request object with callback at [r3+0x34]/[r3+0x38]
                //   [r3+0x04] = max entries (game pre-allocates for this many)
                //   [r3+0x10] = operation type (1 = directory enum)
                // The completion callback receives (req, ctx) after the enumeration.
                // We write status=1 (success) and count=N to the request object,
                // then queue the callback.
                //
                // The game's callback at 0x1804862c reads the result from
                // the callback context (0x180fb264), which is a game-internal
                // pack-list structure. The callback fills in the pack data
                // and the game's main loop can then access it.
                let req = args[3];
                if req != 0 {
                    let cb_pc = self.read_guest_u32(req.wrapping_add(0x34)).unwrap_or(0);
                    let cb_ctx = self.read_guest_u32(req.wrapping_add(0x38)).unwrap_or(0);

                    if let Some(host_path) = self.resolve_or_create_host_path(&path) {
                        let mut entries = Vec::new();
                        if let Ok(read_dir) = fs::read_dir(&host_path) {
                            for entry in read_dir.flatten() {
                                let full = host_path.join(entry.file_name());
                                let is_dir =
                                    fs::metadata(&full).map(|m| m.is_dir()).unwrap_or(false);
                                if is_dir {
                                    if let Some(name) = entry.file_name().to_str() {
                                        entries.push(name.to_string());
                                    }
                                }
                            }
                        }
                        entries.sort();
                        let count = entries.len();

                        // Store pack names in the EappBus so the game can
                        // query them later via subsequent AsyncFileIO calls.
                        self.async_dir_entries = entries.clone();

                        // Set completion status and count in the request object.
                        // Status 0 = success (matching the \"golden\" ordinal-3 convention).
                        self.write_guest_u32(req.wrapping_add(0x20), 0); // status = success
                        self.write_guest_u32(req.wrapping_add(0x24), count as u32); // result = entry count

                        // Queue async completion callback
                        if cb_pc != 0 {
                            self.async_callback_queued_count =
                                self.async_callback_queued_count.wrapping_add(1);
                            self.pending_guest_calls.push_back(PendingGuestCall {
                                pc: cb_pc,
                                arg0: req,
                                arg1: cb_ctx,
                                arg2: 0,
                                arg3: 0,
                            });
                        }

                        info!(target: "EAPP_IMPORT", "AsyncFileIO:7 enum {} -> {} entries: {:?} cb_pc={:#010x}", host_path.display(), count, entries, cb_pc);
                        return 1; // request accepted
                    }
                }
                warn!(target: "EAPP_IMPORT", "AsyncFileIO:7 missing host path {}", path);
                return 0;
            }

            return 1;
        }
        0
    }

    /// Calendar/local-time ABI used by Tetris' menu clock. The game calls
    /// `miscTBD:12(out_tm, ...)`, then reads `out_tm[1] + 60 * out_tm[2]` as
    /// minutes since midnight before passing that scalar to its `H:MM AM/PM`
    /// Handle `Filesytem` (sic) import module used by iQuiz/TWA, Bejeweled and
    /// some other games. On real iPod hardware this provides filesystem
    /// open/read/close. The ABI is object-backed: ordinal 0 receives the
    /// caller's file object in `r3` and stores the synthetic handle at
    /// `[r3+0]`; ordinal 2 receives that handle plus a buffer, length and
    /// `&bytes_read`; ordinal 1 closes the handle.
    fn handle_filesystem_import(&mut self, ordinal: u32, args: [u32; 4]) -> u32 {
        // For ordinal 0, r1 is a path. For read/close calls this normally
        // returns None because r1 is a buffer or an unused argument, but it
        // remains useful in the trace when a title passes a string there.
        let path_str = self.try_read_c_string(args[1], 256);
        info!(target: "EAPP_IMPORT", "Filesytem:{} pc={:#010x} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} path={:?}",
            ordinal, self.bus.pending_pc, args[0], args[1], args[2], args[3], path_str);
        match ordinal {
            // Ordinal 0: filesystem open/init. r1=path, r2=flags, r3=file
            // object. The real implementation writes its handle into the
            // first word of that object; Bejeweled immediately feeds it to
            // Filesytem:2, so leaving this word zero traps the title in its
            // resource-loading spinner.
            0 => {
                let Some(path) = path_str.as_deref() else {
                    warn!(target: "EAPP_IMPORT", "Filesytem:0 called without a path");
                    return 1;
                };
                let Some(host_path) = self.resolve_or_create_host_path(path) else {
                    warn!(target: "EAPP_IMPORT", "Filesytem:0 could not resolve path {:?}", path);
                    return 1;
                };
                if !host_path.is_file() {
                    warn!(target: "EAPP_IMPORT", "Filesytem:0 path is not a file: {}", host_path.display());
                    return 1;
                }

                let handle = self.next_filesystem_handle;
                self.next_filesystem_handle = self.next_filesystem_handle.wrapping_add(1).max(1);
                self.filesystem_open_files.insert(
                    handle,
                    FilesystemFile {
                        host_path: host_path.clone(),
                        offset: 0,
                    },
                );
                let object = args[3];
                let wrote_handle = object != 0 && self.write_guest_u32(object, handle);
                info!(target: "EAPP_IMPORT", "Filesytem:0 opened {} -> handle {} object={:#010x} wrote_handle={} status=0", host_path.display(), handle, object, wrote_handle);
                0
            }
            // Ordinal 1: close the handle stored at the head of the file
            // object. The caller may pass zero during teardown; that is a
            // harmless no-op on the firmware path as well.
            1 => {
                let handle = args[0];
                let closed = self.filesystem_open_files.remove(&handle).is_some();
                info!(target: "EAPP_IMPORT", "Filesytem:1 close handle={} closed={}", handle, closed);
                0
            }
            // Ordinal 2: sequential read(handle, buffer, length, &bytes_read).
            // The returned status is ignored by the game's wrapper; the
            // output count is what determines whether a complete read landed.
            2 => {
                let handle = args[0];
                let buffer = args[1];
                let length = args[2] as usize;
                let out = args[3];
                let Some(file) = self.filesystem_open_files.get(&handle).cloned() else {
                    if out != 0 {
                        self.write_guest_u32(out, 0);
                    }
                    warn!(target: "EAPP_IMPORT", "Filesytem:2 unknown handle={} buffer={:#010x} len={}", handle, buffer, length);
                    return 0;
                };
                let bytes = fs::read(&file.host_path).unwrap_or_else(|error| {
                    warn!(target: "EAPP_IMPORT", "Filesytem:2 read error for {}: {}", file.host_path.display(), error);
                    Vec::new()
                });
                let start = file.offset.min(bytes.len());
                let count = length.min(bytes.len().saturating_sub(start));
                let delivered =
                    buffer != 0 && self.write_guest_bytes(buffer, &bytes[start..start + count]);
                let got = if delivered || count == 0 {
                    count as u32
                } else {
                    0
                };
                if got != 0 {
                    if let Some(open) = self.filesystem_open_files.get_mut(&handle) {
                        open.offset = start + got as usize;
                    }
                }
                if out != 0 {
                    self.write_guest_u32(out, got);
                }
                info!(target: "EAPP_IMPORT", "Filesytem:2 read handle={} buffer={:#010x} len={} -> {} bytes", handle, buffer, length, got);
                0
            }
            _ => {
                warn!(target: "EAPP_IMPORT", "unhandled Filesytem ordinal {}", ordinal);
                0
            }
        }
    }

    /// formatter. Recovered layout: six u32 fields (`sec, min, hour, mday,
    /// mon0, year_since_1900`). Tetris passes a stack slot with room for these
    /// six words; writing a full 9-word C `struct tm` would overwrite the
    /// caller's saved registers.
    fn handle_misc12_local_time(&mut self, args: [u32; 4]) -> u32 {
        let out = args[0];
        if out == 0 {
            return 0;
        }
        let now = Local::now();
        let fields = [
            now.second() as u32,
            now.minute() as u32,
            now.hour() as u32,
            now.day() as u32,
            now.month0() as u32,
            (now.year() - 1900) as u32,
        ];
        for (idx, val) in fields.iter().copied().enumerate() {
            let _ = self.write_guest_u32(out.wrapping_add((idx as u32) * 4), val);
        }
        if self.startup_progress.enabled {
            info!(
                target: "EAPP_PROGRESS",
                "local_time module=miscTBD ordinal=12 out={:#010x} sec={} min={} hour={} mday={} mon0={} year={}",
                out,
                fields[0],
                fields[1],
                fields[2],
                fields[3],
                fields[4],
                fields[5]
            );
        }
        1
    }

    fn handle_misc9_time_api(&mut self, args: [u32; 4]) -> u32 {
        self.misc9_time_diag_count = self.misc9_time_diag_count.wrapping_add(1);
        let before = self.read_guest_u32(args[0]).unwrap_or(0xffff_ffff);
        let host_us = if let Some(step) = self.misc9_fixed_step {
            self.misc9_fixed_clock = self.misc9_fixed_clock.wrapping_add(step);
            self.misc9_fixed_clock as u64
        } else {
            self.host_start.elapsed().as_micros() as u64
        };
        let guest_tick = host_us as u32;
        let wrote = args[0] != 0 && self.write_guest_u32(args[0], guest_tick);
        let after = self.read_guest_u32(args[0]).unwrap_or(0xffff_ffff);
        let guest_time_advances = self
            .misc9_last_pointed_value
            .map(|prev| prev != after)
            .unwrap_or(false);
        self.misc9_last_pointed_value = Some(after);
        // The direct PR #3 runner returns the generated tick in R0 as well as
        // storing it through the pointer. Keep the historical pointer return
        // in wall-clock mode for compatibility, but match the oracle exactly
        // when its fixed-clock comparison mode is enabled.
        let ret = if self.misc9_fixed_step.is_some() {
            guest_tick
        } else {
            args[0]
        };
        let log_limit = fliwheel_var_os("EAPP_TIME_DIAG_LIMIT")
            .and_then(|v| v.to_string_lossy().parse::<u64>().ok())
            .unwrap_or(80);
        if self.startup_progress.enabled && self.misc9_time_diag_count <= log_limit {
            info!(
                target: "EAPP_PROGRESS",
                "time_api module=miscTBD ordinal=9 frame={} call={} args=[{:#010x},{:#010x},{:#010x},{:#010x}] pointed_before={:#010x} pointed_after={:#010x} ret={:#010x} host_us={} guest_time_advances={} writes_guest_time={}",
                self.frame_counter,
                self.misc9_time_diag_count,
                args[0],
                args[1],
                args[2],
                args[3],
                before,
                after,
                ret,
                host_us,
                guest_time_advances,
                wrote
            );
        }
        ret
    }

    fn maybe_log_startup_progress(&mut self) {
        if !self.startup_progress.enabled {
            return;
        }
        let frame = self.frame_counter;
        let fb_hash = self.render_state_hash();
        let hash_changed = self
            .startup_progress
            .last_framebuffer_hash
            .map(|prev| prev != fb_hash)
            .unwrap_or(false);
        if hash_changed && self.startup_progress.first_hash_change_frame.is_none() {
            self.startup_progress.first_hash_change_frame = Some(frame);
        }
        self.startup_progress.last_framebuffer_hash = Some(fb_hash);

        let should_log = frame <= 10
            || frame % self.startup_progress.interval == 0
            || hash_changed
            || self.startup_progress.logged < 10;
        if !should_log || self.startup_progress.logged >= self.startup_progress.max_logs {
            return;
        }
        self.startup_progress.logged += 1;

        let app_time_current = self
            .read_guest_u32(self.app_object.wrapping_add(4))
            .unwrap_or(0);
        let app_time_delta = self
            .read_guest_u32(self.app_object.wrapping_add(8))
            .unwrap_or(0);
        let frame_state = self.read_guest_u8(self.frame_context).unwrap_or(0xff);
        let frame_event_mask = self
            .read_guest_u32(self.frame_context.wrapping_add(0x20))
            .unwrap_or(0);
        let app_event_head = self
            .read_guest_u32(self.app_object.wrapping_add(0x30))
            .unwrap_or(0);
        let app_event_preview = self.preview_event_list(app_event_head, 4);
        let splash_base = 0x1802_56bc;
        let splash_phase = self.read_guest_u8(splash_base).unwrap_or(0xff);
        let splash_timeout_a = self
            .read_guest_u32(splash_base.wrapping_add(4))
            .unwrap_or(0);
        let splash_timeout_b = self
            .read_guest_u32(splash_base.wrapping_add(8))
            .unwrap_or(0);
        let splash_flags = self
            .read_guest_u32(splash_base.wrapping_add(0x14))
            .unwrap_or(0);
        let splash_time_a = self
            .read_guest_u32(splash_base.wrapping_add(0x18))
            .unwrap_or(0);
        let splash_time_b = self
            .read_guest_u32(splash_base.wrapping_add(0x1c))
            .unwrap_or(0);
        let splash_time_c = self
            .read_guest_u32(splash_base.wrapping_add(0x20))
            .unwrap_or(0);
        // State-machine RE (iter 17): the legal→menu gate. `0x18004088` returns
        // 2 if `[*0x18025678] < 3`, else returns 5 if the byte `[*0x18025674] != 0`,
        // else tail-calls into the clock object at `*0x180255d4`. So the 1→5
        // transition (legal screen → menu) requires BOTH conditions.
        let statemach_count = self.read_guest_u32(0x1802_5678).unwrap_or(0xff_ffff);
        let statemach_byte = self.read_guest_u8(0x1802_5674).unwrap_or(0xff);
        // Clock-obj audio gate (iter 19 RE). byte `[clock_obj+0x2c]` is the audio
        // queue pointer (NULL on default boot -> blocks caller 3 / 4 byte-setter
        // chains). byte `[clock_obj+0x54]` is a "ready" flag set by caller 4
        // after firing the byte-setter (re-gates caller 3). clock_obj = 0x1005f710.
        let clock_audio_queue = self.read_guest_u32(0x1005f710 + 0x2c).unwrap_or(0);
        let clock_ready_byte = self.read_guest_u8(0x1005f710 + 0x54).unwrap_or(0xff);
        // RE: caller-3 chain (`0x1b8b4`) sums [clock_obj+0x5c] += sl each frame,
        // compares to threshold [clock_obj+0x60]. sum<=threshold → take the byte-setter
        // gate path; sum>threshold takes the overflow path. iter 19 saw both audio
        // gates pass but byte still not set, so the threshold gate is the
        // likely remaining blocker. Also log [clock_obj+0x6c] frame counter.
        let clock_sum2 = self.read_guest_u32(0x1005f710 + 0x5c).unwrap_or(0);
        let clock_threshold = self.read_guest_u32(0x1005f710 + 0x60).unwrap_or(0);
        let clock_count = self.read_guest_u32(0x1005f710 + 0x6c).unwrap_or(0);
        // RE: caller-3 (`0x1b8b4` → `0x1b630`) reads slot indexes [clock_obj+0xa8]
        // and [clock_obj+0xac], then loads a state byte from [clock_obj+idx*16+0x8c].
        // That byte's bits 0x10 and 0x08 must be set for the byte-setter tail
        // call at `0x1b814: b 0x18005034` to fire.
        let clock_idx_a8 = self.read_guest_u32(0x1005f710 + 0xa8).unwrap_or(0);
        let clock_idx_ac = self.read_guest_u32(0x1005f710 + 0xac).unwrap_or(0);
        let clock_slot_addr_a8 = 0x1005f710u32
            .wrapping_add(clock_idx_a8.wrapping_mul(16))
            .wrapping_add(0x8c);
        let clock_slot_addr_ac = 0x1005f710u32
            .wrapping_add(clock_idx_ac.wrapping_mul(16))
            .wrapping_add(0x8c);
        let clock_slot_byte_a8 = self.read_guest_u8(clock_slot_addr_a8).unwrap_or(0);
        let clock_slot_byte_ac = self.read_guest_u8(clock_slot_addr_ac).unwrap_or(0);
        // RE (iter 20): caller-3 (`0x1b630`) tail-calls `0x90c0(r0=audio_queue)` which
        // returns `[audio_queue+0x14]`. Then `cmp r0, #100; poplt` returns early WITHOUT
        // setting the byte. So the byte-setter `0x18005034` only fires when
        // `[[clock_obj+0x2c]+0x14] >= 100`. This is the real audio-event counter.
        let clock_audio_bytecount: u32 = {
            if clock_audio_queue != 0 {
                self.read_guest_u32(clock_audio_queue + 0x14).unwrap_or(0)
            } else {
                0
            }
        };
        // RE (iter 20): AFTER the byte/bit gate at 0x1b7ac passes, caller-3
        // accumulates `[0x18025580] += r1` each frame (r1=[sp+40] = per-frame
        // delta = approximately per-frame ms or audio samples). If the
        // accumulator >= 2000 then byte-setter `0x18005034` fires (after a few
        // more sub-gates). [0x18025580] = [state_struct+0x34] = `[r4+0x34]`.
        let audio_accumulator = self.read_guest_u32(0x1802_5580).unwrap_or(0);
        // Env-gated diagnostic: write byte 1 to 0x18025674 once per progress
        // interval to prove the gate theory. No guest writer is observed in
        // the binary for this byte, so the value is normally static 0.
        //
        // `FLIWHEEL_EAPP_TEST_READY_DELAY=N` defers the byte write until frame
        // N (so the legal screen can dwell before the legal→menu advance).
        // Default delay is 0 (write on the first emission, matching iter 17).
        let test_ready = fliwheel_var("EAPP_TEST_READY")
            .ok()
            .as_deref()
            .map(|s| s == "1")
            .unwrap_or(false);
        let test_ready_delay: u64 = fliwheel_var("EAPP_TEST_READY_DELAY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if test_ready && frame >= test_ready_delay {
            // Use u32 write (overwrites 4 bytes; adjacent bytes stay 0).
            let _ = self.write_guest_u32(0x1802_5674, 1);
        }
        // RE (iter 20): caller-3 gate `[clock_obj+idx*16+0x8c] & 0x10 || ...0x08`.
        // The audio slot state byte is never written by guest code, so the bit
        // is always 0 and the byte-setter `0x18005034` is never reached via the
        // natural audio-completion path. Env-gated injection used for RE
        // only; observe whether state advances past 1 when both slots get
        // a bit set. The literal value can be overridden via
        // `FLIWHEEL_EAPP_AUDIO_SLOT_BIT_VAL=0xNN` (default 0x10).
        let slot_bit: u32 = fliwheel_var("EAPP_AUDIO_SLOT_BIT_VAL")
            .ok()
            .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0x10);
        if fliwheel_var_os("EAPP_AUDIO_SLOT_BIT").is_some() {
            // clock_obj=0x1005f710. slot 0 == clock_obj+0x8c, slot 1 == clock_obj+0x9c.
            // Inject only the low byte (0x10) by writing a u32 (0x10) at the
            // target address; upper 3 bytes are 0 (matches the static value).
            let _ = self.write_guest_u32(0x1005_f79c, slot_bit);
            let _ = self.write_guest_u32(0x1005_f7ac, slot_bit);
            // RE (iter 20): `0x1b630` calls `0x5a64` which COPIES the field
            // at `0x18025eb0` to `[slot+0x8c]` (overwriting any value we
            // inject here). So inject the byte at the source: `0x18025eb0`.
            let _ = self.write_guest_u32(0x1802_5eb0, slot_bit);
        }
        let import_summary = self.format_frame_import_counts(8);
        let trace_summary = if string_trace_enabled() && !self.string_trace_hits.is_empty() {
            let mut entries: Vec<(u32, u32)> = self
                .string_trace_hits
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect();
            entries.sort_by_key(|&(pc, _)| pc);
            entries
                .iter()
                .map(|(pc, c)| format!("{:#010x}={}", pc, c))
                .collect::<Vec<_>>()
                .join(",")
        } else {
            String::new()
        };
        info!(
            target: "EAPP_PROGRESS",
            "startup_progress frame={} host_us={} fb_hash={:#018x} hash_changed={} first_hash_change={:?} app_time_current={} app_time_delta={} frame_state={} frame_event_mask={:#010x} app_event_head={:#010x} app_events=[{}] splash_phase={} splash_flags={:#010x} splash_timeout_a={} splash_timeout_b={} splash_times=[{},{},{}] statemach_count={} statemach_byte={} clock_audio_queue={:#010x} clock_ready_byte={} clock_sum2={} clock_threshold={} clock_count={} clock_idx_a8={} clock_idx_ac={} clock_slot_byte_a8={} clock_slot_byte_ac={} clock_audio_bytecount={} audio_accumulator={} async=req:{} queued:{} callbacks:{} pending:{} staged:{} imports=[{}] trace=[{}]",
            frame,
            self.host_start.elapsed().as_micros() as u64,
            fb_hash,
            hash_changed,
            self.startup_progress.first_hash_change_frame,
            app_time_current,
            app_time_delta,
            frame_state,
            frame_event_mask,
            app_event_head,
            app_event_preview,
            splash_phase,
            splash_flags,
            splash_timeout_a,
            splash_timeout_b,
            splash_time_a,
            splash_time_b,
            splash_time_c,
            statemach_count,
            statemach_byte,
            clock_audio_queue,
            clock_ready_byte,
            clock_sum2,
            clock_threshold,
            clock_count,
            clock_idx_a8,
            clock_idx_ac,
            clock_slot_byte_a8,
            clock_slot_byte_ac,
            clock_audio_bytecount,
            audio_accumulator,
            self.async_request_count,
            self.async_callback_queued_count,
            self.guest_callback_invocation_count,
            self.async_pending_requests.len(),
            self.staged_files.len(),
            import_summary,
            trace_summary
        );

        // Flush the write-watchpoint log (if any). The CLI's `--timeout` may
        // SIGTERM the process before end-of-run drain fires, so emitting at
        // every startup_progress frame ensures watch captures survive.
        // Entries are unsampled and tagged with the writer PC; callers can
        // correlate them to the splash_phase / splash_times / dispatch-state
        // fields above by frame.
        self.drain_watch_log();
    }

    /// The Sims frame loop waits for a title-specific input-manager status byte
    /// to become zero before it leaves its resource-loading state. The retail
    /// image initializes that byte to 2, while the currently emulated input
    /// path does not produce the observed completion write.
    /// Keep a one-shot override behind an explicit, Sims-only probe flag so
    /// default execution remains unchanged while the next scene contract is
    /// investigated.
    fn maybe_apply_sims_input_ready(&mut self) {
        let is_sims = self
            .metadata
            .bundle_dir
            .to_str()
            .map_or(false, |p| p.contains("1500C") || p.contains("1500E"));
        let enabled = is_sims
            && fliwheel_var("EAPP_SIMS_INPUT_READY")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false);
        if !enabled {
            return;
        }

        let start_frame = fliwheel_var("EAPP_SIMS_INPUT_READY_FRAME")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0);
        if self.frame_counter < start_frame {
            return;
        }

        let input_status = match self.metadata.title.as_str() {
            // Bowling's input manager global.
            "1500C" => 0x1807_380c,
            // Pool is a relocated build; its matching input-manager global
            // is referenced by the same shared helper at 0x180051b4.
            "1500E" => 0x1808_5bac,
            _ => return,
        };
        let before = self.read_guest_u8(input_status).unwrap_or(0xff);
        if before == 2 {
            let wrote = self.write_guest_bytes(input_status, &[0]);
            info!(
                target: "EAPP",
                "Sims input readiness probe title={} addr={:#010x} status={:#04x} -> 0 wrote={}",
                self.metadata.title,
                input_status,
                before,
                wrote
            );
        }
    }

    fn render_state_hash(&self) -> u64 {
        let frame = self.render_state.lock().unwrap();
        let mut hasher = DefaultHasher::new();
        frame.hash(&mut hasher);
        hasher.finish()
    }

    fn read_guest_words(&mut self, addr: u32, count: usize) -> Vec<u32> {
        if addr == 0 {
            return Vec::new();
        }
        (0..count)
            .map(|i| {
                let a = addr.wrapping_add((i * 4) as u32);
                self.read_guest_u32(a).unwrap_or(0xffff_ffff)
            })
            .collect()
    }

    fn read_guest_words_exact(&mut self, addr: u32, count: usize) -> Option<Vec<u32>> {
        if addr == 0 {
            return None;
        }
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let a = addr.wrapping_add((i * 4) as u32);
            out.push(self.read_guest_u32(a)?);
        }
        Some(out)
    }

    fn preview_words(&mut self, addr: u32, count: usize) -> String {
        self.read_guest_words(addr, count)
            .into_iter()
            .map(|w| format!("{:#010x}", w))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn preview_event_list(&mut self, mut head: u32, limit: usize) -> String {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..limit {
            if head == 0 || !seen.insert(head) {
                break;
            }
            let b0 = self.read_guest_u8(head).unwrap_or(0xff);
            let b1 = self.read_guest_u8(head.wrapping_add(1)).unwrap_or(0xff);
            let next = self.read_guest_u32(head.wrapping_add(8)).unwrap_or(0);
            out.push(format!(
                "{:#010x}:b0={} b1={} next={:#010x}",
                head, b0, b1, next
            ));
            head = next;
        }
        out.join("|")
    }

    fn format_frame_import_counts(&self, limit: usize) -> String {
        let mut counts: Vec<_> = self
            .frame_import_counts
            .iter()
            .filter(|((module, _), _)| module != "OpenGLES")
            .collect();
        counts.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        counts
            .into_iter()
            .take(limit)
            .map(|((module, ordinal), count)| format!("{}:{}={}", module, ordinal, count))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn fill_framebuffer(&mut self, color: u32) {
        let mut frame = self.render_state.lock().unwrap();
        frame.fill(color);
    }

    fn handle_bootstrap_return(&mut self) {
        match self.bootstrap_phase {
            BootstrapPhase::Entry => {
                let entry_r0 = self.cpu.reg_get(self.cpu.mode(), 0);
                let entry_r1 = self.cpu.reg_get(self.cpu.mode(), 1);
                let entry_r2 = self.cpu.reg_get(self.cpu.mode(), 2);
                let entry_r3 = self.cpu.reg_get(self.cpu.mode(), 3);
                let entry_r1_preview = self.preview_words(entry_r1, 12);
                let is_lost = self.metadata.title == "1B200";
                let context_prepared = self.app_object != 0;
                if context_prepared {
                    // LOST's shared context was prepared before the initial
                    // vector call so it could receive non-null arguments.
                } else if is_lost {
                    // PR #3's direct runner allocates its shared context from the
                    // RetailOS heap and passes manager+0x100 as the second argument.
                    // LOST only needs the first 0x400-byte context window here; using
                    // the same object for both arguments also preserves that ABI.
                    self.app_object = self.alloc_game_heap(0x400);
                    self.frame_context = self.app_object.wrapping_add(0x100);
                } else {
                    self.app_object = self.alloc_zeroed(0x2000);
                    self.frame_context = self.alloc_zeroed(0x80);
                }
                info!(
                    target: "EAPP",
                    "bootstrap entry returned; entry_ret=[{:#010x},{:#010x},{:#010x},{:#010x}] entry_r1_words=[{}] app_object={:#010x} frame_context={:#010x} aux={:#010x}",
                    entry_r0,
                    entry_r1,
                    entry_r2,
                    entry_r3,
                    entry_r1_preview,
                    self.app_object,
                    self.frame_context,
                    self.header.aux_addr
                );
                if is_lost && !context_prepared && self.header.entry_addr != 0 {
                    // The reference lifecycle calls vector 0 once before the
                    // vector at aux_addr becomes the repeating frame callback.
                    // The terminate vector at header.init_addr is deliberately not
                    // called here; it is vector 1 in this bundle.
                    let _ = self.write_guest_bytes(self.app_object, &[5]);
                    self.cpu.reg_set(self.cpu.mode(), 0, self.app_object);
                    self.cpu
                        .reg_set(self.cpu.mode(), 1, self.frame_context);
                    self.cpu.reg_set(self.cpu.mode(), 2, 0);
                    self.cpu.reg_set(self.cpu.mode(), 3, 0);
                    self.bootstrap_phase = BootstrapPhase::Init;
                    self.cpu
                        .reg_set(self.cpu.mode(), reg::PC, self.header.entry_addr);
                    self.cpu
                        .reg_set(self.cpu.mode(), reg::LR, BOOTSTRAP_RETURN_PC);
                    return;
                }
                let run_init_vectors = fliwheel_var_os("EAPP_INIT_VECTORS").is_some();
                if self.header.init_addr != 0 && run_init_vectors {
                    // RetailOS supplies valid context pointers for each lifecycle vector.  The
                    // entry vector's return registers are not the init vector's arguments.
                    let init_context0 = self.alloc_zeroed(0x400);
                    let init_context1 = self.alloc_zeroed(0x400);
                    self.cpu.reg_set(self.cpu.mode(), 0, init_context0);
                    self.cpu.reg_set(self.cpu.mode(), 1, init_context1);
                    self.cpu.reg_set(self.cpu.mode(), 2, 0);
                    self.cpu.reg_set(self.cpu.mode(), 3, 0);
                    self.bootstrap_phase = BootstrapPhase::Init;
                    self.cpu
                        .reg_set(self.cpu.mode(), reg::PC, self.header.init_addr);
                    self.cpu
                        .reg_set(self.cpu.mode(), reg::LR, BOOTSTRAP_RETURN_PC);
                    return;
                }
                if self.header.init_addr != 0 {
                    info!(
                        target: "EAPP",
                        "bootstrap init vector skipped; set FLIWHEEL_EAPP_INIT_VECTORS=1 to enable"
                    );
                }
                self.finish_bootstrap_entry();
            }
            BootstrapPhase::Init => {
                info!(target: "EAPP", "bootstrap init vector returned");
                self.finish_bootstrap_entry();
            }
            BootstrapPhase::Running => {
                self.frame_counter = self.frame_counter.wrapping_add(1);
                if self
                    .stop_frame
                    .is_some_and(|stop_frame| self.frame_counter >= stop_frame)
                {
                    self.halted = true;
                    self.drain_watch_log();
                    return;
                }
                if self.frame_counter == 1 || self.frame_counter % 600 == 0 {
                    info!(
                        target: "EAPP",
                        "frame {} returned r0={:#010x}",
                        self.frame_counter,
                        self.cpu.reg_get(self.cpu.mode(), 0)
                    );
                }
                self.maybe_apply_sims_input_ready();
                self.maybe_log_startup_progress();
                self.frame_import_counts.clear();
                if !self.dispatch_pending_guest_call() {
                    self.queue_next_frame();
                }
            }
            BootstrapPhase::Done => {
                self.halted = true;
            }
        }
    }

    fn finish_bootstrap_entry(&mut self) {
        // Vortex-specific surface preallocation to prevent null pointer crashes.
        // Must run before first guest frame to set up container structures.
        if self
            .metadata
            .bundle_dir
            .to_str()
            .map_or(false, |p| p.contains("12345"))
        {
            if fliwheel_var_os("VORTEX_PR3_GL165").is_some() {
                info!(target: "EAPP", "VORTEX: using PR #3 ordinal-165 contract");
            } else {
                info!(target: "EAPP", "VORTEX: detected bundle, running preallocation");
                self.vortex_preallocate_surfaces();
                // Verify the write.
                let verify = self.read_guest_u32(WORK_RAM_BASE + 0xff0).unwrap_or(0xdead);
                info!(target: "EAPP", "VORTEX: verification read of WORK_RAM+0xff0 = {:#010x}", verify);
            }
        }
        self.bootstrap_phase = BootstrapPhase::Running;
        self.queue_next_frame();
        self.fill_framebuffer(HLE_INFO_FRAMEBUFFER);
    }

    fn queue_next_frame(&mut self) {
        let frame_context = if self.metadata.title == "1B200" {
            // PR #3 measures RetailOS passing one manager object in r0 and
            // its frame context at manager+0x100 in r1 for LOST. Keep the
            // established independent context used by the other titles.
            self.app_object.wrapping_add(0x100)
        } else {
            self.frame_context
        };
        // LOST's frame vector uses the first argument's +0 byte as a two-state reason
        // handshake. The reference runner's measured path presents reason 0 for the
        // one-time initialization call, then refreshes it to 1 before every normal
        // frame; leaving our zeroed context untouched strands the title in its
        // present-only path and it never reaches its ordinary GL draws.
        if self.metadata.title == "1B200" {
            // PR #3 calls vector 4 once with its seeded context before the
            // scripted frame loop. The first real frame then uses reason 0,
            // followed by reason 1 for the steady-state path.
            let reason = match self.frame_counter {
                0 => 5,
                1 => 0,
                _ => 1,
            };
            let _ = self.write_guest_bytes(self.app_object, &[reason]);
        }
        self.cpu.reg_set(self.cpu.mode(), 0, self.app_object);
        self.cpu.reg_set(self.cpu.mode(), 1, frame_context);
        // The direct HLE writes all four EABI arguments before every vector
        // call. Entry returns can leave r2/r3 holding import-call scratch
        // values, so clear them before the repeating vector rather than
        // leaking the entry vector's register state into the first frame.
        self.cpu.reg_set(self.cpu.mode(), 2, 0);
        self.cpu.reg_set(self.cpu.mode(), 3, 0);
        self.cpu
            .reg_set(self.cpu.mode(), reg::LR, BOOTSTRAP_RETURN_PC);
        self.cpu
            .reg_set(self.cpu.mode(), reg::PC, self.header.aux_addr);
    }

    fn dispatch_pending_guest_call(&mut self) -> bool {
        if self.guest_callback_batch_remaining == 0 {
            self.guest_callback_batch_remaining = self.pending_guest_calls.len();
        }
        if self.guest_callback_batch_remaining == 0 {
            return false;
        }
        if let Some(call) = self.pending_guest_calls.pop_front() {
            self.guest_callback_batch_remaining = self
                .guest_callback_batch_remaining
                .saturating_sub(1);
            self.guest_callback_invocation_count =
                self.guest_callback_invocation_count.wrapping_add(1);
            self.async_pending_requests.remove(&call.arg0);
            if self.startup_progress.enabled {
                info!(
                    target: "EAPP_PROGRESS",
                    "callback_dispatch frame={} count={} pc={:#010x} arg0={:#010x} arg1={:#010x} arg2={:#010x} arg3={:#010x} pending_async={}",
                    self.frame_counter,
                    self.guest_callback_invocation_count,
                    call.pc,
                    call.arg0,
                    call.arg1,
                    call.arg2,
                    call.arg3,
                    self.async_pending_requests.len()
                );
            } else {
                debug!(
                    target: "EAPP",
                    "dispatching guest callback pc={:#010x} arg0={:#010x} arg1={:#010x} arg2={:#010x} arg3={:#010x}",
                    call.pc,
                    call.arg0,
                    call.arg1,
                    call.arg2,
                    call.arg3
                );
            }
            self.cpu.reg_set(self.cpu.mode(), 0, call.arg0);
            self.cpu.reg_set(self.cpu.mode(), 1, call.arg1);
            self.cpu.reg_set(self.cpu.mode(), 2, call.arg2);
            self.cpu.reg_set(self.cpu.mode(), 3, call.arg3);
            self.cpu
                .reg_set(self.cpu.mode(), reg::LR, GUEST_CALLBACK_RETURN_PC);
            self.cpu.reg_set(self.cpu.mode(), reg::PC, call.pc);
            return true;
        }
        false
    }

    fn handle_guest_callback_return(&mut self) {
        if self.guest_callback_batch_remaining > 0 && self.dispatch_pending_guest_call() {
            return;
        }
        // Any callbacks appended while the just-finished batch was running
        // stay queued until the next guest frame. This prevents a completion
        // callback chain from recursively consuming the whole async workload
        // in one frame.
        self.guest_callback_batch_remaining = 0;
        self.queue_next_frame();
    }

    fn alloc_zeroed(&mut self, len: u32) -> u32 {
        let len = (len + 0xf) & !0xf;
        let addr = self.next_alloc;
        let end = addr.saturating_add(len);
        if end <= WORK_RAM_BASE + WORK_RAM_SIZE as u32 {
            self.next_alloc = end;
            self.alloc_sizes.insert(addr, len);
            addr
        } else {
            0
        }
    }

    /// Allocate a zeroed RetailOS-style heap block. The reference runner
    /// reserves an eight-byte size header immediately before the pointer it
    /// returns; LOST's game allocator uses the returned backing block to
    /// create its own sub-arenas and expects the high heap address space.
    fn alloc_game_heap(&mut self, want: u32) -> u32 {
        let size = (want + 7) & !7;
        let total = size.saturating_add(8);
        let block = self.game_heap_next;
        let end = block.saturating_add(total);
        if end > GAME_HEAP_BASE + GAME_HEAP_SIZE as u32 {
            return 0;
        }
        self.game_heap_next = end;
        let _ = self.write_guest_u32(block, total);
        let ptr = block + 8;
        self.alloc_sizes.insert(ptr, size);
        ptr
    }

    fn read_guest_u8(&mut self, addr: u32) -> Option<u8> {
        self.bus.r8(addr).ok()
    }

    fn read_guest_u32(&mut self, addr: u32) -> Option<u32> {
        self.bus.r32(addr).ok()
    }

    /// Read `len` bytes of guest memory. Returns None on any unmapped byte so
    /// callers can log+skip malformed pointers without panicking.
    fn read_guest_bytes(&mut self, addr: u32, len: usize) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(self.bus.r8(addr.wrapping_add(i as u32)).ok()?);
        }
        Some(out)
    }

    fn read_fixed_array_indices(
        &mut self,
        guest_ptr: u32,
        components: usize,
        stride_bytes: usize,
        indices: &[usize],
    ) -> Option<Vec<(f32, f32)>> {
        let tight_stride = components * 4;
        let stride = if stride_bytes == 0 {
            tight_stride
        } else {
            stride_bytes.max(tight_stride)
        };
        let mut pts = Vec::with_capacity(indices.len());
        for &index in indices {
            let start = index.checked_mul(stride)?;
            let bytes =
                self.read_guest_bytes(guest_ptr.wrapping_add(start as u32), tight_stride)?;
            if bytes.len() < tight_stride || bytes.len() < 8 {
                return None;
            }
            let x =
                decode_fixed_16_16(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
            let y = if components >= 2 {
                decode_fixed_16_16(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]))
            } else {
                0.0
            };
            pts.push((x, y));
        }
        Some(pts)
    }

    /// Decode `vertex_count` vertices of `components` signed-16.16 fixed-point
    /// components each from guest memory, honoring the client-array stride.
    /// Returns the (x, y) of each vertex (extra components beyond 2 are ignored
    /// for 2D rasterization). Used for ordinal-137 position (4 comps) and UV
    /// (2 comps) arrays.
    fn read_fixed_array_range(
        &mut self,
        guest_ptr: u32,
        components: usize,
        stride_bytes: usize,
        first_vertex: usize,
        vertex_count: usize,
    ) -> Option<Vec<(f32, f32)>> {
        let tight_stride = components * 4;
        let stride = if stride_bytes == 0 {
            tight_stride
        } else {
            stride_bytes.max(tight_stride)
        };
        let start = first_vertex.checked_mul(stride)?;
        let total = vertex_count.checked_mul(stride)?;
        let bytes = self.read_guest_bytes(guest_ptr.wrapping_add(start as u32), total)?;
        let mut pts = Vec::with_capacity(vertex_count);
        for v in 0..vertex_count {
            let base = v * stride;
            let x = decode_fixed_16_16(u32::from_le_bytes([
                bytes[base],
                bytes[base + 1],
                bytes[base + 2],
                bytes[base + 3],
            ]));
            let y = if components >= 2 {
                decode_fixed_16_16(u32::from_le_bytes([
                    bytes[base + 4],
                    bytes[base + 5],
                    bytes[base + 6],
                    bytes[base + 7],
                ]))
            } else {
                0.0
            };
            pts.push((x, y));
        }
        Some(pts)
    }

    fn write_guest_u32(&mut self, addr: u32, val: u32) -> bool {
        self.bus.w32(addr, val).is_ok()
    }

    fn write_guest_bytes(&mut self, addr: u32, bytes: &[u8]) -> bool {
        for (i, &b) in bytes.iter().enumerate() {
            if self.bus.w8(addr.wrapping_add(i as u32), b).is_err() {
                return false;
            }
        }
        true
    }

    fn observe_audio_guest_pc(&mut self, pc: u32) {
        // The current resource-indexed wrapper contract is proven only for
        // Tetris (66666). Other games place unrelated code at these same
        // virtual addresses and would otherwise produce fabricated audio
        // events with pointer values interpreted as resource IDs.
        if self.metadata.title != "66666" {
            return;
        }
        match pc {
            0x1800_5498 => {
                // The registration wrapper has the actual resource type and
                // index before it loads the active channel byte for Audio:0.
                // Keep this outside the diagnostic string trace so event
                // routing works in normal desktop runs too.
                let mode = self.cpu.mode();
                self.audio_last_registration =
                    Some((self.cpu.reg_get(mode, 0), self.cpu.reg_get(mode, 1)));
            }
            0x1800_3eb8 => {
                // This wrapper is the guest's resource-indexed consumer. It
                // invokes Audio:13/14/15/2 as one logical sound trigger; queue
                // one host event here instead of four duplicate imports.
                let mode = self.cpu.mode();
                let resource_type = self.cpu.reg_get(mode, 0);
                let resource_index = self.cpu.reg_get(mode, 1);
                let host_path = self
                    .audio_resource_paths
                    .get(&(resource_type, resource_index))
                    .cloned();
                self.queue_audio_event(resource_type, resource_index, host_path);
            }
            _ => {}
        }
    }

    fn queue_audio_event(
        &mut self,
        resource_type: u32,
        resource_index: u32,
        host_path: Option<PathBuf>,
    ) {
        let event = EappAudioEvent {
            frame: self.frame_counter,
            resource_type,
            resource_index,
            host_path: host_path.clone(),
        };
        if let Ok(mut queue) = self.audio_events.lock() {
            // A headless run may never drain the frontend queue. Keep the
            // latest bounded window rather than allowing a long attract-mode
            // run to grow without limit.
            const AUDIO_EVENT_QUEUE_LIMIT: usize = 1024;
            if queue.len() >= AUDIO_EVENT_QUEUE_LIMIT {
                queue.pop_front();
            }
            queue.push_back(event);
        }
        if std::env::var_os("EAPP_AUDIO_EVENT_TRACE").is_some() {
            info!(
                target: "EAPP_AUDIO",
                "AudioEvent frame={} type={} index={} path={}",
                self.frame_counter,
                resource_type,
                resource_index,
                host_path
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<unmapped>".to_string())
            );
        }
    }

    fn note_audio_asset_path(&mut self, host_path: &Path) {
        if self.uses_wav_audio_source_abi() {
            self.note_wav_audio_asset_path(host_path);
            return;
        }
        if self.uses_pacman_wav_audio_source_abi() {
            self.note_pacman_wav_asset_path(host_path);
            return;
        }
        if self.uses_mspacman_wav_audio_source_abi() {
            self.note_mspacman_wav_asset_path(host_path);
            return;
        }
        // Keep the provisional Tetris-only resource catalog honest until the
        // other title families have their own Audio ABI derived.
        if self.metadata.title != "66666" {
            return;
        }
        let is_audio = host_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "wav" | "m4a" | "aac"))
            .unwrap_or(false);
        if !is_audio {
            return;
        }
        let Some(resource) = self.audio_last_registration else {
            return;
        };
        if self
            .audio_resource_paths
            .entry(resource)
            .or_insert_with(|| host_path.to_path_buf())
            .as_path()
            == host_path
            && std::env::var_os("EAPP_AUDIO_EVENT_TRACE").is_some()
        {
            info!(
                target: "EAPP_AUDIO",
                "AudioResourceMap type={} index={} path={}",
                resource.0,
                resource.1,
                host_path.display()
            );
        }
    }

    fn uses_wav_audio_source_abi(&self) -> bool {
        matches!(self.metadata.title.as_str(), "44444" | "55555")
    }

    fn uses_pacman_wav_audio_source_abi(&self) -> bool {
        self.metadata.title == "AAAAA"
    }

    fn uses_mspacman_wav_audio_source_abi(&self) -> bool {
        self.metadata.title == "14004"
    }

    fn note_wav_audio_asset_path(&mut self, host_path: &Path) {
        let is_wav = host_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("wav"))
            .unwrap_or(false);
        if !is_wav {
            return;
        }
        let Some(handle) = self.audio_pending_asset_handle.take() else {
            return;
        };
        let Some(source) = self.audio_handles.get_mut(&handle) else {
            return;
        };
        source.host_path = Some(host_path.to_path_buf());
        if std::env::var_os("EAPP_AUDIO_EVENT_TRACE").is_some() {
            info!(
                target: "EAPP_AUDIO",
                "AudioSourceMap handle={:#010x} type={} slot={} path={}",
                handle,
                source.source_type,
                source.slot_index,
                host_path.display()
            );
        }
    }

    fn note_pacman_wav_asset_path(&mut self, host_path: &Path) {
        let is_wav = host_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("wav"))
            .unwrap_or(false);
        if !is_wav {
            return;
        }
        let Some(handle) = self.audio_pacman_pending_handles.pop_front() else {
            return;
        };
        let Some(source) = self.audio_handles.get_mut(&handle) else {
            return;
        };
        source.host_path = Some(host_path.to_path_buf());
        if std::env::var_os("EAPP_AUDIO_EVENT_TRACE").is_some() {
            info!(
                target: "EAPP_AUDIO",
                "AudioSourceMap handle={:#010x} type={} slot={} path={}",
                handle,
                source.source_type,
                source.slot_index,
                host_path.display()
            );
        }
    }

    fn note_mspacman_wav_asset_path(&mut self, host_path: &Path) {
        let is_wav = host_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("wav"))
            .unwrap_or(false);
        if !is_wav {
            return;
        }
        let Some(handle) = self.audio_pending_asset_handle.take() else {
            return;
        };
        let Some((source_type, slot_index, table_base)) = self
            .audio_handles
            .get(&handle)
            .map(|source| (source.source_type, source.slot_index, source.table_base))
        else {
            return;
        };
        self.audio_mspacman_pending_asset_paths.push_back((
            source_type,
            slot_index,
            table_base,
            host_path.to_path_buf(),
        ));
        if let Some(source) = self.audio_handles.get_mut(&handle) {
            source.host_path = Some(host_path.to_path_buf());
        }
    }

    /// Env-gated Tetris localization/string-table PC trace. Logs only the
    /// first few hits for each PC, enough to identify which suspected routines
    /// run on the default path and what guest pointers/status fields they see.
    fn maybe_trace_string_path(&mut self, pc: u32) {
        if !string_trace_enabled() || !STRING_TRACE_PCS.contains(&pc) {
            return;
        }
        let hit_count = {
            let hit = self.string_trace_hits.entry(pc).or_insert(0);
            *hit = hit.wrapping_add(1);
            *hit
        };
        let limit = std::env::var_os("EAPP_STRING_TRACE_LIMIT")
            .and_then(|v| v.to_string_lossy().parse::<u32>().ok())
            .unwrap_or(24);
        if hit_count > limit {
            return;
        }

        let mode = self.cpu.mode();
        let regs = [
            self.cpu.reg_get(mode, 0),
            self.cpu.reg_get(mode, 1),
            self.cpu.reg_get(mode, 2),
            self.cpu.reg_get(mode, 3),
            self.cpu.reg_get(mode, 4),
            self.cpu.reg_get(mode, 5),
            self.cpu.reg_get(mode, 6),
            self.cpu.reg_get(mode, 7),
        ];
        let lr = self.cpu.reg_get(mode, reg::LR);
        let mut details = Vec::new();
        match pc {
            0x1800_7b0c | 0x1800_7b6c | 0x1800_c7a0 | 0x1800_cb84 | 0x1800_cbf8 => {
                // Generic scene/list constructors. 0x1800c7a0 is the shared
                // initializer that stores the string payload at node+0x10 and
                // assigns the drawable/text object at node+0x14.  At function
                // entry, the extra arguments are still on the caller stack; log
                // the first slots so name-entry construction can be tied back
                // to its caller without hardcoding heap addresses.
                let sp = self.cpu.reg_get(mode, reg::SP);
                details.push(format!(
                    "lr={:#010x} sp={:#010x} ctor_r0={:#010x} ctor_r1={:#010x} ctor_r2={:#010x} ctor_r3={:#010x}",
                    lr, sp, regs[0], regs[1], regs[2], regs[3]
                ));
                for idx in 0..8u32 {
                    let addr = sp.wrapping_add(idx.wrapping_mul(4));
                    details.push(format!(
                        "stk{}={:#010x}",
                        idx,
                        self.read_guest_u32(addr).unwrap_or(0)
                    ));
                }
                let obj = regs[0];
                if pc == 0x1800_c7a0 {
                    details.push(format!(
                        "pre_obj={:#010x} vt={:#010x} old_count={} old[10]={:#010x} old[14]={:#010x} old[18]={:#010x} old[30]={:#010x}",
                        obj,
                        self.read_guest_u32(obj).unwrap_or(0),
                        self.read_guest_u32(obj.wrapping_add(8)).unwrap_or(0),
                        self.read_guest_u32(obj.wrapping_add(0x10)).unwrap_or(0),
                        self.read_guest_u32(obj.wrapping_add(0x14)).unwrap_or(0),
                        self.read_guest_u32(obj.wrapping_add(0x18)).unwrap_or(0),
                        self.read_guest_u32(obj.wrapping_add(0x30)).unwrap_or(0),
                    ));
                }
            }
            0x1800_c938 => {
                let obj = regs[0];
                let child_count = self.read_guest_u32(obj.wrapping_add(8)).unwrap_or(0);
                let child_array = self.read_guest_u32(obj.wrapping_add(0x30)).unwrap_or(0);
                details.push(format!(
                    "lr={:#010x} scene_obj={:#010x} vtable={:#010x} count={} child_array={:#010x} obj[0c]={:#010x} obj[10]={:#010x} obj[14]={:#010x} obj[18]={:#010x} obj[28]={:#010x}",
                    lr,
                    obj,
                    self.read_guest_u32(obj).unwrap_or(0),
                    child_count,
                    child_array,
                    self.read_guest_u32(obj.wrapping_add(0x0c)).unwrap_or(0),
                    self.read_guest_u32(obj.wrapping_add(0x10)).unwrap_or(0),
                    self.read_guest_u32(obj.wrapping_add(0x14)).unwrap_or(0),
                    self.read_guest_u32(obj.wrapping_add(0x18)).unwrap_or(0),
                    self.read_guest_u32(obj.wrapping_add(0x28)).unwrap_or(0),
                ));
                for idx in 0..child_count.min(16) {
                    let child = self
                        .read_guest_u32(child_array.wrapping_add(idx.wrapping_mul(4)))
                        .unwrap_or(0);
                    details.push(format!(
                        "child{}={:#010x}/vt={:#010x}",
                        idx,
                        child,
                        self.read_guest_u32(child).unwrap_or(0)
                    ));
                }
            }
            0x1801_8c18 | 0x1801_8cb8 | 0x1801_c580 | 0x1800_6170 => {
                // The name-entry scene is selected by a small guest-side
                // profile/state query before the generic scene leaves are
                // built.  0x1801c580 loads the profile object from +0xe8 and
                // 0x18006170 returns its +8 field; 0x18018cb8 compares that
                // result with 1.  Keep the probe value-oriented so it remains
                // useful across allocator layouts and does not alter the
                // selector.
                let object = if pc == 0x1801_8cb8 { regs[4] } else { regs[0] };
                let profile = self.read_guest_u32(object.wrapping_add(0xe8)).unwrap_or(0);
                let profile_value = if pc == 0x1800_6170 {
                    self.read_guest_u32(object.wrapping_add(8)).unwrap_or(0)
                } else if profile != 0 {
                    self.read_guest_u32(profile.wrapping_add(8)).unwrap_or(0)
                } else {
                    0
                };
                details.push(format!(
                    "PROFILE_SELECTOR pc={:#010x} object={:#010x} result_r0={:#010x} object+e8={:#010x} profile+8={:#010x} object+34={:#010x} object+c0={:#010x} object+c8={:#010x} object+cc={:#010x}",
                    pc,
                    object,
                    regs[0],
                    profile,
                    profile_value,
                    self.read_guest_u32(object.wrapping_add(0x34)).unwrap_or(0),
                    self.read_guest_u32(object.wrapping_add(0xc0)).unwrap_or(0),
                    self.read_guest_u32(object.wrapping_add(0xc8)).unwrap_or(0),
                    self.read_guest_u32(object.wrapping_add(0xcc)).unwrap_or(0)
                ));
            }
            0x1801_62e4 | 0x1801_6320 => {
                // Generic text-object draw wrapper. It receives the same
                // text/string pair as the concrete UTF-16 helper, then calls
                // vtable[0x38]. Logging this wrapper gives the real caller LR
                // above the helper (0x18009464's LR is always 0x18016324).
                let text_obj = if pc == 0x1801_62e4 { regs[0] } else { regs[4] };
                let str_obj = if pc == 0x1801_62e4 { regs[3] } else { regs[7] };
                let vtable = self.read_guest_u32(text_obj).unwrap_or(0);
                let ip = self.cpu.reg_get(mode, 12);
                details.push(format!(
                    "lr={:#010x} text_obj={:#010x} vtable={:#010x} draw_fn={:#010x} r1={:#010x} r2={:#010x} str_obj={:#010x} str[8]={:#010x} str[c]={:#010x}",
                    lr,
                    text_obj,
                    vtable,
                    if pc == 0x1801_6320 { ip } else { self.read_guest_u32(vtable.wrapping_add(0x38)).unwrap_or(0) },
                    regs[1],
                    regs[2],
                    str_obj,
                    self.read_guest_u32(str_obj.wrapping_add(8)).unwrap_or(0),
                    self.read_guest_u32(str_obj.wrapping_add(0x0c)).unwrap_or(0)
                ));
            }
            0x1800_9464 | 0x1800_9514 => {
                // UTF-16 text draw helper.  Function entry gets
                // r0=text/glyph object and r3=string object; by 0x9514 the
                // string object is in r7 and the text object is in r4.  This
                // ties active Player Name / legal text reads back to the UI
                // text object that selected them.
                let text_obj = if pc == 0x1800_9464 { regs[0] } else { regs[4] };
                let str_obj = if pc == 0x1800_9464 { regs[3] } else { regs[7] };
                details.push(format!(
                    "lr={:#010x} text_obj={:#010x} text[0]={:#010x} text[14]={:#010x} text[24]={:#010x} text[28]={:#010x} text[2c]={:#010x} text[30]={:#010x} str_obj={:#010x} str[0]={:#010x} str[8]={:#010x} str[c]={:#010x}",
                    lr,
                    text_obj,
                    self.read_guest_u32(text_obj).unwrap_or(0),
                    self.read_guest_u32(text_obj.wrapping_add(0x14)).unwrap_or(0),
                    self.read_guest_u32(text_obj.wrapping_add(0x24)).unwrap_or(0),
                    self.read_guest_u32(text_obj.wrapping_add(0x28)).unwrap_or(0),
                    self.read_guest_u32(text_obj.wrapping_add(0x2c)).unwrap_or(0),
                    self.read_guest_u32(text_obj.wrapping_add(0x30)).unwrap_or(0),
                    str_obj,
                    self.read_guest_u32(str_obj).unwrap_or(0),
                    self.read_guest_u32(str_obj.wrapping_add(8)).unwrap_or(0),
                    self.read_guest_u32(str_obj.wrapping_add(0x0c)).unwrap_or(0)
                ));
            }
            0x1801_26d8 | 0x1801_2704 => {
                let obj = regs[0];
                details.push(format!(
                    "lr={:#010x} obj={:#010x} obj[0]={:#010x} obj[8]={:#010x} obj[c]={:#010x}",
                    lr,
                    obj,
                    self.read_guest_u32(obj).unwrap_or(0),
                    self.read_guest_u32(obj.wrapping_add(8)).unwrap_or(0),
                    self.read_guest_u32(obj.wrapping_add(0x0c)).unwrap_or(0)
                ));
            }
            0x1801_270c => {
                let obj = regs[0];
                details.push(format!(
                    "lr={:#010x} SET obj={:#010x} ptr={:#010x} len={} old[8]={:#010x} old[c]={:#010x}",
                    lr,
                    obj,
                    regs[1],
                    regs[2],
                    self.read_guest_u32(obj.wrapping_add(8)).unwrap_or(0),
                    self.read_guest_u32(obj.wrapping_add(0x0c)).unwrap_or(0)
                ));
            }
            0x1800_3bd0 | 0x1800_3c08 | 0x1800_3c68 | 0x1800_3c74 | 0x1800_3d40 | 0x1800_3d60
            | 0x1800_3da8 | 0x1800_4fac | 0x1800_5400 | 0x1800_5468 | 0x1800_5480 => {
                // Boot/resource-progress state struct. 0x18003bd0 dispatches
                // on [base+4]; 0x18004fac is the Strings.dta 2nd-stage cb
                // that writes selected byte-counts into this struct and bumps
                // the progress state.
                let base = 0x1802_5674;
                for off in [0x00, 0x04, 0x08, 0x0c, 0x10, 0x14, 0x20, 0x24, 0x28, 0x2c] {
                    details.push(format!(
                        "boot+{:#04x}={:#010x}",
                        off,
                        self.read_guest_u32(base + off).unwrap_or(0)
                    ));
                }
                let desc = 0x1802_995c;
                for off in [0x11c, 0x120, 0x124, 0x128, 0x12c] {
                    details.push(format!(
                        "strdesc+{:#04x}={:#010x}",
                        off,
                        self.read_guest_u32(desc + off).unwrap_or(0)
                    ));
                }
                details.push(format!("lr={:#010x}", lr));
            }
            0x1800_5498 => {
                // Audio resource registration validates (type, index), then
                // calls Audio:0 and stores its return in the corresponding
                // 0x18025ec8 table row. Capture the exact slot before the
                // import so the ABI trace can be reconciled with bus writes
                // and later Audio:13/14/15/2 consumers.
                let kind = regs[0];
                let index = regs[1];
                let status_addr = 0x1802_400c_u32.wrapping_add(kind);
                let table_row = 0x1802_5ec8u32.wrapping_add(kind.wrapping_mul(0x100));
                let slot = table_row.wrapping_add(index.wrapping_mul(4));
                details.push(format!(
                    "AUDIO_RESOURCE_REGISTER type={} index={} r2={:#010x} r3={:#010x} status_addr={:#010x} status={:#04x} table_row={:#010x} slot={:#010x} pre_handle={:#010x} lr={:#010x}",
                    kind,
                    index,
                    regs[2],
                    regs[3],
                    status_addr,
                    self.read_guest_u8(status_addr).unwrap_or(0),
                    table_row,
                    slot,
                    self.read_guest_u32(slot).unwrap_or(0),
                    lr
                ));
            }
            0x1801_c940 => {
                let obj = regs[0];
                details.push(format!("lr={:#010x} parent={:#010x}", lr, obj));
                for off in [
                    0x00, 0x04, 0x08, 0x0c, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30,
                    0x38, 0x40, 0x44, 0x48, 0x4c, 0xbc, 0xc4, 0xd4, 0xd8, 0xe4,
                ] {
                    details.push(format!(
                        "parent+{:#04x}={:#010x}",
                        off,
                        self.read_guest_u32(obj.wrapping_add(off)).unwrap_or(0)
                    ));
                }
            }
            0x1801_fc68 => {
                let req = regs[0];
                for off in [0x08, 0x14, 0x18, 0x20, 0x24, 0x34, 0x38] {
                    details.push(format!(
                        "req+{:#04x}={:#010x}",
                        off,
                        self.read_guest_u32(req.wrapping_add(off)).unwrap_or(0)
                    ));
                }
            }
            0x1801_fc94 | 0x1801_e0fc | 0x1801_e45c | 0x1801_e484 | 0x1801_e708 => {
                let obj = regs[0];
                for off in [
                    0x00, 0x04, 0x08, 0x0c, 0x10, 0x14, 0x18, 0x20, 0x24, 0x2c, 0x124, 0x128,
                    0x12c, 0x130,
                ] {
                    details.push(format!(
                        "obj+{:#04x}={:#010x}",
                        off,
                        self.read_guest_u32(obj.wrapping_add(off)).unwrap_or(0)
                    ));
                }
            }
            0x1801_eed8 | 0x1801_ef1c | 0x1801_f000 | 0x1801_f068 | 0x1801_f1b4 | 0x1801_f250
            | 0x1801_f394 | 0x1801_f474 | 0x1801_f4a8 | 0x1801_f558 | 0x1801_f5a8 | 0x1801_f69c
            | 0x1801_f6ec | 0x1801_f72c | 0x1801_f794 | 0x1801_f900 | 0x1801_fa90 | 0x1801_faa8
            | 0x1801_fb3c => {
                let global = 0x1802_5668;
                for off in [0x00, 0x04, 0x08] {
                    details.push(format!(
                        "glob+{:#04x}={:#010x}",
                        off,
                        self.read_guest_u32(global + off).unwrap_or(0)
                    ));
                }
                let r4 = self.cpu.reg_get(mode, 4);
                let r5 = self.cpu.reg_get(mode, 5);
                let r6 = self.cpu.reg_get(mode, 6);
                details.push(format!(
                    "lr={:#010x} r4={:#010x} r5={:#010x} r6={:#010x}",
                    lr, r4, r5, r6
                ));

                // Most `0x1801fxxx` routines take the menu/resource object in
                // r0 at function entry and keep it in r4 internally. At inner
                // PCs (e.g. 0x1f558) r0 is often a scalar field, so also dump
                // r4 when it looks like a work-RAM object.
                for (label, obj) in [("r0", regs[0]), ("r4", r4)] {
                    if !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&obj) {
                        continue;
                    }
                    for off in [
                        0x00, 0x08, 0x0c, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30,
                        0x38, 0x3c, 0x40, 0x44, 0x48, 0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60, 0x64,
                        0x68, 0x6c, 0x70, 0x74, 0x9c, 0xa0, 0xa4,
                    ] {
                        details.push(format!(
                            "{}+{:#04x}={:#010x}",
                            label,
                            off,
                            self.read_guest_u32(obj.wrapping_add(off)).unwrap_or(0)
                        ));
                    }
                    let str_obj = self.read_guest_u32(obj.wrapping_add(0x50)).unwrap_or(0);
                    if (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&str_obj) {
                        details.push(format!(
                            "{}+50.str obj={:#010x} ptr={:#010x} len={:#010x}",
                            label,
                            str_obj,
                            self.read_guest_u32(str_obj.wrapping_add(8)).unwrap_or(0),
                            self.read_guest_u32(str_obj.wrapping_add(0x0c)).unwrap_or(0)
                        ));
                    }
                }
            }
            0x1801_d644 => {
                // entry: r0=mgr, r1=index. Compute the entry pointer the
                // same way the guest does: mgr + index*388, then read
                // entry[7] (the spin guard) and entry[0x180] (linked-next).
                let mgr = regs[0];
                let idx = regs[1];
                let entry = mgr.wrapping_add(idx.wrapping_mul(388));
                details.push(format!(
                    "mgr={:#010x} idx={} entry={:#010x} e[7]={:#04x} e[180]={:#010x} e[124]={:#04x} e[120]={:#04x}",
                    mgr,
                    idx,
                    entry,
                    self.read_guest_u8(entry.wrapping_add(7)).unwrap_or(0),
                    self.read_guest_u32(entry.wrapping_add(0x180)).unwrap_or(0),
                    self.read_guest_u8(entry.wrapping_add(0x124)).unwrap_or(0),
                    self.read_guest_u8(entry.wrapping_add(0x120)).unwrap_or(0)
                ));
            }
            0x1801_d664 => {
                // dead-spin: logged only if entry[7]!=0 on registration.
                // If this PC fires, the guest loops here forever.
                details.push(format!("SPIN r0={:#010x} r1={:#010x}", regs[0], regs[1]));
            }
            0x1801_d8d0 => {
                // manager init: returns a freshly-allocated 10-slot array.
                // Fires once at startup. Log nothing extra; outer info! has it.
            }
            0x1801_d76c => {
                // dispatcher pops head of pending list = [mgr+0xf28]; the
                // popped entry is at [mgr+0xf28]. If [mgr+0xf28]==0 the
                // list is empty (returns -1, no work).  Log head entry, its
                // [entry+0x170] in-flight flag, and [entry+0x124] (done).
                let mgr = regs[0];
                let head = self.read_guest_u32(mgr.wrapping_add(0xf28)).unwrap_or(0);
                let (inf, e124, e128) = if head != 0 {
                    (
                        self.read_guest_u8(head.wrapping_add(0x170)).unwrap_or(0),
                        self.read_guest_u8(head.wrapping_add(0x124)).unwrap_or(0),
                        self.read_guest_u32(head.wrapping_add(0x128)).unwrap_or(0),
                    )
                } else {
                    (0, 0, 0)
                };
                details.push(format!(
                    "mgr={:#010x} head={:#010x} e[170]={:#04x} e[124]={:#04x} e[128]={:#010x}",
                    mgr, head, inf, e124, e128
                ));
            }
            0x1801_d770 | 0x1801_d800 | 0x1801_d824 | 0x1801_d840 | 0x1801_d858 => {
                // Audio-stream manager. d770 takes the pending-list head,
                // initializes that entry, and starts either the shared-owner
                // read (d800) or the secondary-owner read (d824). At d840,
                // r0 is the initiator result; d858 returns r6, the entry's
                // selected slot handle (or -1 when no pending entry exists).
                // Keeping these probes at the guest PCs makes the release
                // seen in the resource table trace attributable to the actual
                // handoff rather than to an assumed HLE failure.
                let manager = if pc == 0x1801_d770 { regs[0] } else { regs[5] };
                let entry = if pc == 0x1801_d770 {
                    self.read_guest_u32(manager.wrapping_add(0xf28))
                        .unwrap_or(0)
                } else {
                    regs[4]
                };
                let mut state = if entry != 0 {
                    format!(" e[0]={:#010x}", self.read_guest_u32(entry).unwrap_or(0))
                } else {
                    String::new()
                };
                if entry != 0 {
                    for (label, off) in [
                        ("4", 0x04),
                        ("5", 0x05),
                        ("6", 0x06),
                        ("7", 0x07),
                        ("8", 0x08),
                        ("9", 0x09),
                        ("10c", 0x10c),
                        ("110", 0x110),
                        ("114", 0x114),
                        ("118", 0x118),
                        ("120", 0x120),
                        ("124", 0x124),
                        ("128", 0x128),
                        ("164", 0x164),
                        ("168", 0x168),
                        ("16c", 0x16c),
                        ("170", 0x170),
                        ("180", 0x180),
                    ] {
                        let value = if off < 0x10 {
                            self.read_guest_u8(entry.wrapping_add(off)).unwrap_or(0) as u32
                        } else {
                            self.read_guest_u32(entry.wrapping_add(off)).unwrap_or(0)
                        };
                        state.push_str(&format!(" e[{}]={:#010x}", label, value));
                    }
                }
                details.push(format!(
                    "AUDIO_MANAGER pc={:#010x} mgr={:#010x} entry={:#010x} r0={:#010x} r4={:#010x} r5={:#010x} r6={:#010x}{}",
                    pc, manager, entry, regs[0], regs[4], regs[5], regs[6], state
                ));
            }
            0x1800_3eb8 => {
                let kind = regs[0];
                let index = regs[1];
                let table_row = 0x1802_5ec8u32.wrapping_add(kind.wrapping_mul(0x100));
                let handle = self
                    .read_guest_u32(table_row.wrapping_add(index.wrapping_mul(4)))
                    .unwrap_or(0);
                let status = self
                    .read_guest_u8(0x1802_400c_u32.wrapping_add(kind))
                    .unwrap_or(0);
                details.push(format!(
                    "AUDIO_TABLE_CONSUMER type={} index={} status={:#04x} handle={:#010x} r2={:#010x} r3={:#010x} lr={:#010x}",
                    kind, index, status, handle, regs[2], regs[3], lr
                ));
            }
            0x1801_d1b4 => {
                // Audio-stream owner callback invoked by 0x1801fc30 after
                // AsyncFileIO:0 owner completion. Args are
                // (request, status, byte_count, entry/ctx). It forwards those
                // into a second manager via 0x1801d500 with ctx passed on stack.
                let req = regs[0];
                let ctx = regs[3];
                details.push(format!(
                    "AUDIO_CB req={:#010x} status={:#x} bc={} ctx={:#010x} req[4]={:#04x} req[c]={:#010x} req[10]={:#010x} req[170?]={:#04x}",
                    req,
                    regs[1],
                    regs[2],
                    ctx,
                    self.read_guest_u8(req.wrapping_add(4)).unwrap_or(0),
                    self.read_guest_u32(req.wrapping_add(0x0c)).unwrap_or(0),
                    self.read_guest_u32(req.wrapping_add(0x10)).unwrap_or(0),
                    self.read_guest_u8(req.wrapping_add(4)).unwrap_or(0)
                ));
            }
            0x1801_d500 | 0x1801_d548 | 0x1801_d5bc | 0x1801_d5cc => {
                // d500: r0=manager. The actual entry is stack arg 0 from the
                // caller (d1b4 passes request[0x10]); on function entry it is
                // not in a register, but at d548/d5bc the entry is in r4.
                let e = if pc == 0x1801_d500 { regs[3] } else { regs[4] };
                details.push(format!(
                    "D500 pc={:#010x} mgr/r0={:#010x} entry_guess={:#010x} e[4]={:#04x} e[6]={:#04x} e[7]={:#04x} e[110]={:#04x} e[114]={:#010x} e[118]={:#010x} e[11c]={:#010x} e[120]={:#010x} e[124]={:#04x} e[164]={:#010x} e[168]={:#010x} e[16c+4]={:#04x} e[174]={:#010x} e[180]={:#010x}",
                    pc,
                    regs[0],
                    e,
                    self.read_guest_u8(e.wrapping_add(4)).unwrap_or(0),
                    self.read_guest_u8(e.wrapping_add(6)).unwrap_or(0),
                    self.read_guest_u8(e.wrapping_add(7)).unwrap_or(0),
                    self.read_guest_u8(e.wrapping_add(0x110)).unwrap_or(0),
                    self.read_guest_u32(e.wrapping_add(0x114)).unwrap_or(0),
                    self.read_guest_u32(e.wrapping_add(0x118)).unwrap_or(0),
                    self.read_guest_u32(e.wrapping_add(0x11c)).unwrap_or(0),
                    self.read_guest_u32(e.wrapping_add(0x120)).unwrap_or(0),
                    self.read_guest_u8(e.wrapping_add(0x124)).unwrap_or(0),
                    self.read_guest_u32(e.wrapping_add(0x164)).unwrap_or(0),
                    self.read_guest_u32(e.wrapping_add(0x168)).unwrap_or(0),
                    self.read_guest_u8(e.wrapping_add(0x170)).unwrap_or(0),
                    self.read_guest_u32(e.wrapping_add(0x174)).unwrap_or(0),
                    self.read_guest_u32(e.wrapping_add(0x180)).unwrap_or(0)
                ));
            }
            0x1801_d258 | 0x1801_d68c | 0x1801_d424 => {
                // AsyncFileIO:2 completion callbacks. r0 is owner at
                // entry+0x128 and r1 must be the parent entry/context; these
                // callbacks spin forever if that relation is wrong.
                let owner = regs[0];
                let entry = regs[1];
                details.push(format!(
                    "ASYNC2_CB pc={:#010x} owner={:#010x} entry={:#010x} entry+128={:#010x} e[4]={:#04x} e[6]={:#04x} e[110]={:#04x} e[114]={:#010x} e[120]={:#010x} e[124]={:#04x} e[14c]={:#010x} e[164]={:#010x} e[168]={:#010x} owner[1c]={:#04x} owner[34]={:#010x} owner[38]={:#010x}",
                    pc,
                    owner,
                    entry,
                    entry.wrapping_add(0x128),
                    self.read_guest_u8(entry.wrapping_add(4)).unwrap_or(0),
                    self.read_guest_u8(entry.wrapping_add(6)).unwrap_or(0),
                    self.read_guest_u8(entry.wrapping_add(0x110)).unwrap_or(0),
                    self.read_guest_u32(entry.wrapping_add(0x114)).unwrap_or(0),
                    self.read_guest_u32(entry.wrapping_add(0x120)).unwrap_or(0),
                    self.read_guest_u8(entry.wrapping_add(0x124)).unwrap_or(0),
                    self.read_guest_u32(entry.wrapping_add(0x14c)).unwrap_or(0),
                    self.read_guest_u32(entry.wrapping_add(0x164)).unwrap_or(0),
                    self.read_guest_u32(entry.wrapping_add(0x168)).unwrap_or(0),
                    self.read_guest_u8(owner.wrapping_add(0x1c)).unwrap_or(0),
                    self.read_guest_u32(owner.wrapping_add(0x34)).unwrap_or(0),
                    self.read_guest_u32(owner.wrapping_add(0x38)).unwrap_or(0)
                ));
            }
            0x1801_fe28 | 0x1801_fcc8 | 0x1801_fd74 => {
                // I/O initiator: r0 = entry+0x16c (the request struct).
                // Reads [entry+0x170] (==[r0+4]) as the in-flight byte;
                // nonzero -> immediately returns 0 (busy-wait re-link).
                let r = regs[0];
                let entry = r.wrapping_sub(0x16c);
                details.push(format!(
                    "req={:#010x} entry={:#010x} [e+170]={:#04x} [r+4]={:#04x} [r+0xc]={:#010x} [r+0x10]={:#010x}",
                    r,
                    entry,
                    self.read_guest_u8(entry.wrapping_add(0x170)).unwrap_or(0),
                    self.read_guest_u8(r.wrapping_add(4)).unwrap_or(0),
                    self.read_guest_u32(r.wrapping_add(0x0c)).unwrap_or(0),
                    self.read_guest_u32(r.wrapping_add(0x10)).unwrap_or(0)
                ));
            }
            0x1801_fec8 => {
                // Right after `strbne r0, [r4, #4]` (set [entry+0x170]=4 on
                // success). r4 should be entry+0x16c; r6 holds our AsyncFileIO:3
                // return value. If [e+170] != 4 here, our return is being
                // treated as 0 / the strb doesn't fire.
                let r = regs[0];
                let entry = r.wrapping_sub(0x16c);
                details.push(format!(
                    "POST-3 r6={:#010x} [e+170]={:#04x} [r+4]={:#04x}",
                    self.read_guest_u8(r.wrapping_add(4)).unwrap_or(0),
                    self.read_guest_u8(entry.wrapping_add(0x170)).unwrap_or(0),
                    self.read_guest_u8(r.wrapping_add(4)).unwrap_or(0)
                ));
            }
            0x1801_fed8 | 0x1801_fddc => {
                // Success-branch return for initiator A/C. For 0x1fd74,
                // nonzero return sets [request+4]=3 before reaching fddc.
                let req = if pc == 0x1801_fddc { regs[4] } else { regs[0] };
                details.push(format!(
                    "RET pc={:#010x} r0={:#010x} req_guess={:#010x} req[4]={:#04x} req[c]={:#010x} req[10]={:#010x}",
                    pc,
                    regs[0],
                    req,
                    self.read_guest_u8(req.wrapping_add(4)).unwrap_or(0),
                    self.read_guest_u32(req.wrapping_add(0x0c)).unwrap_or(0),
                    self.read_guest_u32(req.wrapping_add(0x10)).unwrap_or(0)
                ));
            }
            0x1801_d370 => {
                // Shared owner cb: (owner, status, byte_count, ctx). ctx=r3.
                // Writes ctx+0x11c=[owner+8]=-1, ctx+0x120=byte_count,
                // ctx+0x124=1; tail-calls [ctx+0x164] (processor) with
                // (r0=[ctx], r1=byte_count, r2=ctx+8, r3=[ctx+0x168]=desc).
                let owner = regs[0];
                let status = regs[1];
                let byte_count = regs[2];
                let ctx = regs[3];
                let processor = self.read_guest_u32(ctx.wrapping_add(0x164)).unwrap_or(0);
                let desc = self.read_guest_u32(ctx.wrapping_add(0x168)).unwrap_or(0);
                let idx_word = self.read_guest_u32(desc.wrapping_add(0x11c)).unwrap_or(0);
                let idx_word_tex = self.read_guest_u32(desc.wrapping_add(0x128)).unwrap_or(0);
                details.push(format!(
                    "owner={:#010x} status={:#x} bc={} ctx={:#010x} proc={:#010x} desc={:#010x} d[11c]={:#x} d[128]={:#x}",
                    owner, status, byte_count, ctx, processor, desc, idx_word, idx_word_tex
                ));
            }
            0x1801_5c30 | 0x1801_5c74 => {
                // Generic descriptor async registrar. 0x18003d54 calls this
                // once for the wav-descriptor list; 0x18005468 re-enters it
                // for the next descriptor until 0x18005480 advances state 4→5.
                let desc = regs[0];
                details.push(format!("lr={:#010x} desc={:#010x}", lr, desc));
                for off in [
                    0x00, 0x04, 0x08, 0x0c, 0x100, 0x107, 0x108, 0x109, 0x10c, 0x110,
                ] {
                    details.push(format!(
                        "desc+{:#04x}={:#010x}",
                        off,
                        self.read_guest_u32(desc.wrapping_add(off)).unwrap_or(0)
                    ));
                }
                details.push(format!("r0ret/pre={:#010x}", regs[0]));
            }
            0x1801_5308 => {
                // Strings.dta processor: (r0=res_obj, r1=byte_count, r2=ctx+8,
                // r3=desc=0x1802995c). Reads r3+0x11c=index, calls 0x1d644,
                // writes desc+0x120=1 (done), desc+0x124=byte_count, then
                // tail-calls desc+0x128 (2nd-stage cb) if set.
                let desc = regs[3];
                let index = self.read_guest_u32(desc.wrapping_add(0x11c)).unwrap_or(0);
                let done = self.read_guest_u8(desc.wrapping_add(0x120)).unwrap_or(0);
                let stored_bc = self.read_guest_u32(desc.wrapping_add(0x124)).unwrap_or(0);
                let next_cb = self.read_guest_u32(desc.wrapping_add(0x128)).unwrap_or(0);
                details.push(format!(
                    "STRINGSPROC desc={:#010x} byte_count_in={} d[11c]=idx={} d[120]=done={} d[124]=bc={} d[128]=next_cb={:#010x}",
                    desc, regs[1], index, done, stored_bc, next_cb
                ));
            }
            0x1801_9770 => {
                // Texture processor: mirror image with +0xc offsets.
                // d[128]=index, d[12c]=done, d[130]=bc, d[134]=next_cb.
                let desc = regs[3];
                let index = self.read_guest_u32(desc.wrapping_add(0x128)).unwrap_or(0);
                let done = self.read_guest_u8(desc.wrapping_add(0x12c)).unwrap_or(0);
                let stored_bc = self.read_guest_u32(desc.wrapping_add(0x130)).unwrap_or(0);
                let next_cb = self.read_guest_u32(desc.wrapping_add(0x134)).unwrap_or(0);
                details.push(format!(
                    "TEXPROC desc={:#010x} byte_count_in={} d[128]=idx={} d[12c]=done={} d[130]=bc={} d[134]=next_cb={:#010x}",
                    desc, regs[1], index, done, stored_bc, next_cb
                ));
            }
            // Scene root and constructor selection trace
            0x1801_c014 => {
                // Scene root installer: reads [clock_obj+0x2c], calls vtable[0x44]
                // This is where the active scene graph is selected for drawing
                let clock_obj = regs[0];
                let root_ptr = self
                    .read_guest_u32(clock_obj.wrapping_add(0x2c))
                    .unwrap_or(0);
                // The root object has a vtable at offset 0 and various children
                let vtable = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr).unwrap_or(0)
                } else {
                    0
                };
                // Read more fields to understand the object layout
                let obj_04 = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr.wrapping_add(0x04))
                        .unwrap_or(0)
                } else {
                    0
                };
                let obj_08 = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr.wrapping_add(0x08))
                        .unwrap_or(0)
                } else {
                    0
                };
                let obj_0c = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr.wrapping_add(0x0c))
                        .unwrap_or(0)
                } else {
                    0
                };
                let obj_10 = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr.wrapping_add(0x10))
                        .unwrap_or(0)
                } else {
                    0
                };
                let obj_14 = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr.wrapping_add(0x14))
                        .unwrap_or(0)
                } else {
                    0
                };
                let obj_18 = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr.wrapping_add(0x18))
                        .unwrap_or(0)
                } else {
                    0
                };
                let obj_1c = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr.wrapping_add(0x1c))
                        .unwrap_or(0)
                } else {
                    0
                };
                let obj_20 = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr.wrapping_add(0x20))
                        .unwrap_or(0)
                } else {
                    0
                };
                let obj_24 = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr.wrapping_add(0x24))
                        .unwrap_or(0)
                } else {
                    0
                };
                let obj_28 = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr.wrapping_add(0x28))
                        .unwrap_or(0)
                } else {
                    0
                };
                let obj_2c = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr.wrapping_add(0x2c))
                        .unwrap_or(0)
                } else {
                    0
                };
                details.push(format!(
                    "ROOT clock_obj={:#010x} root={:#010x} vt={:#010x} [04]={:#010x} [08]={:#010x} [0c]={:#010x} [10]={:#010x} [14]={:#010x} [18]={:#010x} [1c]={:#010x} [20]={:#010x} [24]={:#010x} [28]={:#010x} [2c]={:#010x}",
                    clock_obj, root_ptr, vtable, obj_04, obj_08, obj_0c, obj_10, obj_14, obj_18, obj_1c, obj_20, obj_24, obj_28, obj_2c
                ));
            }
            0x1801_8f40 => {
                // Name-entry screen constructor entry
                details.push(format!(
                    "NAMEENTRY_CTOR r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x}",
                    regs[0], regs[1], regs[2], regs[3]
                ));
            }
            0x1801_95a8 | 0x1801_c940 => {
                // Options/settings or vtable constructor
                details.push(format!(
                    "MENU_CTOR pc={:#010x} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x}",
                    pc, regs[0], regs[1], regs[2], regs[3]
                ));
            }
            0x1801_c95c | 0x1801_c008 => {
                // Post-save construction / UI refresh
                details.push(format!(
                    "UI_REFRESH pc={:#010x} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x}",
                    pc, regs[0], regs[1], regs[2], regs[3]
                ));
            }
            _ => {}
        }
        info!(
            target: "EAPP_STRING_TRACE",
            "pc={:#010x} hit={} frame={} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} {}",
            pc,
            hit_count,
            self.frame_counter,
            regs[0],
            regs[1],
            regs[2],
            regs[3],
            details.join(" ")
        );
    }

    /// Bounded guest-PC trace for runtime reverse engineering. The trace is
    /// deliberately generic at the CPU level but includes the Royal
    /// Solitaire manager state block when that title is running. It is useful
    /// for distinguishing a missing host callback from a guest-side object
    /// contract without changing the default execution path.
    fn maybe_trace_guest_pc(&mut self, pc: u32) {
        if !self.guest_pc_trace_pcs.contains(&pc) {
            return;
        }
        let hit = {
            let count = self.guest_pc_trace_hits.entry(pc).or_insert(0);
            *count = count.wrapping_add(1);
            *count
        };
        if hit > self.guest_pc_trace_limit {
            return;
        }

        let mode = self.cpu.mode();
        let regs = [
            self.cpu.reg_get(mode, 0),
            self.cpu.reg_get(mode, 1),
            self.cpu.reg_get(mode, 2),
            self.cpu.reg_get(mode, 3),
            self.cpu.reg_get(mode, 4),
            self.cpu.reg_get(mode, 5),
            self.cpu.reg_get(mode, 6),
            self.cpu.reg_get(mode, 7),
        ];
        let lr = self.cpu.reg_get(mode, reg::LR);
        let sp = self.cpu.reg_get(mode, reg::SP);
        let mut details = format!(
            "r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} r4={:#010x} r5={:#010x} r6={:#010x} r7={:#010x} lr={:#010x} sp={:#010x}",
            regs[0], regs[1], regs[2], regs[3], regs[4], regs[5], regs[6], regs[7], lr, sp
        );

        if fliwheel_var_os("EAPP_PC_TRACE_DETAIL").is_some() {
            let extra_regs = [
                self.cpu.reg_get(mode, 8),
                self.cpu.reg_get(mode, 9),
                self.cpu.reg_get(mode, 10),
                self.cpu.reg_get(mode, 11),
                self.cpu.reg_get(mode, 12),
            ];
            details.push_str(&format!(
                " r8={:#010x} r9={:#010x} r10={:#010x} r11={:#010x} r12={:#010x}",
                extra_regs[0], extra_regs[1], extra_regs[2], extra_regs[3], extra_regs[4]
            ));
        }

        if self.metadata.title == "55555" && fliwheel_var_os("EAPP_PC_TRACE_DETAIL").is_some() {
            let global = 0x180c_bd80;
            let nav = 0x180c_954c;
            let cursor = 0x180c_0674;
            let nav_state_4 = self.read_guest_u32(nav + 4).unwrap_or(0) & 0xffff;
            let input = if (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&regs[4])
            {
                regs[4]
            } else {
                0
            };
            details.push_str(&format!(
                " bejeweled_global=[{:#04x},{:#04x}] nav_4={:#06x} nav_708_70b=[{:#04x},{:#04x},{:#04x},{:#04x}] nav_724_726=[{:#04x},{:#04x},{:#04x}] cursor_674=[{:#010x},{:#010x},{:#010x}] input_obj={:#010x} input_fields=[{:#010x},{:#010x},{:#010x},{:#010x}]",
                self.read_guest_u8(global + 0x161).unwrap_or(0xff),
                self.read_guest_u8(global + 0x162).unwrap_or(0xff),
                nav_state_4,
                self.read_guest_u8(nav + 0x708).unwrap_or(0xff),
                self.read_guest_u8(nav + 0x709).unwrap_or(0xff),
                self.read_guest_u8(nav + 0x70a).unwrap_or(0xff),
                self.read_guest_u8(nav + 0x70b).unwrap_or(0xff),
                self.read_guest_u8(nav + 0x724).unwrap_or(0xff),
                self.read_guest_u8(nav + 0x725).unwrap_or(0xff),
                self.read_guest_u8(nav + 0x726).unwrap_or(0xff),
                self.read_guest_u32(cursor).unwrap_or(0),
                self.read_guest_u32(cursor + 4).unwrap_or(0),
                self.read_guest_u32(cursor + 8).unwrap_or(0),
                input,
                self.read_guest_u32(input + 4).unwrap_or(0) & 0xffff,
                self.read_guest_u32(input + 0x18).unwrap_or(0),
                self.read_guest_u32(input + 0x1c).unwrap_or(0),
                self.read_guest_u32(input + 0x30).unwrap_or(0),
            ));
        }

        if self.metadata.title == "50514" {
            let global = 0x180c_fa5c;
            let global_bytes = (0..4u32)
                .map(|offset| self.read_guest_u8(global + offset).unwrap_or(0xff))
                .map(|value| format!("{:#04x}", value))
                .collect::<Vec<_>>()
                .join(",");
            let global_words = (0..8u32)
                .map(|index| {
                    self.read_guest_u32(global + index * 4)
                        .unwrap_or(0xdead_beef)
                })
                .map(|value| format!("{:#010x}", value))
                .collect::<Vec<_>>()
                .join(",");
            let object = self.read_guest_u32(global + 8).unwrap_or(0);
            let object_vt = self.read_guest_u32(object).unwrap_or(0);
            let app = 0x1050_0b00;
            let frame = 0x1050_2b00;
            details.push_str(&format!(
                " global_bytes=[{}] global_words=[{}] object={:#010x} object_vt={:#010x} app_state={:#04x} app_event_head={:#010x} frame_flags=[{:#04x},{:#04x},{:#04x},{:#04x}]",
                global_bytes,
                global_words,
                object,
                object_vt,
                self.read_guest_u8(app).unwrap_or(0xff),
                self.read_guest_u32(app + 0x20).unwrap_or(0),
                self.read_guest_u8(frame + 5).unwrap_or(0xff),
                self.read_guest_u8(frame + 6).unwrap_or(0xff),
                self.read_guest_u8(frame + 7).unwrap_or(0xff),
                self.read_guest_u8(frame + 8).unwrap_or(0xff),
            ));
        }

        if (self.metadata.title == "1500C" || self.metadata.title == "1500E")
            && fliwheel_var_os("EAPP_PC_TRACE_DETAIL").is_some()
        {
            let stream_pc = matches!(
                pc,
                0x1800_94c4
                    | 0x1800_95a4
                    | 0x1800_95d8
                    | 0x1800_961c
                    | 0x1800_9850
                    | 0x1800_98e4
                    | 0x1803_bd44
                    | 0x1803_bd80
            );
            let resource_pc = matches!(
                pc,
                0x1802_6394
                    | 0x1802_64a0
                    | 0x1802_64fc
                    | 0x1802_6610
                    | 0x1802_6680
                    | 0x1802_66f8
                    | 0x1802_67c0
            );
            if stream_pc {
                let stream = regs[0];
                details.push_str(&format!(
                    " sims_stream={:#010x} stream_words=[{:#010x},{:#010x},{:#010x},{:#010x},{:#010x},{:#010x},{:#010x},{:#010x},{:#010x},{:#010x}] stream_bytes=[{:#04x},{:#04x},{:#04x}]",
                    stream,
                    self.read_guest_u32(stream + 0x0c).unwrap_or(0),
                    self.read_guest_u32(stream + 0x10).unwrap_or(0),
                    self.read_guest_u32(stream + 0x18).unwrap_or(0),
                    self.read_guest_u32(stream + 0x1c).unwrap_or(0),
                    self.read_guest_u32(stream + 0x20).unwrap_or(0),
                    self.read_guest_u32(stream + 0x24).unwrap_or(0),
                    self.read_guest_u32(stream + 0x28).unwrap_or(0),
                    self.read_guest_u32(stream + 0x30).unwrap_or(0),
                    self.read_guest_u32(stream + 0x38).unwrap_or(0),
                    self.read_guest_u32(stream + 0x3c).unwrap_or(0),
                    self.read_guest_u8(stream + 0x2c).unwrap_or(0xff),
                    self.read_guest_u8(stream + 0x2e).unwrap_or(0xff),
                    self.read_guest_u8(stream + 0x34).unwrap_or(0xff),
                ));
            }
            if resource_pc {
                let resource = regs[0];
                details.push_str(&format!(
                    " sims_resource={:#010x} resource_handle={:#010x} resource_state={:#04x} resource_flags=[{:#04x},{:#04x}] resource_words=[{:#010x},{:#010x},{:#010x}]",
                    resource,
                    self.read_guest_u32(resource).unwrap_or(0),
                    self.read_guest_u8(resource + 0x108).unwrap_or(0xff),
                    self.read_guest_u8(resource + 0x104).unwrap_or(0xff),
                    self.read_guest_u8(resource + 0x105).unwrap_or(0xff),
                    self.read_guest_u32(resource + 0x10c).unwrap_or(0),
                    self.read_guest_u32(resource + 0x110).unwrap_or(0),
                    self.read_guest_u32(resource + 0x114).unwrap_or(0),
                ));
            }
        }

        if self.metadata.title == "66666" && fliwheel_var_os("EAPP_PC_TRACE_DETAIL").is_some()
        {
            // Tetris' movement helpers pass either the active piece object or
            // the piece manager in r0. Keep this dump diagnostic-only: the
            // object layout is still being derived from the decrypted guest,
            // and reading the candidate fields must never affect execution.
            let object = regs[0];
            let object_words = (0..16u32)
                .map(|index| {
                    self.read_guest_u32(object.wrapping_add(index * 4))
                        .unwrap_or(0xdead_beef)
                })
                .map(|value| format!("{:#010x}", value))
                .collect::<Vec<_>>()
                .join(",");
            let board = self.read_guest_u32(object.wrapping_add(0x1c)).unwrap_or(0);
            let board_array = self.read_guest_u32(board.wrapping_add(0x10)).unwrap_or(0);
            let board_words = if board != 0 {
                (0..12u32)
                    .map(|index| {
                        self.read_guest_u32(board.wrapping_add(index * 4))
                            .unwrap_or(0xdead_beef)
                    })
                    .map(|value| format!("{:#010x}", value))
                    .collect::<Vec<_>>()
                    .join(",")
            } else {
                String::new()
            };
            let board_array_words = if board_array != 0 {
                (0..40u32)
                    .map(|index| {
                        self.read_guest_u32(board_array.wrapping_add(index * 4))
                            .unwrap_or(0xdead_beef)
                    })
                    .map(|value| format!("{:#010x}", value))
                    .collect::<Vec<_>>()
                    .join(",")
            } else {
                String::new()
            };
            let board_row_masks = if board_array != 0 {
                (0..40u32)
                    .map(|index| {
                        let row = self
                            .read_guest_u32(board_array.wrapping_add(index * 4))
                            .unwrap_or(0);
                        self.read_guest_u32(row.wrapping_add(8)).unwrap_or(0)
                    })
                    .map(|value| format!("{:#06x}", value))
                    .collect::<Vec<_>>()
                    .join(",")
            } else {
                String::new()
            };
            details.push_str(&format!(
                " tetris_object={:#010x} object_words=[{}] tetris_board={:#010x} board_array={:#010x} board_words=[{}] board_array_words=[{}] board_row_masks=[{}]",
                object,
                object_words,
                board,
                board_array,
                board_words,
                board_array_words,
                board_row_masks
            ));
        }

        info!(
            target: "EAPP_PC_TRACE",
            "pc={:#010x} hit={} frame={} title={} {}",
            pc,
            hit,
            self.frame_counter,
            self.metadata.title,
            details
        );
    }

    /// Best-effort diagnostic dump of the AsyncFileIO request-object layout.
    /// Used to reverse-engineer where the guest expects file payload/length to
    /// be written. Logged once per request object address.
    fn dump_request_object(&mut self, req: u32) {
        if req == 0 || !self.dumped_requests.insert(req) {
            return;
        }
        let fields: [(usize, &str); 16] = [
            (0x00, "[0x00]"),
            (0x04, "[0x04] type"),
            (0x08, "[0x08]"),
            (0x0c, "[0x0c]"),
            (0x10, "[0x10]"),
            (0x14, "[0x14] arg2"),
            (0x18, "[0x18] arg3"),
            (0x1c, "[0x1c]"),
            (0x20, "[0x20]"),
            (0x24, "[0x24]"),
            (0x28, "[0x28]"),
            (0x2c, "[0x2c]"),
            (0x30, "[0x30]"),
            (0x34, "[0x34] cb_pc"),
            (0x38, "[0x38] cb_ctx"),
            (0x3c, "[0x3c]"),
        ];
        let mut rendered = String::new();
        for (off, label) in fields.iter() {
            let val = self
                .read_guest_u32(req.wrapping_add(*off as u32))
                .unwrap_or(0xdeadbeef);
            rendered.push_str(&format!("\n    {} {:#010x}", label, val));
        }
        info!(target: "EAPP", "request object @ {:#010x}:{}", req, rendered);
    }

    fn handle_guest_svc(&mut self, pc: u32) -> bool {
        if self.read_guest_u32(pc) != Some(0xef12_3456) {
            return false;
        }

        let call_num = self.cpu.reg_get(self.cpu.mode(), 0);
        let arg_ptr = self.cpu.reg_get(self.cpu.mode(), 1);
        match call_num {
            3 => {
                let ch = self.read_guest_u8(arg_ptr).unwrap_or_default();
                debug!(target: "EAPP", "svc: putchar {:?}", ch as char);
                self.cpu.reg_set(self.cpu.mode(), 0, ch as u32);
            }
            1 | 2 | 5 | 6 | 9 | 10 | 12 | 24 => {
                debug!(target: "EAPP", "svc: call {} arg_ptr={:#010x}", call_num, arg_ptr);
                self.cpu.reg_set(self.cpu.mode(), 0, 0);
            }
            other => {
                warn!(target: "EAPP", "unhandled guest svc call {} at pc={:#010x}", other, pc);
                self.cpu.reg_set(self.cpu.mode(), 0, 0);
            }
        }

        self.cpu
            .reg_set(self.cpu.mode(), reg::PC, pc.wrapping_add(4));
        true
    }

    fn maybe_patch_guest_state(&mut self, pc: u32) {
        if self.metadata.title != "66666" {
            return;
        }
        if !(0x18013d4c..=0x18014020).contains(&pc) {
            return;
        }

        let owner = match self.cpu.reg_get(self.cpu.mode(), 9) {
            0 => return,
            addr => addr,
        };
        let array = match self.read_guest_u32(owner.wrapping_add(8)) {
            Some(0) | None => return,
            Some(addr) => addr,
        };

        let mut patched = 0;
        for idx in 20..=37u32 {
            let slot_addr = array.wrapping_add(idx * 4);
            if self.read_guest_u32(slot_addr).unwrap_or(0) != 0 {
                continue;
            }
            let entry = self.alloc_zeroed(0x20);
            let payload = self.alloc_zeroed(0x200);
            if entry == 0 || payload == 0 {
                break;
            }
            if !self.write_guest_u32(slot_addr, entry) {
                break;
            }
            // The placeholders stand in for resource entries the guest later
            // treats as ref-counted runtime objects. Initialize the minimal
            // base-object header so normal retain/release paths can safely
            // decrement and destroy them instead of calling through a NULL
            // vtable when input/state transitions release copied slots.
            let _ = self.write_guest_u32(entry, 0x1802_3efc);
            let _ = self.write_guest_u32(entry.wrapping_add(4), 1);
            let _ = self.write_guest_u32(entry.wrapping_add(8), payload);
            patched += 1;
        }

        if patched > 0 {
            warn!(
                target: "EAPP",
                "patched {} placeholder Tetris resource slots at owner={:#010x} array={:#010x}",
                patched,
                owner,
                array
            );
        }
    }

    fn resolve_bundle_path(&self, path: &str) -> Option<PathBuf> {
        let normalized = path.trim_start_matches('/').trim_start_matches('\\');
        for candidate in [path, normalized] {
            if candidate.is_empty() {
                continue;
            }
            let direct = self.metadata.bundle_dir.join(candidate);
            if direct.exists() {
                return Some(direct);
            }
            let resources = self.metadata.bundle_dir.join("Resources").join(candidate);
            if resources.exists() {
                return Some(resources);
            }
        }
        for save_dir in [SAVE_DIR, LEGACY_SAVE_DIR] {
            let saved = self.metadata.bundle_dir.join(save_dir).join(normalized);
            if saved.exists() {
                return Some(saved);
            }
        }
        None
    }

    fn resolve_or_create_host_path(&self, path: &str) -> Option<PathBuf> {
        if let Some(found) = self.resolve_bundle_path(path) {
            return Some(found);
        }

        let normalized = path.trim_start_matches('/').trim_start_matches('\\');
        if normalized.is_empty() {
            return None;
        }

        let writable = self.metadata.bundle_dir.join(SAVE_DIR).join(normalized);
        if let Some(parent) = writable.parent() {
            fs::create_dir_all(parent).ok()?;
        }
        if !writable.exists() {
            fs::write(&writable, []).ok()?;
        }
        Some(writable)
    }

    fn try_read_c_string(&mut self, addr: u32, max_len: usize) -> Option<String> {
        if addr == 0 {
            return None;
        }
        let mut bytes = Vec::new();
        for i in 0..max_len {
            let b = self.bus.r8(addr.wrapping_add(i as u32)).ok()?;
            if b == 0 {
                break;
            }
            if !(0x20..=0x7e).contains(&b) && b != b'/' && b != b'\\' && b != b'_' && b != b'.' {
                return None;
            }
            bytes.push(b);
        }
        if bytes.is_empty() {
            return None;
        }
        String::from_utf8(bytes).ok()
    }

    fn record_pc(&mut self, pc: u32) {
        if self.recent_pcs.back().copied() == Some(pc) {
            return;
        }
        if self.recent_pcs.len() == RECENT_PC_LIMIT {
            self.recent_pcs.pop_front();
        }
        self.recent_pcs.push_back(pc);
    }

    fn format_recent_pcs(&self) -> String {
        self.recent_pcs
            .iter()
            .map(|pc| format!("{:#010x}", pc))
            .collect::<Vec<_>>()
            .join(" -> ")
    }
}

fn texgen_verbose_enabled() -> bool {
    fliwheel_var_os("GL_TEXGEN_VERBOSE")
        .map(|v| v.to_string_lossy() == "1")
        .unwrap_or(false)
}

fn string_trace_enabled() -> bool {
    std::env::var_os("EAPP_STRING_TRACE")
        .map(|v| v.to_string_lossy() == "1")
        .unwrap_or(false)
}

fn array_summary(def: Option<&live_gl::LiveArrayDef>) -> String {
    match def {
        Some(def) => format!(
            "idx={} comps={} fmt={:#x} stride={} ptr={:#010x} valid={} epoch={}",
            def.array_index,
            def.component_count,
            def.format,
            def.stride,
            def.guest_ptr,
            def.valid,
            def.material_epoch
        ),
        None => "<none>".to_string(),
    }
}

fn upload_summary(upload: &live_gl::LiveGlUpload) -> String {
    format!(
        "upload={} file={} file_off={} dim={}x{} format={:?} src_fmt={:#x} pix_type={:#x}",
        upload.index,
        upload.source_file.as_deref().unwrap_or("<unknown>"),
        upload
            .source_file_offset
            .map(|off| off.to_string())
            .unwrap_or_else(|| "<unknown>".to_string()),
        upload.width,
        upload.height,
        upload.format,
        upload.source_format,
        upload.pixel_type
    )
}

impl EappBus {
    fn watch_frame_enabled(&self) -> bool {
        self.watch_frame_window
            .is_none_or(|(start, end)| (start..=end).contains(&self.watch_frame))
    }

    fn record_watch_hit(&mut self, width: u8, addr: u32, val: u32) {
        let Some((start, end)) = self.watch else {
            return;
        };
        if !self.watch_frame_enabled() || !ranges_overlap(addr, width as u32, start, end) {
            return;
        }
        if self.watch_source_dump_dir.is_some() && !self.watch_source_dumped {
            self.dump_watch_source();
        }
        self.watch_log.push(WatchHit {
            addr,
            val,
            pc: self.pending_pc,
            frame: self.watch_frame,
            regs: self.capture_watch_regs(),
        });
        // Hard cap to avoid OOM on a flooding range.
        if self.watch_log.len() > 4096 {
            self.watch_log.truncate(4096);
        }
    }

    fn capture_watch_regs(&self) -> Option<WatchRegs> {
        self.watch_regs.then_some(WatchRegs {
            r0: self.pending_regs[0],
            r1: self.pending_regs[1],
            r2: self.pending_regs[2],
            r3: self.pending_regs[3],
            lr: self.pending_lr,
            sp: self.pending_sp,
            prev_pc: self.previous_pc,
            prev_lr: self.previous_lr,
        })
    }

    fn trace_memcpy(&mut self, pc: u32) {
        if self.memcpy_trace_remaining == 0
            || !matches!(
                pc,
                0x1800_1668 | 0x1800_170c | 0x1800_171c | 0x1800_1788 | 0x1800_17b4
            )
        {
            return;
        }
        let Some((start, end)) = self.watch else {
            return;
        };
        let destination = self.pending_regs[0];
        if !ranges_overlap(destination, 4, start, end) {
            return;
        }
        self.memcpy_trace_remaining -= 1;
        info!(
            target: "EAPP_HW",
            "memcpy pc={:#010x} dst={:#010x} src={:#010x} len_or_state={:#010x} r3={:#010x} lr={:#010x} prev_pc={:#010x}",
            pc,
            destination,
            self.pending_regs[1],
            self.pending_regs[2],
            self.pending_regs[3],
            self.pending_lr,
            self.previous_pc,
        );
    }

    fn trace_watch_read(&mut self, width: u8, offset: u32, value: u32) {
        if self.watch_read_remaining == 0 {
            return;
        }
        if !self.watch_frame_enabled() {
            return;
        }
        let Some((start, end)) = self.watch else {
            return;
        };
        if !ranges_overlap(offset, width as u32, start, end) {
            return;
        }
        self.watch_read_remaining -= 1;
        let regs = if self.watch_regs {
            format!(
                " r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} lr={:#010x} sp={:#010x} prev_pc={:#010x} prev_lr={:#010x}",
                self.pending_regs[0],
                self.pending_regs[1],
                self.pending_regs[2],
                self.pending_regs[3],
                self.pending_lr,
                self.pending_sp,
                self.previous_pc,
                self.previous_lr,
            )
        } else {
            String::new()
        };
        info!(
            target: "EAPP_WATCH_READ",
            "frame={} read w{} addr={:#010x} val={:#010x} pc={:#010x} remaining={}{}",
            self.watch_frame,
            width,
            offset,
            value,
            self.pending_pc,
            self.watch_read_remaining,
            regs,
        );
    }

    fn trace_hw_read(&mut self, width: u8, offset: u32, value: u32) {
        let remaining = if self.hw_trace_read_remaining > 0 {
            self.hw_trace_read_remaining -= 1;
            self.hw_trace_read_remaining
        } else if self.hw_trace_remaining > 0 {
            self.hw_trace_remaining -= 1;
            self.hw_trace_remaining
        } else {
            return;
        };
        let rel = offset.wrapping_sub(HW_STUB_BASE);
        let kind = if rel < 0x20000 {
            format!("control+{:#06x}", rel)
        } else {
            format!("framebuf+{:#08x}", rel - 0x20000)
        };
        let regs = if self.hw_trace_regs {
            format!(
                " r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} lr={:#010x} sp={:#010x} prev_pc={:#010x} prev_lr={:#010x}",
                self.pending_regs[0],
                self.pending_regs[1],
                self.pending_regs[2],
                self.pending_regs[3],
                self.pending_lr,
                self.pending_sp,
                self.previous_pc,
                self.previous_lr,
            )
        } else {
            String::new()
        };
        info!(
            target: "EAPP_HW",
            "hw r{} addr={:#010x} {} val={:#010x} pc={:#010x} remaining={}{}",
            width,
            offset,
            kind,
            value,
            self.pending_pc,
            remaining,
            regs,
        );
    }

    fn trace_hw_write(&mut self, width: u8, offset: u32, value: u32) {
        if self.hw_trace_remaining == 0 {
            return;
        }
        self.hw_trace_remaining -= 1;
        let rel = offset.wrapping_sub(HW_STUB_BASE);
        let kind = if rel < 0x20000 {
            format!("control+{:#06x}", rel)
        } else {
            format!("framebuf+{:#08x}", rel - 0x20000)
        };
        let regs = if self.hw_trace_regs {
            format!(
                " r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} lr={:#010x} sp={:#010x} prev_pc={:#010x} prev_lr={:#010x}",
                self.pending_regs[0],
                self.pending_regs[1],
                self.pending_regs[2],
                self.pending_regs[3],
                self.pending_lr,
                self.pending_sp,
                self.previous_pc,
                self.previous_lr,
            )
        } else {
            String::new()
        };
        info!(
            target: "EAPP_HW",
            "hw w{} addr={:#010x} {} val={:#010x} pc={:#010x} remaining={}{}",
            width,
            offset,
            kind,
            value,
            self.pending_pc,
            self.hw_trace_remaining,
            regs,
        );
    }

    fn dump_dma_source(&self) {
        let Some(dir) = self.dma_source_dump_dir.as_ref() else {
            return;
        };
        let source_start = self.pending_regs[1].wrapping_sub(16);
        self.dump_guest_range(dir, "dma_source", source_start, DMA_FB_SIZE);
        self.dump_guest_range(
            dir,
            "dma_surface",
            source_start.saturating_sub(12),
            DMA_FB_SIZE + 12,
        );
    }

    fn dump_watch_source(&mut self) {
        let Some(dir) = self.watch_source_dump_dir.as_ref() else {
            return;
        };
        let source_start = self.pending_regs[1].wrapping_sub(16);
        self.dump_guest_range(dir, "watch_source", source_start, DMA_FB_SIZE);
        self.watch_source_dumped = true;
    }

    fn dump_guest_range(&self, dir: &Path, label: &str, source_start: u32, len: usize) {
        if len > u32::MAX as usize {
            warn!(
                target: "EAPP_HW",
                "guest source dump skipped source={:#010x} len={:#x}",
                source_start,
                len
            );
            return;
        }
        let len_u32 = len as u32;
        let Some(source_end) = source_start.checked_add(len_u32) else {
            warn!(
                target: "EAPP_HW",
                "guest source dump skipped source={:#010x} len={:#x}",
                source_start,
                len
            );
            return;
        };

        let image_end = FILE_VMA_BASE.saturating_add(self.image_len);
        let work_end = WORK_RAM_BASE.saturating_add(WORK_RAM_SIZE as u32);
        let (region, offset, extension) =
            if source_start >= FILE_VMA_BASE && source_end <= image_end {
                ("file-backed", source_start - FILE_VMA_BASE, "bin")
            } else if source_start >= WORK_RAM_BASE && source_end <= work_end {
                ("work-RAM", source_start - WORK_RAM_BASE, "rgb565")
            } else {
                warn!(
                    target: "EAPP_HW",
                    "guest source dump skipped source={:#010x} len={:#x}",
                    source_start,
                    len
                );
                return;
            };

        let _ = fs::create_dir_all(dir);
        let path = dir.join(format!("{label}_{source_start:#010x}.{extension}"));
        if path.exists() {
            return;
        }
        let mut data = vec![0u8; len];
        if region == "file-backed" {
            self.image.bulk_read(offset, &mut data);
        } else {
            self.work_ram.bulk_read(offset, &mut data);
        }
        if let Err(err) = fs::write(&path, &data) {
            warn!(
                target: "EAPP_HW",
                "guest source dump failed path={} error={}",
                path.display(),
                err
            );
        } else {
            info!(
                target: "EAPP_HW",
                "guest source dump path={} region={} source={:#010x} bytes={}",
                path.display(),
                region,
                source_start,
                data.len()
            );
        }
    }
}

impl TakeControls for Eapp {
    type Controls = EappBinds;

    fn take_controls(&mut self) -> Option<Self::Controls> {
        self.controls.take()
    }
}

impl EappImage {
    pub fn load(metadata: EappMetadata) -> Result<EappImage, EappBuildError> {
        let image = fs::read(&metadata.executable_path)?;
        let header = parse_eapp_header(&image)?;
        let imports = parse_import_modules(&image, header.imports_addr)?;
        Ok(EappImage {
            metadata,
            header,
            imports,
            image,
        })
    }
}

impl Device for EappBus {
    fn kind(&self) -> &'static str {
        "EappBus"
    }

    fn probe(&self, offset: u32) -> Probe {
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => Probe::Device {
                kind: "Ram",
                label: Some("eapp-image"),
                next: Box::new(self.image.probe(offset - FILE_VMA_BASE)),
            },
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                Probe::Device {
                    kind: "Ram",
                    label: Some("eapp-work"),
                    next: Box::new(self.work_ram.probe(offset - WORK_RAM_BASE)),
                }
            }
            GAME_HEAP_BASE..=u32::MAX if offset - GAME_HEAP_BASE < GAME_HEAP_SIZE as u32 => {
                Probe::Device {
                    kind: "Ram",
                    label: Some("eapp-game-heap"),
                    next: Box::new(self.game_heap.probe(offset - GAME_HEAP_BASE)),
                }
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                Probe::Device {
                    kind: "HWStub",
                    label: Some("eapp-hw-stub"),
                    next: Box::new(Probe::Unmapped),
                }
            }
            _ => Probe::Unmapped,
        }
    }
}

fn ranges_overlap(access_start: u32, access_len: u32, watch_start: u32, watch_end: u32) -> bool {
    let access_end = access_start.saturating_add(access_len);
    access_start < watch_end && watch_start < access_end
}

impl Memory for EappBus {
    fn r32(&mut self, offset: u32) -> MemResult<u32> {
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => {
                let value = self.image.r32(offset - FILE_VMA_BASE)?;
                self.trace_watch_read(4, offset, value);
                Ok(value)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                let val = self.work_ram.r32(offset - WORK_RAM_BASE)?;
                // Watch reads from the rserver header/init region (Lost game)
                // Rserver loaded at 0x10001038, header at 0x10001038..0x10001237
                // Data at 0x10012038
                // rserver_watch removed (too noisy) — use ordinal-level tracing instead
                self.trace_watch_read(4, offset, val);
                Ok(val)
            }
            GAME_HEAP_BASE..=u32::MAX if offset - GAME_HEAP_BASE < GAME_HEAP_SIZE as u32 => {
                let value = self.game_heap.r32(offset - GAME_HEAP_BASE)?;
                self.trace_watch_read(4, offset, value);
                Ok(value)
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                let rel = offset - HW_STUB_BASE;
                if self.uncached_sdram_alias {
                    let value = self.work_ram.r32(rel)?;
                    self.trace_hw_read(4, offset, value);
                    self.trace_watch_read(4, offset, value);
                    return Ok(value);
                }
                let value: u32 = if rel < 0x20000 {
                    // DMA control registers
                    self.hw_control_value
                } else {
                    // DMA framebuffer: read back stored pixel data
                    let fb_off = (rel - 0x20000) as u32;
                    if (fb_off as usize) + 4 <= DMA_FB_SIZE {
                        let mut buf = [0u8; 4];
                        self.dma_framebuf.bulk_read(fb_off, &mut buf);
                        u32::from_le_bytes(buf)
                    } else {
                        0
                    }
                };
                self.trace_hw_read(4, offset, value);
                self.trace_watch_read(4, offset, value);
                Ok(value)
            }
            _ if self.zero_unmapped_reads => Ok(0),
            _ => Err(MemException::Unexpected),
        }
    }

    fn w32(&mut self, offset: u32, val: u32) -> MemResult<()> {
        self.record_watch_hit(4, offset, val);
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => {
                self.image.w32(offset - FILE_VMA_BASE, val)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                self.work_ram.w32(offset - WORK_RAM_BASE, val)
            }
            GAME_HEAP_BASE..=u32::MAX if offset - GAME_HEAP_BASE < GAME_HEAP_SIZE as u32 => {
                self.game_heap.w32(offset - GAME_HEAP_BASE, val)
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                self.trace_hw_write(4, offset, val);
                let rel = offset - HW_STUB_BASE;
                if rel == 0x20000 && self.hw_fb_write_count == 0 {
                    self.dump_dma_source();
                }
                if self.uncached_sdram_alias {
                    self.work_ram.w32(rel, val)?;
                    if rel >= 0x20000 {
                        let fb_off = rel - 0x20000;
                        if (fb_off as usize) + 4 <= DMA_FB_SIZE {
                            self.dma_framebuf.bulk_write(fb_off, &val.to_le_bytes());
                            self.hw_fb_write_count += 1;
                            self.hw_dma_dirty = true;
                            if fb_off < self.hw_fb_write_min {
                                self.hw_fb_write_min = fb_off;
                            }
                            if fb_off + 4 > self.hw_fb_write_max {
                                self.hw_fb_write_max = fb_off + 4;
                            }
                            if fb_off == 0 && self.hw_fb_write_count > 1 {
                                self.hw_dma_frame += 1;
                            }
                        }
                    }
                    return Ok(());
                }
                if rel < 0x20000 {
                    // DMA control register writes
                } else {
                    // DMA framebuffer pixel storage
                    let fb_off = (rel - 0x20000) as u32;
                    if (fb_off as usize) + 4 <= DMA_FB_SIZE {
                        self.dma_framebuf.bulk_write(fb_off, &val.to_le_bytes());
                        self.hw_fb_write_count += 1;
                        self.hw_dma_dirty = true;
                        if fb_off < self.hw_fb_write_min {
                            self.hw_fb_write_min = fb_off;
                        }
                        if fb_off + 4 > self.hw_fb_write_max {
                            self.hw_fb_write_max = fb_off + 4;
                        }
                        // Detect new DMA frame: first pixel rewritten
                        if fb_off == 0 && self.hw_fb_write_count > 1 {
                            self.hw_dma_frame += 1;
                        }
                    }
                }
                Ok(())
            }
            _ if self.zero_unmapped_writes => Ok(()),
            _ => Err(MemException::Unexpected),
        }
    }

    fn r8(&mut self, offset: u32) -> MemResult<u8> {
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => {
                let value = self.image.r8(offset - FILE_VMA_BASE)?;
                self.trace_watch_read(1, offset, value as u32);
                Ok(value)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                let value = self.work_ram.r8(offset - WORK_RAM_BASE)?;
                self.trace_watch_read(1, offset, value as u32);
                Ok(value)
            }
            GAME_HEAP_BASE..=u32::MAX if offset - GAME_HEAP_BASE < GAME_HEAP_SIZE as u32 => {
                let value = self.game_heap.r8(offset - GAME_HEAP_BASE)?;
                self.trace_watch_read(1, offset, value as u32);
                Ok(value)
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                let rel = offset - HW_STUB_BASE;
                if self.uncached_sdram_alias {
                    let value = self.work_ram.r8(rel)?;
                    self.trace_hw_read(1, offset, value as u32);
                    self.trace_watch_read(1, offset, value as u32);
                    return Ok(value);
                }
                let value: u8 = if rel < 0x20000 {
                    self.hw_control_value as u8
                } else {
                    let fb_off = (rel - 0x20000) as usize;
                    if fb_off < DMA_FB_SIZE {
                        let mut buf = [0u8; 1];
                        self.dma_framebuf.bulk_read(fb_off as u32, &mut buf);
                        buf[0]
                    } else {
                        0
                    }
                };
                self.trace_hw_read(1, offset, value as u32);
                self.trace_watch_read(1, offset, value as u32);
                Ok(value)
            }
            _ if self.zero_unmapped_reads => Ok(0),
            _ => Err(MemException::Unexpected),
        }
    }

    fn r16(&mut self, offset: u32) -> MemResult<u16> {
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => {
                let value = self.image.r16(offset - FILE_VMA_BASE)?;
                self.trace_watch_read(2, offset, value as u32);
                Ok(value)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                let value = self.work_ram.r16(offset - WORK_RAM_BASE)?;
                self.trace_watch_read(2, offset, value as u32);
                Ok(value)
            }
            GAME_HEAP_BASE..=u32::MAX if offset - GAME_HEAP_BASE < GAME_HEAP_SIZE as u32 => {
                let value = self.game_heap.r16(offset - GAME_HEAP_BASE)?;
                self.trace_watch_read(2, offset, value as u32);
                Ok(value)
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                let rel = offset - HW_STUB_BASE;
                if self.uncached_sdram_alias {
                    let value = self.work_ram.r16(rel)?;
                    self.trace_hw_read(2, offset, value as u32);
                    self.trace_watch_read(2, offset, value as u32);
                    return Ok(value);
                }
                let value: u16 = if rel < 0x20000 {
                    self.hw_control_value as u16
                } else {
                    let fb_off = (rel - 0x20000) as usize;
                    if fb_off + 2 <= DMA_FB_SIZE {
                        let mut buf = [0u8; 2];
                        self.dma_framebuf.bulk_read(fb_off as u32, &mut buf);
                        u16::from_le_bytes(buf)
                    } else {
                        0
                    }
                };
                self.trace_hw_read(2, offset, value as u32);
                self.trace_watch_read(2, offset, value as u32);
                Ok(value)
            }
            _ if self.zero_unmapped_reads => Ok(0),
            _ => Err(MemException::Unexpected),
        }
    }

    fn w8(&mut self, offset: u32, val: u8) -> MemResult<()> {
        self.record_watch_hit(1, offset, val as u32);
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => {
                self.image.w8(offset - FILE_VMA_BASE, val)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                self.work_ram.w8(offset - WORK_RAM_BASE, val)
            }
            GAME_HEAP_BASE..=u32::MAX if offset - GAME_HEAP_BASE < GAME_HEAP_SIZE as u32 => {
                self.game_heap.w8(offset - GAME_HEAP_BASE, val)
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                self.trace_hw_write(1, offset, val as u32);
                let rel = offset - HW_STUB_BASE;
                if self.uncached_sdram_alias {
                    self.work_ram.w8(rel, val)?;
                    if rel >= 0x20000 {
                        let fb_off = rel - 0x20000;
                        if (fb_off as usize) < DMA_FB_SIZE {
                            self.dma_framebuf.bulk_write(fb_off, &[val]);
                            self.hw_dma_dirty = true;
                        }
                    }
                    return Ok(());
                }
                if rel >= 0x20000 {
                    let fb_off = (rel - 0x20000) as u32;
                    if (fb_off as usize) < DMA_FB_SIZE {
                        self.dma_framebuf.bulk_write(fb_off, &[val]);
                        self.hw_dma_dirty = true;
                    }
                }
                Ok(())
            }
            _ if self.zero_unmapped_writes => Ok(()),
            _ => Err(MemException::Unexpected),
        }
    }

    fn w16(&mut self, offset: u32, val: u16) -> MemResult<()> {
        self.record_watch_hit(2, offset, val as u32);
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => {
                self.image.w16(offset - FILE_VMA_BASE, val)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                self.work_ram.w16(offset - WORK_RAM_BASE, val)
            }
            GAME_HEAP_BASE..=u32::MAX if offset - GAME_HEAP_BASE < GAME_HEAP_SIZE as u32 => {
                self.game_heap.w16(offset - GAME_HEAP_BASE, val)
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                self.trace_hw_write(2, offset, val as u32);
                let rel = offset - HW_STUB_BASE;
                if self.uncached_sdram_alias {
                    self.work_ram.w16(rel, val)?;
                    if rel >= 0x20000 {
                        let fb_off = rel - 0x20000;
                        if (fb_off as usize) + 2 <= DMA_FB_SIZE {
                            self.dma_framebuf.bulk_write(fb_off, &val.to_le_bytes());
                            self.hw_dma_dirty = true;
                        }
                    }
                    return Ok(());
                }
                if rel >= 0x20000 {
                    let fb_off = (rel - 0x20000) as u32;
                    if (fb_off as usize) + 2 <= DMA_FB_SIZE {
                        self.dma_framebuf.bulk_write(fb_off, &val.to_le_bytes());
                        self.hw_dma_dirty = true;
                    }
                }
                Ok(())
            }
            _ if self.zero_unmapped_writes => Ok(()),
            _ => Err(MemException::Unexpected),
        }
    }

    fn x16(&mut self, offset: u32) -> MemResult<u16> {
        self.r16(offset)
    }

    fn x32(&mut self, offset: u32) -> MemResult<u32> {
        self.r32(offset)
    }
}

/// Best-effort reader for a binary P6 PPM (used by the optional live-vs-offline
/// pixel diff). Returns the decoded RGBA8 pixel buffer or None on any parse
/// error. Only supports the exact format written by `framebuffer_to_ppm`.
fn read_ppm_p6(path: &std::path::Path) -> Option<Vec<Rgba8>> {
    let bytes = std::fs::read(path).ok()?;
    if !bytes.starts_with(b"P6") {
        return None;
    }
    let mut idx = 2usize;
    let mut fields = Vec::new();
    while fields.len() < 3 {
        // skip whitespace
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx < bytes.len() && bytes[idx] == b'#' {
            while idx < bytes.len() && bytes[idx] != b'\n' {
                idx += 1;
            }
            continue;
        }
        let start = idx;
        while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        let tok = std::str::from_utf8(&bytes[start..idx]).ok()?;
        fields.push(tok.parse::<u32>().ok()?);
        if fields.len() == 3 {
            // skip single whitespace after maxval
            idx += 1;
            break;
        }
    }
    let width = fields[0] as usize;
    let height = fields[1] as usize;
    let _maxval = fields[2];
    let payload = &bytes[idx..];
    let need = width * height * 3;
    if payload.len() < need {
        return None;
    }
    let mut out = Vec::with_capacity(width * height);
    for px in payload[..need].chunks_exact(3) {
        out.push(Rgba8::rgba(px[0], px[1], px[2], 255));
    }
    Some(out)
}

fn make_controls(input_state: Arc<Mutex<EappInputState>>) -> EappBinds {
    let mut controls = EappBinds::default();

    macro_rules! bind_key {
        ($key:expr, $field:ident) => {
            let state = Arc::clone(&input_state);
            controls.keys.insert(
                $key,
                Box::new(move |pressed| {
                    state.lock().unwrap().$field = pressed;
                }),
            );
        };
    }

    bind_key!(EappKey::Up, up);
    bind_key!(EappKey::Down, down);
    bind_key!(EappKey::Left, left);
    bind_key!(EappKey::Right, right);
    bind_key!(EappKey::Action, action);
    bind_key!(EappKey::Menu, menu);

    let state = Arc::clone(&input_state);
    controls.wheel = Some(Box::new(move |(_dx, dy)| {
        state.lock().unwrap().wheel_delta += dy;
    }));

    controls
}

fn find_game_executable(bundle_dir: &Path) -> Result<PathBuf, EappBuildError> {
    let exe_dir = bundle_dir.join("Executables");
    let mut bins = fs::read_dir(&exe_dir)
        .map_err(|_| EappBuildError::MissingExecutable(bundle_dir.display().to_string()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            let name_is_real = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| !name.starts_with('.') && !name.starts_with("._"))
                .unwrap_or(false);
            name_is_real && path.extension().map(|ext| ext == "bin").unwrap_or(false)
        })
        .collect::<Vec<_>>();
    bins.sort();
    bins.into_iter()
        .next()
        .ok_or_else(|| EappBuildError::MissingExecutable(bundle_dir.display().to_string()))
}

/// Pixel-coordinate titles do not all share the same screen origin. The
/// PopCap pair submits screen-space geometry in top-to-bottom order, while
/// the Tetris-style runtime presents its framebuffer bottom-to-top. Keep the
/// established Tetris default, but make the evidence-backed PopCap choice
/// automatic; `FLIWHEEL_GL_PRESENT_VFLIP` remains an explicit override.
fn live_gl_default_present_vflip(game_id: &str) -> bool {
    !matches!(game_id, "44444" | "55555" | "zuma" | "bejeweled")
}

fn parse_eapp_header(image: &[u8]) -> Result<EappHeader, EappBuildError> {
    if image.len() < EAPP_HEADER_SIZE {
        return Err(EappBuildError::InvalidImage(
            "file too small for eapp header".into(),
        ));
    }
    if &image[0..4] != b"eapp" {
        return Err(EappBuildError::InvalidImage("missing eapp magic".into()));
    }

    let load_addr_guess = read_u32_at(image, 0x04)?;
    let format_version = read_u32_at(image, 0x08)?;
    let header_size = read_u32_at(image, 0x0c)?;
    let imports_addr = read_u32_at(image, 0x10)?;
    let entry_addr = read_u32_at(image, 0x14)?;
    let init_addr = read_u32_at(image, 0x18)?;
    let aux_addr = read_u32_at(image, 0x24)?;

    Ok(EappHeader {
        load_addr_guess,
        format_version,
        header_size,
        imports_addr,
        entry_addr,
        init_addr,
        aux_addr,
    })
}

fn parse_import_modules(
    image: &[u8],
    mut name_addr: u32,
) -> Result<Vec<EappImportModule>, EappBuildError> {
    let mut modules = Vec::new();
    let mut seen = HashSet::new();

    while name_addr != 0 {
        if !seen.insert(name_addr) {
            return Err(EappBuildError::InvalidImage(format!(
                "import descriptor loop at {:#010x}",
                name_addr
            )));
        }

        let name_offset = vma_to_offset(name_addr)? as usize;
        let name_bytes = image
            .get(name_offset..name_offset + IMPORT_NAME_LEN)
            .ok_or_else(|| EappBuildError::InvalidImage("truncated import name".into()))?;
        let name = c_string(name_bytes)?;
        let count = read_u32_at(image, name_offset + IMPORT_COUNT_OFFSET)?;
        let next_addr = read_u32_at(image, name_offset + IMPORT_NEXT_OFFSET)?;
        let stubs_addr = name_addr + IMPORT_STUBS_OFFSET as u32;
        let literals_addr = stubs_addr + count * 4;

        if name == IMPORT_SENTINEL_NAME {
            break;
        }

        modules.push(EappImportModule {
            name_addr,
            name,
            count,
            next_addr,
            stubs_addr,
            literals_addr,
        });
        name_addr = next_addr;
    }

    Ok(modules)
}

fn c_string(bytes: &[u8]) -> Result<String, EappBuildError> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let slice = &bytes[..end];
    String::from_utf8(slice.to_vec())
        .map_err(|_| EappBuildError::InvalidImage("non-utf8 import name".into()))
}

fn read_u32_at(image: &[u8], offset: usize) -> Result<u32, EappBuildError> {
    let bytes = image
        .get(offset..offset + 4)
        .ok_or_else(|| EappBuildError::InvalidImage(format!("truncated u32 at {:#x}", offset)))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn vma_to_offset(addr: u32) -> Result<u32, EappBuildError> {
    addr.checked_sub(FILE_VMA_BASE).ok_or_else(|| {
        EappBuildError::InvalidImage(format!("address {:#010x} is outside file VMA", addr))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        decode_palette8, is_palette8_compressed_upload_call, live_gl_default_present_vflip,
        matrix_helper_transform, orthographic_matrix, Eapp, EappInputState, GL_IDENTITY_MATRIX,
        GL_PALETTE8_R5_G6_B5_OES, GL_PALETTE8_RGBA8_OES, GL_TEXTURE_2D, GL_TEXTURE_RECTANGLE,
    };

    #[test]
    fn palette8_rgba8_decodes_palette_then_indices() {
        let mut raw = vec![0; 1024 + 3];
        raw[0..4].copy_from_slice(&[1, 2, 3, 4]);
        raw[4..8].copy_from_slice(&[10, 20, 30, 40]);
        raw[8..12].copy_from_slice(&[50, 60, 70, 80]);
        raw[1024..].copy_from_slice(&[0, 1, 2]);

        assert_eq!(
            decode_palette8(&raw, 3, 1, GL_PALETTE8_RGBA8_OES),
            Some(vec![
                super::Rgba8::rgba(1, 2, 3, 4),
                super::Rgba8::rgba(10, 20, 30, 40),
                super::Rgba8::rgba(50, 60, 70, 80),
            ])
        );
    }

    #[test]
    fn palette8_rgba8_rejects_short_data() {
        assert_eq!(
            decode_palette8(&[0; 1024], 1, 1, GL_PALETTE8_RGBA8_OES),
            None
        );
    }

    #[test]
    fn palette8_rgb565_decodes_two_byte_palette_then_indices() {
        let mut raw = vec![0; 512 + 3];
        // Red, green, and blue in little-endian RGB565.
        raw[0..2].copy_from_slice(&[0x00, 0xf8]);
        raw[2..4].copy_from_slice(&[0xe0, 0x07]);
        raw[4..6].copy_from_slice(&[0x1f, 0x00]);
        raw[512..].copy_from_slice(&[0, 1, 2]);

        assert_eq!(
            decode_palette8(&raw, 3, 1, GL_PALETTE8_R5_G6_B5_OES),
            Some(vec![
                super::Rgba8::rgba(255, 0, 0, 255),
                super::Rgba8::rgba(0, 255, 0, 255),
                super::Rgba8::rgba(0, 0, 255, 255),
            ])
        );
    }

    #[test]
    fn palette8_compressed_upload_accepts_rectangle_and_2d_targets() {
        assert!(is_palette8_compressed_upload_call([
            GL_TEXTURE_2D,
            0,
            GL_PALETTE8_RGBA8_OES,
            1,
        ]));
        assert!(is_palette8_compressed_upload_call([
            GL_TEXTURE_RECTANGLE,
            0,
            GL_PALETTE8_RGBA8_OES,
            1,
        ]));
        assert!(is_palette8_compressed_upload_call([
            GL_TEXTURE_RECTANGLE,
            0,
            GL_PALETTE8_R5_G6_B5_OES,
            1,
        ]));
        assert!(!is_palette8_compressed_upload_call([0x84f5, 0, 0x1908, 1]));
    }

    #[test]
    fn wheel_packet_normalizes_physical_96_detents_to_guest_ring() {
        let mut position = 0;

        assert_eq!(Eapp::wheel_frame_bits(&mut position, -1.0), 0x4000_0005);
        assert_eq!(position, 2);
        assert_eq!(Eapp::wheel_frame_bits(&mut position, 1.0), 0x4000_0000);
        assert_eq!(position, 0);
    }

    #[test]
    fn normalized_wheel_position_preserves_quadrant_boundaries() {
        assert_eq!(Eapp::normalized_wheel_position(0), 0x00);
        assert_eq!(Eapp::normalized_wheel_position(24), 0x40);
        assert_eq!(Eapp::normalized_wheel_position(48), 0x80);
        assert_eq!(Eapp::normalized_wheel_position(72), 0xc0);
        assert_eq!(Eapp::normalized_wheel_position(95), 0xfd);
    }

    #[test]
    fn wheel_packet_omits_idle_reports() {
        let mut position = 17;

        assert_eq!(Eapp::wheel_frame_bits(&mut position, 0.0), 0);
        assert_eq!(position, 17);
    }

    #[test]
    fn lost_wheel_encoder_matches_pr3_direct_runner() {
        assert_eq!(Eapp::lost_wheel_packet(0), 0x4000_003d);
        assert_eq!(Eapp::lost_wheel_packet(1), 0x4000_003a);
        assert_eq!(Eapp::lost_wheel_packet(8), 0x4000_0028);
        assert_eq!(Eapp::lost_wheel_packet(16), 0x4000_0012);
        assert_eq!(Eapp::lost_wheel_packet(119), 0x4000_0000);
    }

    #[test]
    fn watch_frame_range_accepts_single_and_inclusive_ranges() {
        assert_eq!(Eapp::parse_watch_frame_range("120-450"), Some((120, 450)));
        assert_eq!(Eapp::parse_watch_frame_range("37"), Some((37, 37)));
        assert_eq!(Eapp::parse_watch_frame_range("450-120"), None);
        assert_eq!(Eapp::parse_watch_frame_range(""), None);
    }

    #[test]
    fn bejeweled_arrow_buttons_map_to_clickwheel_tap_angles() {
        let cases: [(&str, fn(&mut EappInputState), u8); 4] = [
            ("up", |state: &mut EappInputState| state.up = true, 0x30),
            ("left", |state: &mut EappInputState| state.left = true, 0x70),
            ("down", |state: &mut EappInputState| state.down = true, 0xb0),
            (
                "right",
                |state: &mut EappInputState| state.right = true,
                0xf0,
            ),
        ];

        for (name, set, expected) in cases {
            let mut state = EappInputState::default();
            set(&mut state);
            assert_eq!(Eapp::bejeweled_tap_angle(&state), Some(expected), "{name}");
        }
        assert_eq!(Eapp::bejeweled_tap_angle(&EappInputState::default()), None);
    }

    #[test]
    fn input_event_ids_follow_guest_button_mapping() {
        let buttons: [(&str, fn(&mut EappInputState), u8); 5] = [
            (
                "menu",
                |state: &mut EappInputState| state.menu = true,
                1 << 1,
            ),
            (
                "action",
                |state: &mut EappInputState| state.action = true,
                1 << 2,
            ),
            (
                "left",
                |state: &mut EappInputState| state.left = true,
                1 << 3,
            ),
            (
                "right",
                |state: &mut EappInputState| state.right = true,
                1 << 4,
            ),
            (
                "up/down",
                |state: &mut EappInputState| state.up = true,
                1 << 5,
            ),
        ];

        for (name, set, expected) in buttons {
            let mut state = EappInputState::default();
            set(&mut state);
            assert_eq!(
                Eapp::input_event_id_mask_for_state(&state),
                expected,
                "{name}"
            );
        }

        let mut down = EappInputState::default();
        down.down = true;
        assert_eq!(Eapp::input_event_id_mask_for_state(&down), 1 << 5);
    }

    #[test]
    fn popcap_live_gl_default_uses_guest_screen_origin() {
        assert!(!live_gl_default_present_vflip("44444"));
        assert!(!live_gl_default_present_vflip("55555"));
        assert!(!live_gl_default_present_vflip("zuma"));
        assert!(!live_gl_default_present_vflip("bejeweled"));
        assert!(live_gl_default_present_vflip("66666"));
    }

    #[test]
    fn vortex_matrix_helpers_match_centered_rotation_contract() {
        let args = [0, 160.0f32.to_bits(), 120.0f32.to_bits(), 0];
        let translated = matrix_helper_transform(169, GL_IDENTITY_MATRIX, args, 0.0).unwrap();
        assert_eq!(translated[12], 160.0);
        assert_eq!(translated[13], 120.0);

        let rotated = matrix_helper_transform(
            173,
            translated,
            [0, (-707.4536f32).to_bits(), 0, 0],
            1.0,
        )
        .unwrap();
        let centered = matrix_helper_transform(
            169,
            rotated,
            [0, (-160.0f32).to_bits(), (-120.0f32).to_bits(), 0],
            0.0,
        )
        .unwrap();
        assert!((centered[0] - 0.9761).abs() < 0.001);
        assert!((centered[1] - 0.2172).abs() < 0.001);
        assert!((centered[4] + 0.2172).abs() < 0.001);
        assert!((centered[5] - 0.9761).abs() < 0.001);
    }

    #[test]
    fn vortex_ortho_helper_matches_screen_projection_contract() {
        let matrix = orthographic_matrix(
            [0x1806_3e7c, 0.0f32.to_bits(), 320.0f32.to_bits(), 0.0f32.to_bits()],
            240.0,
            -1.0,
            1.0,
        )
        .unwrap();
        assert!((matrix[0] - 0.00625).abs() < 1e-6);
        assert!((matrix[5] - 0.008333334).abs() < 1e-6);
        assert!((matrix[10] + 1.0).abs() < 1e-6);
        assert_eq!(matrix[12], -1.0);
        assert_eq!(matrix[13], -1.0);
        assert_eq!(matrix[15], 1.0);
    }
}
