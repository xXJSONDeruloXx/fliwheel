//! Minimal PowerVR USSE / Lost `rserver.bin` analysis scaffold.
//!
//! This is intentionally conservative: the PowerVR MBX USSE instruction
//! encoding used by iPod click wheel games is not fully documented here, so
//! this module starts as a deterministic parser/cache rather than pretending to
//! implement unknown arithmetic semantics. It gives the EAPP HLE a concrete
//! shader-program object to hang experiments from:
//!
//! - locate the non-code string section (`RenderServerVersion:...`)
//! - expose a word stream for the apparent USSE region (offset 0x200+)
//! - classify common structural markers and embedded strings
//! - provide a tiny VM state object for future opcode implementations
//!
//! The important architectural shift is that OpenGLES:164 no longer just logs
//! and returns `1`; it can parse and retain the rserver program.

#[derive(Clone, Debug)]
pub struct UsseWord {
    pub offset: usize,
    pub raw: u32,
    pub lo: u16,
    pub hi: u16,
}

#[derive(Clone, Debug)]
pub struct UsseString {
    pub offset: usize,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct RserverBlock {
    pub addr: u32,
    pub offset: usize,
    pub len_words: u32,
    pub words: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct UsseProgram {
    pub base_addr: u32,
    pub byte_len: usize,
    pub code_start: usize,
    pub code_end: usize,
    pub words: Vec<UsseWord>,
    pub strings: Vec<UsseString>,
    pub render_server_version: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct UsseVm {
    pub pc_word: usize,
    pub scalar_regs: [u32; 16],
    pub predicate: bool,
    pub halted: bool,
    pub executed_words: usize,
}

impl RserverBlock {
    pub fn summary(&self) -> String {
        let preview = self
            .words
            .iter()
            .take(8)
            .map(|w| format!("{:#010x}", w))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "addr={:#010x} off=0x{:04x} len_words={} preview=[{}]",
            self.addr, self.offset, self.len_words, preview
        )
    }
}

impl UsseProgram {
    /// Parse a Lost-style `rserver.bin` blob loaded at `base_addr`.
    pub fn parse(base_addr: u32, bytes: &[u8]) -> Self {
        let code_start = if bytes.len() >= 0x200 { 0x200 } else { 0 };
        let strings = extract_ascii_strings(bytes, 8);
        let version = strings
            .iter()
            .find(|s| s.value.starts_with("RenderServerVersion:"))
            .map(|s| s.value.clone());

        // Stop the apparent instruction stream at the render-server version
        // string when present. Everything after that is mostly display-control
        // strings and lookup tables.
        let code_end = version
            .as_ref()
            .and_then(|_| strings.iter().find(|s| s.value.starts_with("RenderServerVersion:")))
            .map(|s| s.offset)
            .unwrap_or(bytes.len())
            .min(bytes.len());

        let mut words = Vec::new();
        let mut off = code_start;
        while off + 4 <= code_end {
            let raw = u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
            words.push(UsseWord {
                offset: off,
                raw,
                lo: raw as u16,
                hi: (raw >> 16) as u16,
            });
            off += 4;
        }

        Self {
            base_addr,
            byte_len: bytes.len(),
            code_start,
            code_end,
            words,
            strings,
            render_server_version: version,
        }
    }

    pub fn summary(&self) -> String {
        let first_words = self
            .words
            .iter()
            .take(6)
            .map(|w| format!("+0x{:04x}={:#010x}", w.offset, w.raw))
            .collect::<Vec<_>>()
            .join(", ");
        let version = self
            .render_server_version
            .as_deref()
            .unwrap_or("<unknown>");
        format!(
            "base={:#010x} bytes={} code=0x{:x}..0x{:x} words={} strings={} version={} first=[{}]",
            self.base_addr,
            self.byte_len,
            self.code_start,
            self.code_end,
            self.words.len(),
            self.strings.len(),
            version,
            first_words
        )
    }

    pub fn scan_runtime_blocks(base_addr: u32, bytes: &[u8]) -> Vec<RserverBlock> {
        let mut out = Vec::new();
        let mut off = 0usize;
        while off + 8 <= bytes.len() {
            let word = u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
            if word == 0xCAFEBABE {
                let len_words = u32::from_le_bytes([
                    bytes[off + 4],
                    bytes[off + 5],
                    bytes[off + 6],
                    bytes[off + 7],
                ]);
                // Treat absurd lengths as a marker-only block and cap previews.
                let max_words = len_words.min(0x100) as usize;
                let mut words = Vec::new();
                for i in 0..max_words {
                    let woff = off + i * 4;
                    if woff + 4 > bytes.len() {
                        break;
                    }
                    words.push(u32::from_le_bytes([
                        bytes[woff],
                        bytes[woff + 1],
                        bytes[woff + 2],
                        bytes[woff + 3],
                    ]));
                }
                out.push(RserverBlock {
                    addr: base_addr.wrapping_add(off as u32),
                    offset: off,
                    len_words,
                    words,
                });
                off += 8;
            } else {
                off += 4;
            }
        }
        out
    }

    /// Execute up to `budget` placeholder words. This currently advances the
    /// VM deterministically and records instruction count. Opcode semantics will
    /// be filled in as USSE encodings are identified.
    pub fn step_placeholder(&self, vm: &mut UsseVm, budget: usize) {
        let mut left = budget;
        while left > 0 && !vm.halted {
            if vm.pc_word >= self.words.len() {
                vm.halted = true;
                break;
            }
            let word = &self.words[vm.pc_word];
            // Known no-op-ish marker in Lost streams: many 0x0000e?10 words
            // appear in setup tables. For now just expose raw words to future
            // opcode implementations and advance.
            vm.scalar_regs[0] = word.raw;
            vm.pc_word += 1;
            vm.executed_words += 1;
            left -= 1;
        }
    }
}

fn extract_ascii_strings(bytes: &[u8], min_len: usize) -> Vec<UsseString> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if is_printable(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_printable(bytes[i]) {
                i += 1;
            }
            if i - start >= min_len {
                let value = String::from_utf8_lossy(&bytes[start..i]).to_string();
                out.push(UsseString { offset: start, value });
            }
        } else {
            i += 1;
        }
    }
    out
}

fn is_printable(b: u8) -> bool {
    matches!(b, 0x20..=0x7e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_and_words() {
        let mut bytes = vec![0u8; 0x220];
        bytes[0x200..0x204].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        bytes.extend_from_slice(b"RenderServerVersion:RELEASE:2704\0");
        let p = UsseProgram::parse(0x1000_1038, &bytes);
        assert_eq!(p.code_start, 0x200);
        assert!(p.words.iter().any(|w| w.raw == 0x1234_5678));
        assert_eq!(p.render_server_version.as_deref(), Some("RenderServerVersion:RELEASE:2704"));
    }

    #[test]
    fn scans_cafebabe_blocks() {
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x14].copy_from_slice(&0xCAFEBABEu32.to_le_bytes());
        bytes[0x14..0x18].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x18..0x1c].copy_from_slice(&0x12345678u32.to_le_bytes());
        let blocks = UsseProgram::scan_runtime_blocks(0x1001_2038, &bytes);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].addr, 0x1001_2048);
        assert_eq!(blocks[0].len_words, 4);
        assert_eq!(blocks[0].words[2], 0x12345678);
    }
}
