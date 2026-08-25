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
use usse::{UsseProgram, UsseVm};
pub use rasterizer::{
    blend_src_over, decode_texture_pixels, framebuffer_hash, framebuffer_to_ppm, rasterize_quad,
    rasterize_triangle, sample_nearest, Rgba8, Texture, TextureFormat,
};

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
// Use a 64 MiB synthetic app RAM window, matching the high-memory 5G-class
// iPods that many clickwheel games targeted. Smaller scratch windows truncate
// guest heaps/arenas: PopCap titles were observed copying assets past both
// 0x1080_0000 (8 MiB) and 0x1200_0000 (32 MiB).
const WORK_RAM_SIZE: usize = 64 * 1024 * 1024;

/// Base address of stubbed hardware registers (observed in Zuma/Bejeweled at
/// `0x1400000c`). On real iPod hardware this region contains DMA/display FIFO
/// registers; the emulator stubs it with read-zero/write-discard semantics.
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
    0x1800_4fac, // Strings.dta second-stage callback / progress updater
    0x1800_5400, // state-4 wav descriptor callback/scanner
    0x1800_5468, // state-4 scanner tail-calls 0x15c30 for next wav desc
    0x1800_5480, // state-4 complete: set boot state 5 and re-enter 0x03bd0
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
    0x1801_95a8, // options/settings object constructor (compare with name-entry)
    0x1801_c940, // vtable constructor that builds menu/options objects
    // Scene root pointer and UI object activators
    0x1801_c95c, // post-save object construction entry
    0x1801_c008, // UI update / scene refresh dispatcher
];
const STACK_TOP: u32 = WORK_RAM_BASE + WORK_RAM_SIZE as u32 - 0x1000;
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

fn ordinal45_resource_format(format: u32) -> Option<TextureFormat> {
    match format {
        // Observed in Mahjong resource texture objects. These are copied from
        // guest work RAM and decoded as alpha masks with white tint until more
        // exact palette/color state is proven.
        0x8808 | 0x0801 => Some(TextureFormat::A8),
        _ => None,
    }
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
        let enabled = std::env::var_os("CLICKY_STARTUP_PROGRESS_TRACE")
            .map(|v| v.to_string_lossy() == "1")
            .unwrap_or(false);
        let max_logs = std::env::var_os("CLICKY_STARTUP_PROGRESS_FRAMES")
            .and_then(|v| v.to_string_lossy().parse::<usize>().ok())
            .unwrap_or(180);
        let interval = std::env::var_os("CLICKY_STARTUP_PROGRESS_INTERVAL")
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
    manifest_rows: u64,
    dump_count: u64,
    last_hash: Option<u64>,
}

impl StartupArtifactCapture {
    fn from_env() -> StartupArtifactCapture {
        let enabled = std::env::var_os("CLICKY_STARTUP_CAPTURE_DIR").is_some();
        let dir = std::env::var_os("CLICKY_STARTUP_CAPTURE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp/clicky_tetris_startup_capture"));
        let manifest_path = dir.join("manifest.tsv");
        let periodic_interval = std::env::var_os("CLICKY_STARTUP_CAPTURE_PERIOD")
            .and_then(|v| v.to_string_lossy().parse::<u64>().ok())
            .unwrap_or(30)
            .max(1);
        let max_frames = std::env::var_os("CLICKY_STARTUP_CAPTURE_MAX_FRAMES")
            .and_then(|v| v.to_string_lossy().parse::<u64>().ok())
            .unwrap_or(1200);
        let max_dumps = std::env::var_os("CLICKY_STARTUP_CAPTURE_MAX_DUMPS")
            .and_then(|v| v.to_string_lossy().parse::<u64>().ok())
            .unwrap_or(400);
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
    render_state: Arc<Mutex<Vec<u32>>>,
    controls: Option<EappBinds>,
    next_alloc: u32,
    bootstrap_phase: BootstrapPhase,
    app_object: u32,
    frame_context: u32,
    frame_counter: u64,
    /// Total CPU steps executed (for throttling DMA present checks)
    step_counter: u64,
    pending_guest_calls: VecDeque<PendingGuestCall>,
    /// Host file contents staged for delivery to the guest, keyed by the guest
    /// request-object address that asked for them.
    staged_files: HashMap<u32, StagedFile>,
    /// Request objects we've already dumped once, to keep logs tractable.
    dumped_requests: HashSet<u32>,
    /// Per-PC counters for env-gated Tetris localization/string-table tracing.
    string_trace_hits: HashMap<u32, u32>,
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
    async_request_count: u64,
    async_callback_queued_count: u64,
    guest_callback_invocation_count: u64,
    async_pending_requests: HashSet<u32>,
    /// Synthetic handles returned by direct AsyncFileIO open/read wrappers
    /// (ordinals 12/14/16). Keyed by the small guest-visible handle written
    /// into the caller's file object.
    async_open_files: HashMap<u32, PathBuf>,
    next_async_file_handle: u32,
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
    /// `CLICKY_EXPERIMENTAL_GL_HLE=1`; when `None` the legacy fill-color
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
    /// Host path the bytes came from.
    host_path: PathBuf,
}

#[derive(Debug, Copy, Clone)]
struct PendingGuestCall {
    pc: u32,
    arg0: u32,
    arg1: u32,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum BootstrapPhase {
    Entry,
    Running,
    Done,
}

#[derive(Debug)]
struct EappBus {
    image: Ram,
    image_len: u32,
    work_ram: Ram,
    dma_framebuf: Ram,
    /// Track range of DMA FB writes
    hw_fb_write_count: usize,
    hw_fb_write_min: u32,
    hw_fb_write_max: u32,
    /// DMA frame counter: incremented each time pixel 0 is re-written
    hw_dma_frame: usize,
    /// Set to true when DMA FB has been written at least once.
    /// Cleared by the Eapp struct after presenting the DMA content.
    hw_dma_dirty: bool,
    /// Optional write-watchpoint range (start, end exclusive). When set,
    /// every byte write whose address falls in [start, end) is recorded to
    /// `watch_log` tagged with `pending_pc`. Used for RE: attributing
    /// object-field writes to the guest instruction that performed them.
    watch: Option<(u32, u32)>,
    /// PC of the instruction currently executing. Captured by `step()` before
    /// `cpu.step(...)` so the bus (which only sees `&mut EappBus`) can tag
    /// watchpoint hits. Stale by one instruction for multi-access instrs, but
    /// close enough to locate the writer.
    pending_pc: u32,
    /// Accumulated write-watchpoint hits (addr, value, pc). Drained and
    /// dumped on fatal / at end of run. Bounded; a flooded range is a sign
    /// the watch was set too wide.
    watch_log: Vec<WatchHit>,
}

#[derive(Debug, Clone, Copy)]
struct WatchHit {
    addr: u32,
    val: u32,
    pc: u32,
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

