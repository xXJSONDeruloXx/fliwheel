use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum GlValueClass {
    MappedPointer,
    CodePointer,
    Scalar,
    Float,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum GlMemoryRegion {
    WorkRam,
    Image,
    Trampoline,
    Unmapped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlFileBacking {
    pub path: String,
    pub base_addr: u32,
    pub len: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlMemorySnapshot {
    pub addr: u32,
    pub requested_len: usize,
    pub len: usize,
    pub truncated: bool,
    pub region: GlMemoryRegion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_backing: Option<GlFileBacking>,
    pub bytes_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlRegisterSnapshot {
    pub name: String,
    pub value: u32,
    pub class: GlValueClass,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_value: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<GlMemorySnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlStackWordSnapshot {
    pub offset: usize,
    pub value: u32,
    pub class: GlValueClass,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub float_value: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<GlMemorySnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlImportRecord {
    pub seq: u64,
    pub seq_in_frame: u64,
    pub frame: u64,
    pub ordinal: u32,
    pub pc: u32,
    pub lr: u32,
    pub sp: u32,
    pub return_value: u32,
    pub stack: GlMemorySnapshot,
    pub stack_words: Vec<GlStackWordSnapshot>,
    pub registers: Vec<GlRegisterSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlFrameRecord {
    pub first_frame: u64,
    pub last_frame: u64,
    pub repeat_count: u64,
    pub signature: String,
    pub records: Vec<GlImportRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlTraceFixture {
    pub title: String,
    pub bundle_dir: String,
    pub executable_path: String,
    pub file_vma_base: u32,
    pub work_ram_base: u32,
    pub work_ram_size: usize,
    pub stack_snapshot_len: usize,
    pub pointer_snapshot_len: usize,
    pub capture_start_frame: u64,
    pub capture_end_frame: u64,
    pub frames: Vec<GlFrameRecord>,
}

#[derive(Debug, Clone)]
pub struct GlTraceRecorder {
    capture_start_frame: u64,
    capture_end_frame: u64,
    stack_snapshot_len: usize,
    pointer_snapshot_len: usize,
    next_seq: u64,
    current_frame: Option<u64>,
    current_records: Vec<GlImportRecord>,
    frames: Vec<GlFrameRecord>,
}

impl GlTraceRecorder {
    pub fn new(
        capture_start_frame: u64,
        capture_end_frame: u64,
        stack_snapshot_len: usize,
        pointer_snapshot_len: usize,
    ) -> Self {
        Self {
            capture_start_frame,
            capture_end_frame,
            stack_snapshot_len,
            pointer_snapshot_len,
            next_seq: 1,
            current_frame: None,
            current_records: Vec::new(),
            frames: Vec::new(),
        }
    }

    pub fn capture_range(&self) -> (u64, u64) {
        (self.capture_start_frame, self.capture_end_frame)
    }

    pub fn stack_snapshot_len(&self) -> usize {
        self.stack_snapshot_len
    }

    pub fn pointer_snapshot_len(&self) -> usize {
        self.pointer_snapshot_len
    }

    pub fn capture_record(&mut self, frame: u64, mut record: GlImportRecord) {
        match self.current_frame {
            Some(cur) if cur != frame => {
                self.flush_current_frame();
                self.current_frame = Some(frame);
            }
            None => self.current_frame = Some(frame),
            _ => {}
        }

        record.seq = self.next_seq;
        record.seq_in_frame = self.current_records.len() as u64 + 1;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.current_records.push(record);
    }

    pub fn finalize(mut self) -> GlTraceFixture {
        self.flush_current_frame();
        GlTraceFixture {
            title: String::new(),
            bundle_dir: String::new(),
            executable_path: String::new(),
            file_vma_base: 0,
            work_ram_base: 0,
            work_ram_size: 0,
            stack_snapshot_len: self.stack_snapshot_len,
            pointer_snapshot_len: self.pointer_snapshot_len,
            capture_start_frame: self.capture_start_frame,
            capture_end_frame: self.capture_end_frame,
            frames: self.frames,
        }
    }

    fn flush_current_frame(&mut self) {
        let Some(frame) = self.current_frame else {
            return;
        };
        if self.current_records.is_empty() {
            self.current_frame = None;
            return;
        }

        let signature = frame_signature(&self.current_records);
        if let Some(last) = self.frames.last_mut() {
            if last.signature == signature {
                last.last_frame = frame;
                last.repeat_count = last.repeat_count.saturating_add(1);
                self.current_records.clear();
                self.current_frame = None;
                return;
            }
        }

        let records = std::mem::take(&mut self.current_records);
        self.frames.push(GlFrameRecord {
            first_frame: frame,
            last_frame: frame,
            repeat_count: 1,
            signature,
            records,
        });
        self.current_frame = None;
    }
}

fn frame_signature(records: &[GlImportRecord]) -> String {
    let mut hasher = DefaultHasher::new();
    for record in records {
        record.ordinal.hash(&mut hasher);
        record.pc.hash(&mut hasher);
        record.lr.hash(&mut hasher);
        record.sp.hash(&mut hasher);
        record.return_value.hash(&mut hasher);
        for reg in &record.registers {
            reg.name.hash(&mut hasher);
            reg.value.hash(&mut hasher);
            (reg.class as u8).hash(&mut hasher);
            if let Some(f) = reg.float_value {
                f.to_bits().hash(&mut hasher);
            }
        }
        record.stack.addr.hash(&mut hasher);
        record.stack.requested_len.hash(&mut hasher);
        record.stack.len.hash(&mut hasher);
        record.stack.truncated.hash(&mut hasher);
        (record.stack.region as u8).hash(&mut hasher);
        record.stack.bytes_hex.hash(&mut hasher);
        for word in &record.stack_words {
            word.offset.hash(&mut hasher);
            word.value.hash(&mut hasher);
            (word.class as u8).hash(&mut hasher);
            if let Some(f) = word.float_value {
                f.to_bits().hash(&mut hasher);
            }
        }
    }
    format!("{:016x}", hasher.finish())
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{:02x}", b);
    }
    out
}