        let mut eapp = Eapp {
            cpu,
            bus: EappBus {
                image: image_ram,
                image_len: mapped_image_len as u32,
                work_ram,
                dma_framebuf: Ram::new(320 * 240 * 2), // RGB565 320×240
                hw_fb_write_count: 0,
                hw_fb_write_min: u32::MAX,
                hw_fb_write_max: 0,
                hw_dma_frame: 0,
                hw_dma_dirty: false,
                watch: None,
                pending_pc: 0,
                watch_log: Vec::new(),
            },
            metadata: image.metadata,
            header: image.header,
            imports,
            trampoline_to_import,
            logged_imports: HashSet::new(),
            recent_pcs: VecDeque::with_capacity(RECENT_PC_LIMIT),
            input_state,
            input_event_prev_mask: 0,
            render_state,
            controls: Some(controls),
            next_alloc: WORK_RAM_BASE + 0x1000,
            bootstrap_phase: BootstrapPhase::Entry,
            app_object: 0,
            frame_context: 0,
            frame_counter: 0,
            step_counter: 0,
            pending_guest_calls: VecDeque::new(),
            staged_files: HashMap::new(),
            dumped_requests: HashSet::new(),
            string_trace_hits: HashMap::new(),
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
            async_request_count: 0,
            async_callback_queued_count: 0,
            guest_callback_invocation_count: 0,
            async_pending_requests: HashSet::new(),
            async_open_files: HashMap::new(),
            next_async_file_handle: 1,
            async_dir_entries: Vec::new(),
            gl_trace_frames: None,
            gl_capture: None,
            staged_file_generation: 0,
            halted: false,
            live_gl: Self::maybe_init_live_gl(game_id),
            usse_program: None,
            usse_vm: UsseVm::default(),
        };
        if eapp.metadata.title == "1B200"
            && std::env::var_os("CLICKY_EAPP_LOST_PATCH_RENDER_CALL").is_some()
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
    /// range is set via `CLICKY_EAPP_WATCH=addr,len`. Unlike the fatal-path
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
                "  write addr={:#010x} val={:#010x}{} pc={:#010x}",
                hit.addr, hit.val, ascii, hit.pc,
            );
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
            self.live_log_completed_frame(&completed, true);
            self.live_log_signature_detail(&completed);
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
        if let Ok(rel) = host_path.strip_prefix(&self.metadata.bundle_dir) {
            return rel.display().to_string();
        }
        if let Ok(rel) = host_path.strip_prefix(self.metadata.bundle_dir.join(".clicky-saves")) {
            return format!(".clicky-saves/{}", rel.display());
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
        let labels = ["MENU", "PLAY", "VOLUME", "OPTIONS", "RECORDS", "HELP", "EXIT"];
        let encodings: [(&str, fn(&str) -> Vec<u8>); 3] = [
            ("ascii", |s| s.as_bytes().to_vec()),
            ("utf16le", |s| s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()),
            ("utf16be", |s| s.encode_utf16().flat_map(|c| c.to_be_bytes()).collect()),
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

        let utf16be = |s: &str| -> Vec<u8> {
            s.encode_utf16().flat_map(|c| c.to_be_bytes()).collect()
        };
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
                    refs.push(format!("{:#010x}->{:#010x}", WORK_RAM_BASE + off as u32, val));
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
            let value_refs = pointer_refs(value_addr, value_addr + (value_end - value_start) as u32, 16);
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
            let mut entries: Vec<(u32, u32)> =
                self.string_trace_hits.iter().map(|(k, v)| (*k, *v)).collect();
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
            && !self.live_gl.as_ref().map(|lg| lg.frame_active).unwrap_or(false)
        {
            self.maybe_present_dma_frame();
        }
        let pc = self.cpu.reg_get(self.cpu.mode(), reg::PC);
        self.record_pc(pc);
        // Surface the current PC to the bus so write-watchpoint hits can be
        // tagged with the writer's PC. Set before `cpu.step`, which may emit
        // memory writes for the instruction at `pc`.
        self.bus.pending_pc = pc;
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
            && std::env::var("CLICKY_EAPP_ASYNC3_COMPLETE")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false)
            && self.metadata.bundle_dir.to_str().map_or(false, |p| p.contains("66666"))
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
        if self.metadata.bundle_dir.to_str().map_or(false, |p| p.contains("12345")) {
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
            && self.metadata.bundle_dir.to_str().map_or(false, |p| p.contains("33333"))
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
            if let Ok(raw) = std::env::var("CLICKY_EAPP_HOST_EVENT_FLAGS") {
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
                let delay = std::env::var("CLICKY_EAPP_HOST_EVENT_DELAY")
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
        // comparison. Prefer `CLICKY_EAPP_HOST_EVENT_FLAGS=0x18` now because it
        // uses the guest's real `0x50b0 -> 0x5aa4` event-copy path.
        if std::env::var_os("CLICKY_EAPP_AUDIO_SLOT_BIT").is_some()
            && pc == 0x1801_b630u32
        {
            let bit: u32 = std::env::var("CLICKY_EAPP_AUDIO_SLOT_BIT_VAL")
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

        self.maybe_trace_string_path(pc);

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
            if let Some(obj) = self.read_guest_words(self.cpu.reg_get(mode, 0), 16).get(0..16) {
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
            // is the payoff of the `CLICKY_EAPP_WATCH` RE hook: every write
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
            "Audio" => {
                self.trace_audio_call(import.ordinal, pc, lr, args);
                // Audio:52 (r0=rserver base) and Audio:51 (r0=prev_ret, r3=shared_ptr)
                // are part of Lost's render server init. The game divides:
                //   result = -Audio51_ret / Audio52_ret
                // We need non-zero from both to avoid 0/0 and get a useful result.
                // Audio:51 is called with r0 = Audio:52's return value (1 after our fix)
                // and r3 = shared data pointer (0x1001208C).
                let r3 = args[3];
                if import.ordinal == 52 && args[0] >= 0x10000000 {
                    info!(target: "EAPP_GL", "audio_52_rserver: r0={:#010x} ret=1", args[0]);
                    1u32
                } else if import.ordinal == 51 && r3 >= 0x10000000 {
                    // r3 is the same shared data pointer as Audio:52's r3
                    info!(target: "EAPP_GL", "audio_51_rserver: r0={:#010x} ret=1", args[0]);
                    1u32
                } else {
                    0
                }
            }
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

        // Env-gated (`CLICKY_ALLOC_TRACE=1`) allocator-return log. `miscTBD:0`
        // is the runtime allocator and the only path that returns freshly-
        // zeroed guest memory; logging its (caller_lr, returned_addr, len)
        // lets any faulting work-RAM object be attributed to the guest call
        // site that created it. Useful for null-vtable / null-deref teardown
        // investigations.
        if import.module == "miscTBD" && import.ordinal == 0 && std::env::var_os("CLICKY_ALLOC_TRACE").is_some() {
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

    /// Parse `CLICKY_EAPP_WATCH=0xADDR,0xLEN` into a work-RAM write-
    /// watchpoint range `(start, end)`. Used by the in-bus watch hook to
    /// attribute field writes to the guest instruction that performed them.
    /// Returns `None` when unset, so default behavior is unchanged.
    fn parse_watch_env() -> Option<(u32, u32)> {
        let raw = std::env::var("CLICKY_EAPP_WATCH").ok()?;
        let mut parts = raw.split(',');
        let addr_str = parts.next()?.trim();
        let len_str = parts.next().map(|s| s.trim()).unwrap_or("0x20");
        let parse_num = |s: &str| -> Option<u32> {
            let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
            u32::from_str_radix(s, 16).ok().or_else(|| s.parse::<u32>().ok())
        };
        let addr = parse_num(addr_str)?;
        let len = parse_num(len_str).unwrap_or(0x20);
        Some((addr, addr.wrapping_add(len)))
    }

    /// Read the experimental GL HLE env flags and construct live state only
    /// when `CLICKY_EXPERIMENTAL_GL_HLE=1`. Returns `None` (legacy path) when
    /// the flag is absent or not enabled, so default behavior is unchanged.
    fn maybe_init_live_gl(game_id: String) -> Option<LiveGlState> {
        let enabled = std::env::var_os("CLICKY_EXPERIMENTAL_GL_HLE")
            .map(|v| v.to_string_lossy() == "1")
            .unwrap_or(false);
        if !enabled {
            return None;
        }
        let present_vflip = std::env::var_os("CLICKY_GL_PRESENT_VFLIP")
            .and_then(|v| v.to_string_lossy().parse::<u32>().ok())
            .map(|n| n != 0)
            .unwrap_or(true);
        let gate_b = std::env::var_os("CLICKY_GL_GATE_B")
            .map(|v| v.to_string_lossy() == "1")
            .unwrap_or(false);
        let continuous = std::env::var_os("CLICKY_GL_LIVE_CONTINUOUS")
            .map(|v| v.to_string_lossy() == "1")
            .unwrap_or(false);
        let dump_frames = std::env::var_os("CLICKY_GL_DUMP_FRAMES")
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
            99 => { self.live_handle_upload(args); 0 }
            137 => { self.live_handle_array_def(args); 0 }
            40 => { self.live_handle_enable_array(args); 0 }
            169 => { self.live_handle_translate(args); 0 }
            159 => { self.live_handle_bind_material(args); 0 }
            37 => { self.live_handle_draw(args); 0 }
            38 => { self.live_handle_draw_elements(args); 0 }
            45 => { self.live_handle_resource_upload(args); 0 }
            // Candidate lifecycle from observed live ordering:
            // 158 always precedes all steady-state draws; 157 always follows.
            // Neutral names until exact ABI semantics are proven.
            158 => { self.live_handle_candidate_begin(); 0 }
            157 => { self.live_handle_candidate_present(); 0 }
            165 => { self.live_handle_ordinal_165(args); 0 }
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
                    let non_zero: Vec<(String, u32)> = header_words.iter().enumerate()
                        .filter(|(_, &w)| w != 0)
                        .map(|(i, w)| (format!("+0x{:03x}", i * 4), *w))
                        .collect();
                    if !non_zero.is_empty() {
                        let summary: Vec<String> = non_zero.iter()
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
                    // Behind CLICKY_EAPP_THUMB_STUBS=1 env var.
                    if std::env::var_os("CLICKY_EAPP_THUMB_STUBS").is_some() {
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
                    // Behind CLICKY_EAPP_FILL_RSERVER_HEADER=1 env var.
                    if std::env::var_os("CLICKY_EAPP_FILL_RSERVER_HEADER").is_some() {
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
                info!(target: "EAPP_GL", "ordinal_167: shader_bind program={:#010x}", args[0]);
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
                    let _ = self.write_guest_u32(args[2], 4); // size = 4 bytes
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
                0
            }
            // Ordinal 19: unknown render dispatch. Called from Lost's render
            // function (0x18007264) which is currently unreachable because the
            // rserver dispatch table is empty.
            19 => {
                if let Some(program) = self.usse_program.as_ref() {
                    program.step_placeholder(&mut self.usse_vm, 64);
                    info!(target: "EAPP_GL", "ordinal_19: render_dispatch r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} usse_pc={} executed={} r0_raw={:#010x} halted={}",
                        args[0], args[1], args[2], args[3], self.usse_vm.pc_word, self.usse_vm.executed_words, self.usse_vm.scalar_regs[0], self.usse_vm.halted);
                } else {
                    info!(target: "EAPP_GL", "ordinal_19: render_dispatch r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} usse=<none>", args[0], args[1], args[2], args[3]);
                }
                0
            }
            // Draw-adjacent state ordinals; recorded by observation only.
            175 | 149 | 125 | 36 => 0,
            // Ordinal 148 appears before pointer-backed material draws in the
            // menu phase. Evidence: r0=4, r1=1, r2=0x101029e8 (work RAM ptr).
            // Semantics not yet confirmed — capture args for analysis.
            148 => { self.live_handle_ordinal_148(args); 0 }
            // Ordinal 4: glBindTexture(target, texture).
            // r0=target (e.g. 0xDE1=GL_TEXTURE_2D), r1=texture_name.
            // Capture the texture name so that the next ordinal-99 upload
            // can be associated with it (instead of / in addition to ordinal-45).
            4 => {
                let tex_name = args[1];
                if tex_name != 0 {
                    if let Some(lg) = self.live_gl.as_mut() {
                        // Only set if not already captured by ordinal 45
                        if lg.pending_tex_name.is_none() {
                            lg.pending_tex_name = Some(tex_name);
                        }
                    }
                }
                0
            }
            _ => 0
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
                let shader_state = self.read_guest_u32(state_ptr.wrapping_add(shader_state_off)).unwrap_or(0);
                if shader_state == 0xffffffff {
                    self.write_guest_u32(state_ptr.wrapping_add(shader_state_off), 0);
                    info!(target: "EAPP_GL", "present: patched shader state 0xffffffff -> 0 at {:#010x}+0x60", state_ptr);
                }
            }
        }
        // One-time work RAM scan for Lost: scan the large allocation
        // buffer and rserver region for non-zero data written during init.
        if self.frame_counter == 10 && std::env::var_os("CLICKY_EAPP_LOST_MEMSCAN").is_some() {
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
                    if *name == "rserver_data_0x11000" || *name == "rserver_ptrs_0x10012700" || *name == "rserver_ptrs_0x1001F400" {
                        // Full dump for rserver data
                        let mut all_nz: Vec<String> = vec![];
                        for i in (0..0x8000).step_by(4) {
                            let addr = base.wrapping_add(i);
                            let w = match self.read_guest_u32(addr) {
                                Some(w) if w != 0 => Some(format!("+0x{:04x}={:#010x}", i, w)),
                                _ => None,
                            };
                            if let Some(s) = w { all_nz.push(s); }
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
        if std::env::var_os("CLICKY_EAPP_LOST_PATCH_NEG1").is_some() {
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
        if !has_dma && std::env::var_os("CLICKY_EAPP_LOST_SPLASH").is_some() && self.frame_counter <= 1 {
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
                    has_dma = true;  // Treat as DMA content
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
        let tex_name = self.live_gl.as_mut().and_then(|l| l.pending_tex_name.take());
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
                if array_index == 0 { total_comps.min(4) }
                else if array_index == 1 { 2 }
                else { 4 }
            } else {
                // Fallback: use 4 for pos, 2 for UV
                if array_index == 0 { 4 } else if array_index == 1 { 2 } else { 4 }
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

    /// Ordinal 40: enable/select an array by index (direct arg r0 only).
    fn live_handle_enable_array(&mut self, args: [u32; 4]) {
        let array_index = args[0];
        if let Some(lg) = self.live_gl.as_mut() {
            lg.enabled_arrays.insert(array_index);
        }
        debug!(target: "EAPP_GL", "live_enable_array idx={}", array_index);
    }

    /// Ordinal 169: accumulate translation (r1=tx, r2=ty as floats). Reset to
    /// zero after each confirmed draw (ordinal 37).
    fn live_handle_translate(&mut self, args: [u32; 4]) {
        let tx = f32::from_bits(args[1]);
        let ty = f32::from_bits(args[2]);
        if let Some(lg) = self.live_gl.as_mut() {
            lg.translation.0 += tx;
            lg.translation.1 += ty;
        }
    }

    /// Ordinal 159: record the small selector/handle (r0) and state blob
    /// pointer (r1). The exact handle-creation path remains unsolved.
    fn live_handle_bind_material(&mut self, args: [u32; 4]) {
        let handle = args[0];
        let state_ptr = args[1];
        if let Some(lg) = self.live_gl.as_mut() {
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
        if handle < 0x1000_0000
            && (state_ptr >= 0x1000_0000 && state_ptr < 0x2000_0000)
        {
            if self.dumped_pointer_handles.insert(state_ptr) {
                let words = self.read_guest_words(state_ptr, 32);
                let shader_state = self.read_guest_u32(state_ptr.wrapping_add(0x60)).unwrap_or(0);
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
        
        // Vortex-specific: container at 0x18063ebc needs full structure wiring
        if state_ptr == 0x18063ebc {
            // Check if already initialized (avoid double-alloc)
            let current_array = self.read_guest_u32(state_ptr.wrapping_add(0x5c)).unwrap_or(0);
            if current_array != 0 {
                info!(target: "EAPP_GL", "ordinal_165: Vortex container already initialized");
                return;
            }
            
            info!(target: "EAPP_GL", "ordinal_165: initializing Vortex surface structures");
            
            // Read current container state before modification
            let before_c4 = self.read_guest_u32(state_ptr.wrapping_add(4)).unwrap_or(0xdead);
            let before_c54 = self.read_guest_u32(state_ptr.wrapping_add(0x54)).unwrap_or(0xdead);
            let before_c5c = self.read_guest_u32(state_ptr.wrapping_add(0x5c)).unwrap_or(0xdead);
            info!(target: "EAPP_GL", "ordinal_165: container BEFORE: +4={:#010x} +54={:#010x} +5c={:#010x}", 
                  before_c4, before_c54, before_c5c);
            
            // Allocate work-RAM structures
            let surface_size = 320u32 * 240 * 2; // RGB565 framebuffer
            let surface_buf = self.alloc_zeroed(surface_size);
            let object = self.alloc_zeroed(0x40); // 64-byte object
            let array = self.alloc_zeroed(0x4);   // 4-byte array (single pointer)
            
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
                let addr_in_image = *addr >= FILE_VMA_BASE && *addr - FILE_VMA_BASE < self.bus.image_len;
                let addr_in_work = *addr >= WORK_RAM_BASE && *addr - WORK_RAM_BASE < WORK_RAM_SIZE as u32;
                info!(target: "EAPP_GL", "ordinal_165: write {} at {:#010x}: in_image={} in_work={}", 
                      name, addr, addr_in_image, addr_in_work);
                let result = self.write_guest_u32(*addr, *val);
                info!(target: "EAPP_GL", "ordinal_165: write {} result={}", name, result);
            }
            
            // Verify all writes
            let v_object = self.read_guest_u32(state_ptr.wrapping_add(4)).unwrap_or(0xdead);
            let v_count = self.read_guest_u32(state_ptr.wrapping_add(0x54)).unwrap_or(0xdead);
            let v_array = self.read_guest_u32(state_ptr.wrapping_add(0x5c)).unwrap_or(0xdead);
            let v_buf = self.read_guest_u32(object.wrapping_add(4)).unwrap_or(0xdead);
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
        let array = self.alloc_zeroed(0x4);   // 4-byte array
        let state_block = self.alloc_zeroed(0x200); // mutable GL/state block used by Vortex init helpers
        
        if work_container == 0 || surface_buf == 0 || object == 0 || array == 0 || state_block == 0 {
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
        if addr != 0 { Some(addr) } else { None }
    }

    /// Ordinal 148: observed immediately before pointer-backed material
    /// draws in the menu phase. Evidence from first live observation:
    ///   r0=4, r1=1, r2=0x101029e8 (work RAM ptr), r3=0
    /// Appears between `159(handle=0x8)` and the next `137` array def.
    /// Semantics not yet confirmed; logged for analysis.
    fn live_handle_ordinal_148(&mut self, args: [u32; 4]) {
        let ptr_r2 = args[2];
        if !texgen_verbose_enabled() {
            return;
        }
        info!(
            target: "EAPP_GL",
            "ordinal_148 r0={} r1={} r2={:#010x} r3={}",
            args[0], args[1], ptr_r2, args[3]
        );
        // Dump guest memory at r2 when it is a valid work-RAM pointer.
        if (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&ptr_r2) && ptr_r2 != 0 {
            let words = self.read_guest_words(ptr_r2, 32);
            let hex: Vec<String> = words.iter().map(|w| format!("{:#010x}", w)).collect();
            info!(target: "EAPP_GL", "ordinal_148 r2_dump addr={:#010x} words=[{}]", ptr_r2, hex.join(","));
            // The descriptor has 7 sub-pointers at offsets [13..19] (words).
            // Dump each one to see glyph vertex/UV tables.
            for slot in 13..20usize {
                if slot >= words.len() {
                    break;
                }
                let sub_ptr = words[slot];
                if !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&sub_ptr)
                    || sub_ptr == 0
                {
                    continue;
                }
                // Dump 16 words (enough for 4 vertices of 4 comps).
                let sub = self.read_guest_words(sub_ptr, 16);
                let sub_rendered: Vec<String> = sub
                    .iter()
                    .map(|w| {
                        let f = decode_fixed_16_16(*w);
                        format!("{:#010x}({:.2})", w, f)
                    })
                    .collect();
                info!(
                    target: "EAPP_GL",
                    "ordinal_148 glyph_table slot={} ptr={:#010x} words=[{}]",
                    slot,
                    sub_ptr,
                    sub_rendered.join(",")
                );
            }
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
            translation,
            pos_def,
            pos_enabled,
            enabled_arrays,
            draw_index,
            material_epoch,
            explicit_uv_def,
            explicit_uv_enabled,
        ) =
            {
                let lg = match self.live_gl.as_ref() {
                    Some(lg) => lg,
                    None => return,
                };
                let mut enabled_arrays: Vec<u32> = lg.enabled_arrays.iter().copied().collect();
                enabled_arrays.sort_unstable();
                let (explicit_uv_def, explicit_uv_enabled) =
                    if let Some(def) = lg.arrays.get(&1).cloned() {
                        (Some(def), lg.enabled_arrays.contains(&1))
                    } else if let Some(def) = lg.arrays.get(&2).cloned().filter(|d| {
                        d.valid && d.format == live_gl::GL_FIXED && d.component_count == 2
                    }) {
                        (Some(def), lg.enabled_arrays.contains(&2))
                    } else {
                        (None, false)
                    };
                (
                    lg.current_handle,
                    lg.current_state_ptr,
                    lg.translation,
                    lg.arrays.get(&0).cloned(),
                    lg.enabled_arrays.contains(&0),
                    enabled_arrays,
                    lg.draws.len(),
                    lg.current_material_epoch,
                    explicit_uv_def,
                    explicit_uv_enabled,
                )
            };
        let _ = material_epoch;
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
        let tint = Rgba8::rgba(255, 255, 255, 255);
        let mut record = match self.live_gl.as_mut() {
            Some(lg) => lg.rasterize_triangle_strip_record(
                draw_index,
                handle,
                state_ptr,
                translation,
                &positions,
                explicit.as_deref(),
                tint,
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
        self.live_finalize_draws(vec![record]);
    }

    /// Ordinal 38: observed in the Sims/Sudoku/Solitaire engine family as
    /// `DrawElements(mode=5, count=N, type=GL_UNSIGNED_SHORT, indices=ptr)`.
    /// Decode indexed triangle strips using the currently enabled client
    /// arrays. Malformed pointers/types fail safely and record a skipped draw.
    fn live_handle_draw_elements(&mut self, args: [u32; 4]) {
        let mode = args[0];
        let count = args[1] as usize;
        let index_type = args[2];
        let indices_ptr = args[3];
        if mode != live_gl::DRAW_MODE_TRIANGLE_STRIP
            || index_type != live_gl::GL_UNSIGNED_SHORT
            || count < 3
            || count > 4096
            || indices_ptr == 0
        {
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
            translation,
            pos_def,
            pos_enabled,
            enabled_arrays,
            draw_index,
            explicit_uv_def,
            explicit_uv_enabled,
        ) =
            {
                let lg = match self.live_gl.as_ref() {
                    Some(lg) => lg,
                    None => return,
                };
                let mut enabled_arrays: Vec<u32> = lg.enabled_arrays.iter().copied().collect();
                enabled_arrays.sort_unstable();
                let (explicit_uv_def, explicit_uv_enabled) =
                    if let Some(def) = lg.arrays.get(&1).cloned() {
                        (Some(def), lg.enabled_arrays.contains(&1))
                    } else if let Some(def) = lg.arrays.get(&2).cloned().filter(|d| {
                        d.valid && d.format == live_gl::GL_FIXED && d.component_count == 2
                    }) {
                        (Some(def), lg.enabled_arrays.contains(&2))
                    } else {
                        (None, false)
                    };
                (
                    lg.current_handle,
                    lg.current_state_ptr,
                    lg.translation,
                    lg.arrays.get(&0).cloned(),
                    lg.enabled_arrays.contains(&0),
                    enabled_arrays,
                    lg.draws.len(),
                    explicit_uv_def,
                    explicit_uv_enabled,
                )
            };
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
        let tint = Rgba8::rgba(255, 255, 255, 255);
        let mut record = match self.live_gl.as_mut() {
            Some(lg) => lg.rasterize_triangle_strip_record(
                draw_index,
                handle,
                state_ptr,
                translation,
                &positions,
                explicit.as_deref(),
                tint,
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
            translation,
            pos_def,
            pos_enabled,
            enabled_arrays,
            draw_index,
            material_epoch,
            explicit_uv_def,
            explicit_uv_enabled,
        ) =
            {
                let lg = match self.live_gl.as_ref() {
                    Some(lg) => lg,
                    None => return,
                };
                let mut enabled_arrays: Vec<u32> = lg.enabled_arrays.iter().copied().collect();
                enabled_arrays.sort_unstable();
                let (explicit_uv_def, explicit_uv_enabled) =
                    if let Some(def) = lg.arrays.get(&1).cloned() {
                        (Some(def), lg.enabled_arrays.contains(&1))
                    } else if let Some(def) = lg.arrays.get(&2).cloned().filter(|d| {
                        d.valid && d.format == live_gl::GL_FIXED && d.component_count == 2
                    }) {
                        (Some(def), lg.enabled_arrays.contains(&2))
                    } else {
                        (None, false)
                    };
                (
                    lg.current_handle,
                    lg.current_state_ptr,
                    lg.translation,
                    lg.arrays.get(&0).cloned(),
                    lg.enabled_arrays.contains(&0),
                    enabled_arrays,
                    lg.draws.len(),
                    lg.current_material_epoch,
                    explicit_uv_def,
                    explicit_uv_enabled,
                )
            };
        let pointer_handle =
            (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&handle);
        let effective_translation = if pointer_handle {
            self.live_gl
                .as_ref()
                .and_then(|lg| {
                    (lg.pointer_text_carry_handle == Some(handle)).then_some((
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

        let positions = match self.live_decode_positions_range(
            &pos_def,
            pos_enabled,
            effective_translation,
            first,
            count,
        ) {
            Some(p) => p,
            None => {
                let rec = live_gl::LiveDrawRecord {
                    draw_index,
                    handle,
                    state_ptr,
                    translation: effective_translation,
                    positions: [(0.0, 0.0); 4],
                    uvs: [(0.0, 0.0); 4],
                    has_uv: false,
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

        let generated = if quad_groups == 1 {
            self.live_decode_generated_uvs(state_ptr)
        } else {
            None
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
        let solid_color = if handle == 0x3 {
            self.live_decode_solid_color(&explicit_uv_def, explicit_uv_enabled, material_epoch)
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
            let solid_color = if handle == 0x3 {
                solid_color
            } else if has_uv {
                None
            } else {
                solid_color
            };

            let mut record = match self.live_gl.as_mut() {
                Some(lg) => lg.rasterize_draw(
                    draw_index + quad_idx,
                    handle,
                    state_ptr,
                    effective_translation,
                    positions,
                    uvs,
                    has_uv,
                    solid_color,
                    tint,
                    used_generated_uvs,
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
            if pointer_handle && quad_groups == 1 {
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
        let text_ptr = recorded_ch
            .map(|_| 0u32)
            .or_else(|| {
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
            || def.component_count != 4
        {
            return None;
        }
        let stride = if def.stride == 0 {
            def.component_count as usize * 4
        } else {
            def.stride as usize
        };
        let mut acc = [0.0f32; 4];
        for vertex in 0..4usize {
            let base = def.guest_ptr.wrapping_add((vertex * stride) as u32);
            for (component, slot) in acc.iter_mut().enumerate() {
                let word = self.read_guest_u32(base.wrapping_add((component * 4) as u32))?;
                *slot += decode_fixed_16_16(word).clamp(0.0, 1.0);
            }
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

    /// Reset per-draw translation, increment the draw counter, and capture the
    /// first complete candidate frame (after the known steady-state four
    /// ordinal-37 draws) unless continuous capture is enabled.
    fn live_finalize_draw(&mut self, record: Option<live_gl::LiveDrawRecord>) {
        self.live_finalize_draws(record.into_iter().collect());
    }

    fn live_finalize_draws(&mut self, records: Vec<live_gl::LiveDrawRecord>) {
        let should_capture;
        if let Some(lg) = self.live_gl.as_mut() {
            let increment = records.len().max(1);
            lg.draws.extend(records);
            lg.translation = (0.0, 0.0);
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
    /// presented buffer to the desktop render state when `CLICKY_GL_GATE_B=1`.
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
            let color = draw
                .solid_color
                .map(|c| format!("solid=rgba({},{},{},{})", c.r, c.g, c.b, c.a))
                .unwrap_or_else(|| "solid=<none>".to_string());
            let tint = format!(
                "tint=rgba({},{},{},{}) texgen={}",
                draw.tint.r, draw.tint.g, draw.tint.b, draw.tint.a, draw.used_generated_uvs
            );
            info!(
                target: "EAPP_GL",
                "draw_detail guest_frame={} draw={} handle={:#x} state_ptr={:#010x} enabled={:?} pos_arr={} uv_arr={} translation=({:.2},{:.2}) bounds=({:.1},{:.1})-({:.1},{:.1}) uvs=[{}] inferred_dim={:?} {} {} {} coverage={} status={} state_words=[{}]",
                self.frame_counter,
                draw.draw_index + 1,
                draw.handle,
                draw.state_ptr,
                draw.enabled_arrays,
                pos,
                uv,
                draw.translation.0,
                draw.translation.1,
                draw.bounds.0,
                draw.bounds.1,
                draw.bounds.2,
                draw.bounds.3,
                uvs,
                draw.inferred_dim,
                upload,
                color,
                tint,
                draw.coverage,
                draw.skipped_reason.as_deref().unwrap_or("rasterized"),
                state_words
            );
        }
    }

    /// Optional startup capture (`CLICKY_STARTUP_CAPTURE_DIR=/tmp/...`). Writes
    /// a chronological TSV manifest for completed frames, and dumps PPMs for
    /// every presented framebuffer hash change plus periodic samples.
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
        let reason = if hash_changed {
            "hash_change"
        } else if periodic {
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

    /// Optional continuous frame dumping (`CLICKY_GL_DUMP_FRAMES=N`). Writes
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
                // fail when r1 held a stale large pointer/value.
                let len = args[0].max(0x10);
                self.alloc_zeroed(len)
            }
            9 => {
                // Candidate monotonic tick API. Tetris calls this with r0
                // pointing at app_object+4 and app_object+8, then computes a
                // frame delta from the values stored there. The splash timeout
                // thresholds in the guest are 4_000_000 and 2_000_000, matching
                // microsecond units, so expose host monotonic microseconds.
                self.handle_misc9_time_api(args)
            }
            6 => {
                // Lost (1B200) calls this with r0=2 (command type?),
                // r1=pointer into rserver data region (0x10012038 =
                // rserver+0x11000), r2/r3=string pointers.
                // Could be a render-server communication channel.
                // Return value might affect game's draw decision.
                let ret = std::env::var("CLICKY_MISCTBD6_RET")
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
                let bits = Self::input_state_bits(&state) | self.env_input_script_bits();
                if args[0] != 0 {
                    self.write_guest_u32(args[0], bits);
                }
                if args[1] != 0 {
                    self.write_guest_u32(args[1], bits);
                }
                let event_list = self.build_input_event_list(&state);
                let input_obj = self.cpu.reg_get(self.cpu.mode(), 4);
                let input_ctx = self.cpu.reg_get(self.cpu.mode(), 5);
                if (WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&input_obj) {
                    // Tetris' post-import wrapper passes [input_obj+0x30] as
                    // the event-list head to the event consumer. input_ctx+0x20
                    // is a filter/state mask, not the list pointer. Always
                    // overwrite the head, including zero, so a one-frame press
                    // cannot be re-consumed forever as a stale event node.
                    self.write_guest_u32(input_obj.wrapping_add(0x30), event_list);
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
                bits
            }
            1 => self.alloc_zeroed(0x40),
            _ => 0,
        }
    }

    fn effective_input_state(&self) -> EappInputState {
        let mut state = self.input_state.lock().unwrap().clone();
        self.apply_env_input_script(&mut state);
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

    /// Headless input smoke-test helper. Format:
    /// `CLICKY_EAPP_INPUT_SCRIPT="menu:190-200,menu:230-240,action:260-270"`.
    /// Raw masks can also be injected for ABI discovery, e.g.
    /// `bits=0x40000001:190-195`. This intentionally layers on top of live host
    /// input and is ignored when unset, so normal headed input remains
    /// controlled by minifb callbacks.
    fn apply_env_input_script(&self, state: &mut EappInputState) {
        let Ok(script) = std::env::var("CLICKY_EAPP_INPUT_SCRIPT") else {
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
            match key.trim().to_ascii_lowercase().as_str() {
                "up" => state.up = true,
                "down" => state.down = true,
                "left" => state.left = true,
                "right" => state.right = true,
                "action" | "select" | "enter" => state.action = true,
                "menu" | "m" => state.menu = true,
                _ => {}
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
        // Tetris' input wrapper consumes a linked list of button events. Event
        // byte 0 is button id; byte 1 is 2 for press and 1 for release. Emit
        // only edges, matching the firmware event-list semantics; held-state
        // remains available through the compact bits returned by ordinal 0.
        for id in 1..=5u8 {
            let bit = 1u8 << id;
            if pressed & bit != 0 {
                events.push((id, 2u8));
            }
            if released & bit != 0 {
                events.push((id, 1u8));
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
        let mut mask = 0u8;
        // The id-to-mask table in the guest maps 1..5 to five logical buttons.
        // These bindings are still provisional, but unlike a return-only
        // bitfield they feed the structure the game actually traverses.
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

    fn active_env_input_script_entries(&self) -> Vec<(String, String)> {
        let Ok(script) = std::env::var("CLICKY_EAPP_INPUT_SCRIPT") else {
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
                    "Language" => std::env::var("CLICKY_EAPP_LANGUAGE")
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

    /// Env-gated (`CLICKY_AUDIO_TRACE=1`) diagnostic that dumps, for each
    /// `Audio:*` import call, the register args plus a short byte preview of
    /// any arg that looks like a guest pointer (work-RAM or file VMA). When
    /// the pointed region begins with a RIFF/WAVE header, the parsed format
    /// (channels, sample rate, bits, data offset/length) is logged too.
    ///
    /// This is purely an investigation aid for deriving the Audio runtime
    /// ABI across titles. It does not change guest-visible behavior.
    fn trace_audio_call(
        &mut self,
        ordinal: u32,
        pc: u32,
        lr: u32,
        args: [u32; 4],
    ) {
        if std::env::var_os("CLICKY_AUDIO_TRACE").is_none() {
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
            // RIFF/WAVE sniff: "RIFF" <len:le32> "WAVE" "fmt " ...
            let riff = preview.len() >= 12
                && &preview[0..4] == b"RIFF"
                && &preview[8..12] == b"WAVE";
            if riff {
                let wav = self.describe_wav_at(arg);
                detail.push_str(&format!(
                    "r{}={:#010x}->WAV[{}]",
                    i, arg, wav.unwrap_or_else(|| "unparsed".to_string())
                ));
            } else {
                detail.push_str(&format!("r{}={:#010x}->[{}]", i, arg, hex));
            }
        }
        info!(
            target: "EAPP_AUDIO",
            "Audio:{} pc={:#010x} lr={:#010x} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} {}",
            ordinal, pc, lr, args[0], args[1], args[2], args[3], detail
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
            let chunk_len =
                u32::from_le_bytes([header[pos + 4], header[pos + 5], header[pos + 6], header[pos + 7]]) as usize;
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
            // after AsyncFileIO:2 has advanced the audio-stream entry. It has
            // the same owner-completion shape as ordinal 0: the import receives
            // the transient owner in r0, and firmware later calls 0x1801fbfc,
            // which forwards [owner+0x20/0x24] to the linked request callback.
            let complete = std::env::var("CLICKY_EAPP_ASYNC3_COMPLETE")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false);
            let is_tetris = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("66666"));
            let owner = args[0];
            let req = self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0);
            let req_cb = self.read_guest_u32(req.wrapping_add(0x0c)).unwrap_or(0);
            let req_ctx = self.read_guest_u32(req.wrapping_add(0x10)).unwrap_or(0);
            info!(
                target: "EAPP_IMPORT",
                "AsyncFileIO:1 owner={:#010x} req={:#010x} req_cb={:#010x} req_ctx={:#010x} complete={}",
                owner,
                req,
                req_cb,
                req_ctx,
                (complete && is_tetris) as u8
            );
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
                self.async_callback_queued_count =
                    self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: 0x1801_fbfc,
                    arg0: owner,
                    arg1: 0,
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
            let complete = std::env::var("CLICKY_EAPP_ASYNC3_COMPLETE")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false);
            let is_tetris = self
                .metadata
                .bundle_dir
                .to_str()
                .map_or(false, |p| p.contains("66666"));
            let owner = args[0];
            let callback_pc = self.read_guest_u32(owner.wrapping_add(0x34)).unwrap_or(0);
            let callback_ctx = self.read_guest_u32(owner.wrapping_add(0x38)).unwrap_or(0);
            info!(
                target: "EAPP_IMPORT",
                "AsyncFileIO:2 owner={:#010x} cb_pc={:#010x} cb_ctx={:#010x} complete={}",
                owner,
                callback_pc,
                callback_ctx,
                (complete && is_tetris) as u8
            );
            if complete && is_tetris && owner != 0 && callback_pc != 0 {
                let _ = self.write_guest_bytes(owner.wrapping_add(0x1c), &[0]);
                self.async_callback_queued_count =
                    self.async_callback_queued_count.wrapping_add(1);
                self.pending_guest_calls.push_back(PendingGuestCall {
                    pc: callback_pc,
                    arg0: owner,
                    arg1: callback_ctx,
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
                    self.next_async_file_handle = self.next_async_file_handle.wrapping_add(1).max(1);
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
                // and forwards those values to the request callback. Keep the
                // completion env-gated with the AsyncFileIO:3 byte-count path
                // until the full parsed-resource boot reaches the final menu.
                let owner = args[3];
                let complete = std::env::var("CLICKY_EAPP_ASYNC3_COMPLETE")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
                if let Some(host_path) = self.resolve_or_create_host_path(&path) {
                    let bytes = fs::read(&host_path).unwrap_or_else(|e| {
                        warn!(target: "EAPP_IMPORT", "AsyncFileIO:0 read error for {}: {}", host_path.display(), e);
                        Vec::new()
                    });
                    let n = bytes.len() as u32;
                    info!(
                        target: "EAPP_IMPORT",
                        "AsyncFileIO:0 stream path={} owner={:#010x} bytes={} complete={}",
                        host_path.display(),
                        owner,
                        n,
                        complete as u8
                    );
                    if complete && owner != 0 {
                        self.write_guest_u32(owner.wrapping_add(0x20), 0);
                        self.write_guest_u32(owner.wrapping_add(0x24), n);
                        self.async_callback_queued_count =
                            self.async_callback_queued_count.wrapping_add(1);
                        self.pending_guest_calls.push_back(PendingGuestCall {
                            pc: 0x1801_fbfc,
                            arg0: owner,
                            arg1: 0,
                        });
                        if self.startup_progress.enabled {
                            let req = self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0);
                            info!(
                                target: "EAPP_PROGRESS",
                                "async0_callback_queued frame={} queued={} owner={:#010x} req={:#010x} bytes={} pending_async={}",
                                self.frame_counter,
                                self.async_callback_queued_count,
                                owner,
                                req,
                                n,
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
                if let Some(host_path) = self.resolve_or_create_host_path(&path) {
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
                            let requested_len = if want != 0 { want as usize } else { bytes.len() };
                            let n = bytes.len().min(requested_len);
                            // Request-object reads provide a destination and a capacity; the
                            // firmware copies the bytes actually read and reports the byte count.
                            // Do NOT zero-fill the full requested capacity here. Texas Hold'em
                            // requests large font buffers whose capacity overlaps the following
                            // heap request object; memset-style short-read filling wipes [req+8]
                            // and the callback PC before the completion trampoline runs.
                            let should_deliver = if std::env::var_os("CLICKY_EAPP_SKIP_RSERVER").is_some() {
                                // When set, skip loading rserver.bin so the game keeps its
                                // original code at 0x10001038 (used by Lost to test if the
                                // built-in fixed-function engine renders).
                                !host_path.to_str().map_or(false, |p| p.contains("rserver.bin"))
                            } else {
                                true
                            };
                            let bytes_delivered = should_deliver && dest != 0 && self.write_guest_bytes(dest, &bytes[..n]);
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
                                // CLICKY_EAPP_ASYNC3_COMPLETE=1 to re-enable
                                // the iteration-10 behavior for RE.
                                let complete = std::env::var("CLICKY_EAPP_ASYNC3_COMPLETE")
                                    .map(|v| v == "1" || v == "true")
                                    .unwrap_or(false);
                                if complete {
                                    self.write_guest_u32(req.wrapping_add(0x20), 1);
                                    self.write_guest_u32(req.wrapping_add(0x24), n as u32);
                                }
                                if std::env::var_os("EAPP_ASYNC_OWNER").is_some() {
                                    let owner =
                                        self.read_guest_u32(req.wrapping_add(0x08)).unwrap_or(0);
                                    let (ostate, oresult, ocb, octx) = if owner != 0 {
                                        (
                                            self.read_guest_u32(owner.wrapping_add(0x04)).unwrap_or(0),
                                            self.read_guest_u32(owner.wrapping_add(0x08)).unwrap_or(0),
                                            self.read_guest_u32(owner.wrapping_add(0x0c)).unwrap_or(0),
                                            self.read_guest_u32(owner.wrapping_add(0x10)).unwrap_or(0),
                                        )
                                    } else {
                                        (0, 0, 0, 0)
                                    };
                                    // `0x1801d370` reads the per-load processor
                                    // from [ctx+0x164] and the load-bar/owner
                                    // accounting from [ctx+0x11c/0x120/0x124].
                                    // Dump them so we can follow the chain.
                                    let (c120, c124, c164, c168) = if octx != 0 {
                                        (
                                            self.read_guest_u32(octx.wrapping_add(0x120)).unwrap_or(0),
                                            self.read_guest_u32(octx.wrapping_add(0x124)).unwrap_or(0),
                                            self.read_guest_u32(octx.wrapping_add(0x164)).unwrap_or(0),
                                            self.read_guest_u32(octx.wrapping_add(0x168)).unwrap_or(0),
                                        )
                                    } else {
                                        (0, 0, 0, 0)
                                    };
                                    info!(
                                        target: "EAPP_ASYNC_OWNER",
                                        "owner_dump frame={} req={:#010x} owner={:#010x} path={} bytes={} want={} complete={} [o+4]={:#010x} [o+8]={:#010x} [o+c]={:#010x} [o+10]={:#010x} [ctx+120]={:#010x} [ctx+124]={:#010x} [ctx+164]={:#010x} [ctx+168]={:#010x}",
                                        self.frame_counter,
                                        req,
                                        owner,
                                        host_path.display(),
                                        n,
                                        requested_len,
                                        complete as u8,
                                        ostate,
                                        oresult,
                                        ocb,
                                        octx,
                                        c120,
                                        c124,
                                        c164,
                                        c168
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
                                let is_dir = fs::metadata(&full).map(|m| m.is_dir()).unwrap_or(false);
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
    /// Handle `Filesytem` (sic) import module used by iQuiz/TWA and some other
    /// games. On real iPod hardware this provides filesystem open/read/close.
    /// The emulator stubs it with minimal responses so games can progress past
    /// their init sequences.
    fn handle_filesystem_import(&mut self, ordinal: u32, args: [u32; 4]) -> u32 {
        // Try to read the path from r1 for diagnostic purposes
        let path_str = self.try_read_c_string(args[1], 256);
        info!(target: "EAPP_IMPORT", "Filesytem:{} pc={:#010x} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} path={:?}",
            ordinal, self.bus.pending_pc, args[0], args[1], args[2], args[3], path_str);
        match ordinal {
            // Ordinal 0: filesystem open/init. r1=path, r2=flags.
            // Return 1 (success) and track the file for later reads.
            0 => {
                if let Some(path) = path_str.as_deref() {
                    if let Some(host_path) = self.resolve_or_create_host_path(path) {
                        if host_path.exists() {
                            let handle = self.next_async_file_handle;
                            self.next_async_file_handle = self.next_async_file_handle.wrapping_add(1).max(1);
                            self.async_open_files.insert(handle, host_path.clone());
                            // Write handle to the output pointer if provided
                            if args[2] != 0 {
                                // r2 might be an output pointer for the handle
                                // But on the real iPod this might be flags, not a pointer
                            }
                            info!(target: "EAPP_IMPORT", "Filesytem:0 opened {} -> handle {}", host_path.display(), handle);
                        }
                    }
                }
                1
            }
            // Unknown ordinals: return 0 (no-op).
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
        let host_us = self.host_start.elapsed().as_micros() as u64;
        let guest_tick = host_us as u32;
        let wrote = args[0] != 0 && self.write_guest_u32(args[0], guest_tick);
        let after = self.read_guest_u32(args[0]).unwrap_or(0xffff_ffff);
        let guest_time_advances = self
            .misc9_last_pointed_value
            .map(|prev| prev != after)
            .unwrap_or(false);
        self.misc9_last_pointed_value = Some(after);
        let ret = args[0];
        let log_limit = std::env::var_os("CLICKY_EAPP_TIME_DIAG_LIMIT")
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
        // `CLICKY_EAPP_TEST_READY_DELAY=N` defers the byte write until frame
        // N (so the legal screen can dwell before the legal→menu advance).
        // Default delay is 0 (write on the first emission, matching iter 17).
        let test_ready = std::env::var("CLICKY_EAPP_TEST_READY")
            .ok()
            .as_deref()
            .map(|s| s == "1")
            .unwrap_or(false);
        let test_ready_delay: u64 = std::env::var("CLICKY_EAPP_TEST_READY_DELAY")
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
        // `CLICKY_EAPP_AUDIO_SLOT_BIT_VAL=0xNN` (default 0x10).
        let slot_bit: u32 = std::env::var("CLICKY_EAPP_AUDIO_SLOT_BIT_VAL")
            .ok()
            .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0x10);
        if std::env::var_os("CLICKY_EAPP_AUDIO_SLOT_BIT").is_some() {
            // clock_obj=0x1005f710. slot 0 == clock_obj+0x8c, slot 1 == clock_obj+0x9c.
            // Inject only the low byte (0x10) by writing a u32 (0x10) at the
            // target address; upper 3 bytes are 0 (matches the static value).
            let _ = self.write_guest_u32(0x1005f710 + 0x00 * 16 + 0x8c, slot_bit);
            let _ = self.write_guest_u32(0x1005f710 + 0x01 * 16 + 0x8c, slot_bit);
            // RE (iter 20): `0x1b630` calls `0x5a64` which COPIES the field
            // at `0x18025eb0` to `[slot+0x8c]` (overwriting any value we
            // inject here). So inject the byte at the source: `0x18025eb0`.
            let _ = self.write_guest_u32(0x1802_5eb0, slot_bit);
        }
        let import_summary = self.format_frame_import_counts(8);
        let trace_summary = if string_trace_enabled() && !self.string_trace_hits.is_empty() {
            let mut entries: Vec<(u32, u32)> =
                self.string_trace_hits.iter().map(|(k, v)| (*k, *v)).collect();
            entries.sort_by_key(|&(pc, _)| pc);
            entries.iter()
                .map(|(pc, c)| format!("{:#010x}={}", pc, c))
                .collect::<Vec<_>>().join(",")
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
                self.app_object = self.alloc_zeroed(0x2000);
                self.frame_context = self.alloc_zeroed(0x80);
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
                // Vortex-specific surface preallocation to prevent null pointer crashes
                // Must run before first guest frame to set up container structures
                if self.metadata.bundle_dir.to_str().map_or(false, |p| p.contains("12345")) {
                    info!(target: "EAPP", "VORTEX: detected bundle, running preallocation");
                    self.vortex_preallocate_surfaces();
                    // Verify the write
                    let verify = self.read_guest_u32(WORK_RAM_BASE + 0xff0).unwrap_or(0xdead);
                    info!(target: "EAPP", "VORTEX: verification read of WORK_RAM+0xff0 = {:#010x}", verify);
                }
                self.bootstrap_phase = BootstrapPhase::Running;
                self.queue_next_frame();
                self.fill_framebuffer(HLE_INFO_FRAMEBUFFER);
            }
            BootstrapPhase::Running => {
                self.frame_counter = self.frame_counter.wrapping_add(1);
                if self.frame_counter == 1 || self.frame_counter % 600 == 0 {
                    info!(
                        target: "EAPP",
                        "frame {} returned r0={:#010x}",
                        self.frame_counter,
                        self.cpu.reg_get(self.cpu.mode(), 0)
                    );
                }
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

    fn queue_next_frame(&mut self) {
        self.cpu.reg_set(self.cpu.mode(), 0, self.app_object);
        self.cpu.reg_set(self.cpu.mode(), 1, self.frame_context);
        self.cpu
            .reg_set(self.cpu.mode(), reg::LR, BOOTSTRAP_RETURN_PC);
        self.cpu
            .reg_set(self.cpu.mode(), reg::PC, self.header.aux_addr);
    }

    fn dispatch_pending_guest_call(&mut self) -> bool {
        if let Some(call) = self.pending_guest_calls.pop_front() {
            self.guest_callback_invocation_count =
                self.guest_callback_invocation_count.wrapping_add(1);
            self.async_pending_requests.remove(&call.arg0);
            if self.startup_progress.enabled {
                info!(
                    target: "EAPP_PROGRESS",
                    "callback_dispatch frame={} count={} pc={:#010x} arg0={:#010x} arg1={:#010x} pending_async={}",
                    self.frame_counter,
                    self.guest_callback_invocation_count,
                    call.pc,
                    call.arg0,
                    call.arg1,
                    self.async_pending_requests.len()
                );
            } else {
                debug!(
                    target: "EAPP",
                    "dispatching guest callback pc={:#010x} arg0={:#010x} arg1={:#010x}",
                    call.pc,
                    call.arg0,
                    call.arg1
                );
            }
            self.cpu.reg_set(self.cpu.mode(), 0, call.arg0);
            self.cpu.reg_set(self.cpu.mode(), 1, call.arg1);
            self.cpu
                .reg_set(self.cpu.mode(), reg::LR, GUEST_CALLBACK_RETURN_PC);
            self.cpu.reg_set(self.cpu.mode(), reg::PC, call.pc);
            return true;
        }
        false
    }

    fn handle_guest_callback_return(&mut self) {
        if !self.dispatch_pending_guest_call() {
            self.queue_next_frame();
        }
    }

    fn alloc_zeroed(&mut self, len: u32) -> u32 {
        let len = (len + 0xf) & !0xf;
        let addr = self.next_alloc;
        let end = addr.saturating_add(len);
        if end <= WORK_RAM_BASE + WORK_RAM_SIZE as u32 {
            self.next_alloc = end;
            addr
        } else {
            0
        }
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
            0x1800_3bd0
            | 0x1800_3c08
            | 0x1800_3c68
            | 0x1800_3c74
            | 0x1800_3d40
            | 0x1800_3d60
            | 0x1800_3da8
            | 0x1800_4fac
            | 0x1800_5400
            | 0x1800_5468
            | 0x1800_5480 => {
                // Boot/resource-progress state struct. 0x18003bd0 dispatches
                // on [base+4]; 0x18004fac is the Strings.dta 2nd-stage cb
                // that writes selected byte-counts into this struct and bumps
                // the progress state.
                let base = 0x1802_5674;
                for off in [
                    0x00, 0x04, 0x08, 0x0c, 0x10, 0x14, 0x20, 0x24, 0x28, 0x2c,
                ] {
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
            0x1801_c940 => {
                let obj = regs[0];
                details.push(format!("lr={:#010x} parent={:#010x}", lr, obj));
                for off in [
                    0x00, 0x04, 0x08, 0x0c, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x24,
                    0x28, 0x2c, 0x30, 0x38, 0x40, 0x44, 0x48, 0x4c, 0xbc, 0xc4,
                    0xd4, 0xd8, 0xe4,
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
                    details.push(format!("req+{:#04x}={:#010x}", off, self.read_guest_u32(req.wrapping_add(off)).unwrap_or(0)));
                }
            }
            0x1801_fc94 | 0x1801_e0fc | 0x1801_e45c | 0x1801_e484 | 0x1801_e708 => {
                let obj = regs[0];
                for off in [0x00, 0x04, 0x08, 0x0c, 0x10, 0x14, 0x18, 0x20, 0x24, 0x2c, 0x124, 0x128, 0x12c, 0x130] {
                    details.push(format!("obj+{:#04x}={:#010x}", off, self.read_guest_u32(obj.wrapping_add(off)).unwrap_or(0)));
                }
            }
            0x1801_eed8
            | 0x1801_ef1c
            | 0x1801_f000
            | 0x1801_f068
            | 0x1801_f1b4
            | 0x1801_f250
            | 0x1801_f394
            | 0x1801_f474
            | 0x1801_f4a8
            | 0x1801_f558
            | 0x1801_f5a8
            | 0x1801_f69c
            | 0x1801_f6ec
            | 0x1801_f72c
            | 0x1801_f794
            | 0x1801_f900
            | 0x1801_fa90
            | 0x1801_faa8
            | 0x1801_fb3c => {
                let global = 0x1802_5668;
                for off in [0x00, 0x04, 0x08] {
                    details.push(format!("glob+{:#04x}={:#010x}", off, self.read_guest_u32(global + off).unwrap_or(0)));
                }
                let r4 = self.cpu.reg_get(mode, 4);
                let r5 = self.cpu.reg_get(mode, 5);
                let r6 = self.cpu.reg_get(mode, 6);
                details.push(format!("lr={:#010x} r4={:#010x} r5={:#010x} r6={:#010x}", lr, r4, r5, r6));

                // Most `0x1801fxxx` routines take the menu/resource object in
                // r0 at function entry and keep it in r4 internally. At inner
                // PCs (e.g. 0x1f558) r0 is often a scalar field, so also dump
                // r4 when it looks like a work-RAM object.
                for (label, obj) in [("r0", regs[0]), ("r4", r4)] {
                    if !(WORK_RAM_BASE..WORK_RAM_BASE + WORK_RAM_SIZE as u32).contains(&obj) {
                        continue;
                    }
                    for off in [
                        0x00, 0x08, 0x0c, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x24, 0x28,
                        0x2c, 0x30, 0x38, 0x3c, 0x40, 0x44, 0x48, 0x4c, 0x50, 0x54,
                        0x58, 0x5c, 0x60, 0x64, 0x68, 0x6c, 0x70, 0x74, 0x9c, 0xa0,
                        0xa4,
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
                    0x00, 0x04, 0x08, 0x0c, 0x100, 0x107, 0x108, 0x109, 0x10c,
                    0x110,
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
                let root_ptr = self.read_guest_u32(clock_obj.wrapping_add(0x2c)).unwrap_or(0);
                // The root object has a vtable at offset 0 and various children
                let vtable = if root_ptr != 0 {
                    self.read_guest_u32(root_ptr).unwrap_or(0)
                } else { 0 };
                // Read more fields to understand the object layout
                let obj_04 = if root_ptr != 0 { self.read_guest_u32(root_ptr.wrapping_add(0x04)).unwrap_or(0) } else { 0 };
                let obj_08 = if root_ptr != 0 { self.read_guest_u32(root_ptr.wrapping_add(0x08)).unwrap_or(0) } else { 0 };
                let obj_0c = if root_ptr != 0 { self.read_guest_u32(root_ptr.wrapping_add(0x0c)).unwrap_or(0) } else { 0 };
                let obj_10 = if root_ptr != 0 { self.read_guest_u32(root_ptr.wrapping_add(0x10)).unwrap_or(0) } else { 0 };
                let obj_14 = if root_ptr != 0 { self.read_guest_u32(root_ptr.wrapping_add(0x14)).unwrap_or(0) } else { 0 };
                let obj_18 = if root_ptr != 0 { self.read_guest_u32(root_ptr.wrapping_add(0x18)).unwrap_or(0) } else { 0 };
                let obj_1c = if root_ptr != 0 { self.read_guest_u32(root_ptr.wrapping_add(0x1c)).unwrap_or(0) } else { 0 };
                let obj_20 = if root_ptr != 0 { self.read_guest_u32(root_ptr.wrapping_add(0x20)).unwrap_or(0) } else { 0 };
                let obj_24 = if root_ptr != 0 { self.read_guest_u32(root_ptr.wrapping_add(0x24)).unwrap_or(0) } else { 0 };
                let obj_28 = if root_ptr != 0 { self.read_guest_u32(root_ptr.wrapping_add(0x28)).unwrap_or(0) } else { 0 };
                let obj_2c = if root_ptr != 0 { self.read_guest_u32(root_ptr.wrapping_add(0x2c)).unwrap_or(0) } else { 0 };
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

        let writable = self
            .metadata
            .bundle_dir
            .join(".clicky-saves")
            .join(normalized);
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
    std::env::var_os("CLICKY_GL_TEXGEN_VERBOSE")
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
                self.image.r32(offset - FILE_VMA_BASE)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                let val = self.work_ram.r32(offset - WORK_RAM_BASE);
                // Watch reads from the rserver header/init region (Lost game)
                // Rserver loaded at 0x10001038, header at 0x10001038..0x10001237
                // Data at 0x10012038
                // rserver_watch removed (too noisy) — use ordinal-level tracing instead
                val
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                let rel = offset - HW_STUB_BASE;
                if rel < 0x20000 {
                    // DMA control registers
                    Ok(1)
                } else {
                    // DMA framebuffer: read back stored pixel data
                    let fb_off = (rel - 0x20000) as u32;
                    if (fb_off as usize) + 4 <= DMA_FB_SIZE {
                        let mut buf = [0u8; 4];
                        self.dma_framebuf.bulk_read(fb_off, &mut buf);
                        Ok(u32::from_le_bytes(buf))
                    } else {
                        Ok(0)
                    }
                }
            }
            _ => Err(MemException::Unexpected),
        }
    }

    fn w32(&mut self, offset: u32, val: u32) -> MemResult<()> {
        if let Some((start, end)) = self.watch {
            if ranges_overlap(offset, 4, start, end) {
                self.watch_log.push(WatchHit {
                    addr: offset,
                    val,
                    pc: self.pending_pc,
                });
                // Hard cap to avoid OOM on a flooding range.
                if self.watch_log.len() > 4096 {
                    self.watch_log.truncate(4096);
                }
            }
        }
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => {
                self.image.w32(offset - FILE_VMA_BASE, val)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                self.work_ram.w32(offset - WORK_RAM_BASE, val)
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                let rel = offset - HW_STUB_BASE;
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
            _ => Err(MemException::Unexpected),
        }
    }

    fn r8(&mut self, offset: u32) -> MemResult<u8> {
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => {
                self.image.r8(offset - FILE_VMA_BASE)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                self.work_ram.r8(offset - WORK_RAM_BASE)
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                let rel = offset - HW_STUB_BASE;
                if rel < 0x20000 {
                    Ok(1)
                } else {
                    let fb_off = (rel - 0x20000) as usize;
                    if fb_off < DMA_FB_SIZE {
                        let mut buf = [0u8; 1];
                        self.dma_framebuf.bulk_read(fb_off as u32, &mut buf);
                        Ok(buf[0])
                    } else {
                        Ok(0)
                    }
                }
            }
            _ => Err(MemException::Unexpected),
        }
    }

    fn r16(&mut self, offset: u32) -> MemResult<u16> {
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => {
                self.image.r16(offset - FILE_VMA_BASE)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                self.work_ram.r16(offset - WORK_RAM_BASE)
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                let rel = offset - HW_STUB_BASE;
                if rel < 0x20000 {
                    Ok(1)
                } else {
                    let fb_off = (rel - 0x20000) as usize;
                    if fb_off + 2 <= DMA_FB_SIZE {
                        let mut buf = [0u8; 2];
                        self.dma_framebuf.bulk_read(fb_off as u32, &mut buf);
                        Ok(u16::from_le_bytes(buf))
                    } else {
                        Ok(0)
                    }
                }
            }
            _ => Err(MemException::Unexpected),
        }
    }

    fn w8(&mut self, offset: u32, val: u8) -> MemResult<()> {
        if let Some((start, end)) = self.watch {
            if ranges_overlap(offset, 1, start, end) {
                self.watch_log.push(WatchHit {
                    addr: offset,
                    val: val as u32,
                    pc: self.pending_pc,
                });
                if self.watch_log.len() > 4096 {
                    self.watch_log.truncate(4096);
                }
            }
        }
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => {
                self.image.w8(offset - FILE_VMA_BASE, val)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                self.work_ram.w8(offset - WORK_RAM_BASE, val)
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                let rel = offset - HW_STUB_BASE;
                if rel >= 0x20000 {
                    let fb_off = (rel - 0x20000) as u32;
                    if (fb_off as usize) < DMA_FB_SIZE {
                        self.dma_framebuf.bulk_write(fb_off, &[val]);
                        self.hw_dma_dirty = true;
                    }
                }
                Ok(())
            }
            _ => Err(MemException::Unexpected),
        }
    }

    fn w16(&mut self, offset: u32, val: u16) -> MemResult<()> {
        if let Some((start, end)) = self.watch {
            if ranges_overlap(offset, 2, start, end) {
                self.watch_log.push(WatchHit {
                    addr: offset,
                    val: val as u32,
                    pc: self.pending_pc,
                });
                if self.watch_log.len() > 4096 {
                    self.watch_log.truncate(4096);
                }
            }
        }
        match offset {
            FILE_VMA_BASE..=u32::MAX if offset - FILE_VMA_BASE < self.image_len => {
                self.image.w16(offset - FILE_VMA_BASE, val)
            }
            WORK_RAM_BASE..=u32::MAX if offset - WORK_RAM_BASE < WORK_RAM_SIZE as u32 => {
                self.work_ram.w16(offset - WORK_RAM_BASE, val)
            }
            HW_STUB_BASE..=u32::MAX if offset - HW_STUB_BASE < HW_STUB_SIZE as u32 => {
                let rel = offset - HW_STUB_BASE;
                if rel >= 0x20000 {
                    let fb_off = (rel - 0x20000) as u32;
                    if (fb_off as usize) + 2 <= DMA_FB_SIZE {
                        self.dma_framebuf.bulk_write(fb_off, &val.to_le_bytes());
                        self.hw_dma_dirty = true;
                    }
                }
                Ok(())
            }
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
        .filter(|path| path.extension().map(|ext| ext == "bin").unwrap_or(false))
        .collect::<Vec<_>>();
    bins.sort();
    bins.into_iter()
        .next()
        .ok_or_else(|| EappBuildError::MissingExecutable(bundle_dir.display().to_string()))
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
