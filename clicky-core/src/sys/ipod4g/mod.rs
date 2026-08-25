use std::io::{Read, Seek};

use armv4t_emu::{reg, Cpu};
use thiserror::Error;

use crate::block::BlockDev;
use crate::devices::{Device, Probe};
use crate::error::*;
use crate::executor::*;
use crate::gui::RenderCallback;
use crate::memory::{armv4t_adaptor::MemoryAdapter, MemAccess, MemAccessKind, Memory};
use crate::signal::{self, gpio, irq};

mod controls;
mod gdb;
mod hle_bootloader;

pub use controls::{Ipod4gBinds, Ipod4gKey};
pub use gdb::Ipod4gGdb;

use hle_bootloader::run_hle_bootloader;

use crate::devices::platform::pp::common::*;
use crate::devices::util::{ArcMutexDevice, MemSniffer};
mod devices {
    pub mod i2c {
        pub use crate::devices::i2c::devices::Pcf5060x;
    }

    pub use crate::devices::{
        display::hd66753::Hd66753,
        generic::{ide, AsanRam, Stub},
        platform::pp::*,
    };
}

enum BlockMode {
    Blocking,
    NonBlocking,
}

pub enum BootKind<F: Read + Seek> {
    ColdBoot,
    HLEBoot { fw_file: F },
}

#[derive(Debug)]
struct Ipod4gControls {
    hold: gpio::Sender,
    controls: devices::Controls<signal::Master>,
}

/// A Ipod4g system
#[derive(Debug)]
pub struct Ipod4g {
    frozen: bool,         // set after a fatal error to enable post-mortem debugging
    skip_irq_check: bool, // set by the GDB stub when single-stepping though code

    cpu: Cpu,
    cop: Cpu,
    devices: Ipod4gBus,
    controls: Option<Ipod4gControls>,

    irq_pending: irq::Pending,
    dma_pending: irq::Pending,
    gpio_changed: gpio::Changed,
    i2c_changed: signal::Trigger,

    executor: Executor,

    // Optional hang-watchdog used for bring-up diagnostics. Inert unless the
    // `CLICKY_WATCHDOG_MS` env var is set at construction time.
    watchdog: Option<Watchdog>,
}

/// Bring-up diagnostic watchdog. If the guest spends more than `threshold_ms`
/// of wall-clock time inside `step()` without any progress observable by the
/// caller (e.g. new IRQ, DMA, or MMIO), the watchdog logs the PC and register
/// state of both cores. It only ever logs once per stall, then resets.
#[derive(Debug)]
struct Watchdog {
    threshold: std::time::Duration,
    last_progress: std::time::Instant,
    last_cpu_pc: u32,
    last_cop_pc: u32,
    fired: bool,
    /// Optional periodic PC sampler cadence (env `CLICKY_SAMPLE_MS`).
    sample_every: Option<std::time::Duration>,
    last_sample: std::time::Instant,
    /// Recent (cpu_pc, cop_pc) pairs for tight-loop detection.
    pc_history: std::collections::VecDeque<(u32, u32)>,
    /// Optional CPU PC focus range for targeted hang detection.
    focus_cpu_pc_range: Option<(u32, u32)>,
    /// Optional watched addresses to trace once the CPU enters the focus range.
    trace_addrs: Vec<u32>,
}

const WATCHDOG_HISTORY: usize = 256;

impl Watchdog {
    fn parse_u32(s: &str) -> Option<u32> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            u32::from_str_radix(hex, 16).ok()
        } else {
            s.parse().ok()
        }
    }

    fn parse_u32_range(s: &str) -> Option<(u32, u32)> {
        let s = s.trim();
        let (start, end) = s
            .split_once("..")
            .or_else(|| s.split_once('-'))?;
        Some((Self::parse_u32(start)?, Self::parse_u32(end)?))
    }

    fn focus_contains(&self, pc: u32) -> bool {
        self.focus_cpu_pc_range
            .map(|(start, end)| start <= pc && pc <= end)
            .unwrap_or(true)
    }

    fn from_env() -> Option<Watchdog> {
        let wd_ms: Option<u64> = std::env::var("CLICKY_WATCHDOG_MS")
            .ok()
            .and_then(|s| s.parse().ok());
        let sample_ms: Option<u64> = std::env::var("CLICKY_SAMPLE_MS")
            .ok()
            .and_then(|s| s.parse().ok());
        let focus_cpu_pc_range = std::env::var("CLICKY_WATCH_CPU_PC_RANGE")
            .ok()
            .and_then(|s| Self::parse_u32_range(&s));
        let trace_addrs: Vec<u32> = std::env::var("CLICKY_TRACE_ADDRS")
            .ok()
            .map(|s| {
                s.split(',')
                    .filter_map(Self::parse_u32)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Activate if any diagnostic probe is requested. If only the sampler or
        // focused trace is on, use an effectively-infinite threshold so the
        // generic stall detector never fires.
        let activate = wd_ms.is_some()
            || sample_ms.is_some()
            || focus_cpu_pc_range.is_some()
            || !trace_addrs.is_empty();
        if !activate {
            return None;
        }
        let now = std::time::Instant::now();
        Some(Watchdog {
            threshold: std::time::Duration::from_millis(wd_ms.unwrap_or(u64::MAX / 2)),
            last_progress: now,
            last_cpu_pc: 0,
            last_cop_pc: 0,
            fired: false,
            sample_every: sample_ms.map(std::time::Duration::from_millis),
            last_sample: now,
            pc_history: std::collections::VecDeque::new(),
            focus_cpu_pc_range,
            trace_addrs,
        })
    }
}

#[derive(Error, Debug)]
pub enum Ipod4gBuildError {
    #[error("invalid flash dump: {0}")]
    InvalidDump(&'static str),
    #[error("HLE bootloader failed! {0}")]
    HleBootloader(#[from] hle_bootloader::HleBootloaderError),
}

impl Ipod4g {
    /// Returns a new Ipod4g instance.
    pub fn new<F>(
        hdd: Box<dyn BlockDev>,
        flash_rom: Option<Box<[u8]>>,
        boot_kind: BootKind<F>,
    ) -> Result<Ipod4g, Ipod4gBuildError>
    where
        F: Read + Seek,
    {
        let executor = Executor::new().expect("failed to create task executor");

        // initialize base system
        let irq_pending = irq::Pending::new();
        let dma_pending = irq::Pending::new();
        let gpio_changed = gpio::Changed::new();
        let i2c_changed = signal::Trigger::new(signal::TriggerKind::Edge);

        let mut sys = Ipod4g {
            frozen: false,
            skip_irq_check: false,

            cpu: Cpu::new(),
            cop: Cpu::new(),
            devices: Ipod4gBus::new(executor.spawner(), irq_pending.clone(), dma_pending.clone()),
            controls: None,

            irq_pending,
            dma_pending,
            gpio_changed: gpio_changed.clone(),
            i2c_changed: i2c_changed.clone(),

            executor,

            watchdog: Watchdog::from_env(),
        };

        if let Some(w) = &sys.watchdog {
            eprintln!(
                "[watchdog] active: threshold={:?}, sample_every={:?}, focus_cpu_pc_range={:?}, trace_addrs={:?}",
                w.threshold, w.sample_every, w.focus_cpu_pc_range, w.trace_addrs
            );
        }

        // connect HDD
        sys.devices
            .eidecon
            .as_ide()
            .attach(devices::ide::IdeIdx::IDE0, hdd);

        // Set up flash_rom (if available)
        if let Some(flash_rom) = flash_rom {
            sys.devices
                .flash
                .use_dump(flash_rom)
                .map_err(Ipod4gBuildError::InvalidDump)?
        }

        // hook-up external controls
        let (mut hold_tx, hold_rx) = gpio::new(gpio_changed, "Hold");
        let (controls_tx, controls_rx) = devices::Controls::new_tx_rx(i2c_changed);

        {
            let mut gpio_abcd = sys.devices.gpio_abcd.lock().unwrap();
            gpio_abcd.register_in(5, hold_rx.clone());
        }

        {
            sys.devices.opto.register_controls(controls_rx, hold_rx)
        }

        // HACK: Hold is active-low, so set it to high by default
        hold_tx.set_high();

        sys.controls = Some(Ipod4gControls {
            hold: hold_tx,
            controls: controls_tx,
        });

        // Run the HLE bootloader if an HLE boot was requested
        if let BootKind::HLEBoot { fw_file } = boot_kind {
            run_hle_bootloader(&mut sys, fw_file)?
        }

        Ok(sys)
    }

    /// Hang-watchdog probe. Called every `step()`. When enabled via the
    /// `CLICKY_WATCHDOG_MS` env var, it records the guest PCs and, if the same
    /// PC pair persists past the configured threshold, logs a one-shot
    /// diagnostic with both cores' register state. Inert otherwise.
    fn watchdog_observe(&mut self) {
        let Some(w) = self.watchdog.as_mut() else {
            return;
        };

        let now = std::time::Instant::now();
        let cpu_pc = self.cpu.reg_get(armv4t_emu::Mode::User, armv4t_emu::reg::PC);
        let cop_pc = self.cop.reg_get(armv4t_emu::Mode::User, armv4t_emu::reg::PC);
        let cpu_cpsr = self.cpu.reg_get(armv4t_emu::Mode::User, armv4t_emu::reg::CPSR);
        let cop_cpsr = self.cop.reg_get(armv4t_emu::Mode::User, armv4t_emu::reg::CPSR);
        let cpu_r: [u32; 4] = [
            self.cpu.reg_get(armv4t_emu::Mode::User, 0),
            self.cpu.reg_get(armv4t_emu::Mode::User, 1),
            self.cpu.reg_get(armv4t_emu::Mode::User, 2),
            self.cpu.reg_get(armv4t_emu::Mode::User, 3),
        ];
        let cpu_run = self.devices.cpucon.is_cpu_running(CpuId::Cpu);
        let cop_run = self.devices.cpucon.is_cpu_running(CpuId::Cop);

        // Periodic PC sampler (independent of stall detection).
        if let Some(cadence) = w.sample_every {
            if now.duration_since(w.last_sample) >= cadence {
                w.last_sample = now;
                eprintln!(
                    "[watchdog] SAMPLE cpu_pc={:#010x} (cpsr={:#010x},run={}) r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x} | cop_pc={:#010x} (cpsr={:#010x},run={})",
                    cpu_pc, cpu_cpsr, cpu_run, cpu_r[0], cpu_r[1], cpu_r[2], cpu_r[3],
                    cop_pc, cop_cpsr, cop_run
                );
            }
        }

        // Track PC history to detect tight-loop stalls. Count distinct 64KB
        // pages visited by each core; if both cores stay within a tiny set of
        // pages over the whole window, the guest is busy-waiting.
        w.pc_history.push_back((cpu_pc, cop_pc));
        if w.pc_history.len() > WATCHDOG_HISTORY {
            w.pc_history.pop_front();
        }
        let tight_loop = if w.pc_history.len() == WATCHDOG_HISTORY {
            use std::collections::HashSet;
            let cpu_pages: HashSet<u32> =
                w.pc_history.iter().map(|&(p, _)| p >> 16).collect();
            let cop_pages: HashSet<u32> =
                w.pc_history.iter().map(|&(_, p)| p >> 16).collect();
            // Allow the loop to call into a couple of helper pages (e.g. an
            // IDE handler) while still being recognized as stuck.
            !cpu_pages.is_empty()
                && cpu_pages.len() <= 3
                && cop_pages.len() <= 3
        } else {
            false
        };

        // Optional focus range lets us ignore transient tight loops elsewhere
        // and only fire once the CPU reaches the interesting region.
        let in_focus = w.focus_contains(cpu_pc);

        // `progressing` is false only when the guest is in a tight loop inside
        // the focus range (or, if no focus is configured, anywhere). PC
        // movement inside the loop does NOT count as progress; only escaping to
        // a new region resets the timer.
        let progressing = !tight_loop || !in_focus;
        if progressing {
            w.last_cpu_pc = cpu_pc;
            w.last_cop_pc = cop_pc;
            w.last_progress = now;
            w.fired = false;
            return;
        }

        if !w.fired && now.duration_since(w.last_progress) >= w.threshold {
            w.fired = true;
            let dump_regs = |name: &str, cpu: &Cpu| -> String {
                let mode = armv4t_emu::Mode::User;
                let r: Vec<u32> = (0..16u8)
                    .map(|i| cpu.reg_get(mode, i))
                    .collect();
                let cpsr = cpu.reg_get(mode, armv4t_emu::reg::CPSR);
                format!(
                    "{} pc={:#010x} sp={:#010x} lr={:#010x} cpsr={:#010x} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x}",
                    name, r[15], r[13], r[14], cpsr, r[0], r[1], r[2], r[3]
                )
            };
            eprintln!("[watchdog] HUNG CPU {}", dump_regs("CPU", &self.cpu));
            eprintln!("[watchdog] HUNG COP {}", dump_regs("COP", &self.cop));
            // Dump the live instructions around each core's PC by reading
            // through the system bus (this applies memcon translation, so it
            // reflects what the CPU actually executes, not the raw mapping).
            for (name, pc) in [("CPU", cpu_pc), ("COP", cop_pc)] {
                let mut words = Vec::with_capacity(32);
                for i in 0..32 {
                    let addr = pc.wrapping_sub(32).wrapping_add(i * 4);
                    match self.devices.r32(addr) {
                        Ok(v) => words.push(format!("{:#010x}:{:08x}", addr, v)),
                        Err(_) => words.push(format!("{:#010x}:<err>", addr)),
                    }
                }
                eprintln!("[watchdog] {} code: {}", name, words.join(" "));
            }
            w.last_progress = now;
        }
    }

    /// Run the system for a single CPU instruction, returning `true` if the
    /// system is still running, or `false` upon reaching some sort of "graceful
    /// exit" condition (e.g: power-off).
    fn step(
        &mut self,
        _halt_block_mode: BlockMode,
        mut sniff_memory: (&[u32], impl FnMut(CpuId, MemAccess)),
    ) -> FatalMemResult<bool> {
        if self.frozen {
            return Ok(true);
        }

        // TODO: if neither CPU is running, efficiently block until the next IRQ

        self.watchdog_observe();

        let (diag_focus_range, diag_trace_addrs) = self
            .watchdog
            .as_ref()
            .map(|w| (w.focus_cpu_pc_range, w.trace_addrs.clone()))
            .unwrap_or((None, Vec::new()));

        let devices = &mut self.devices;
        for (cpu, cpuid) in [(&mut self.cpu, CpuId::Cpu), (&mut self.cop, CpuId::Cop)].iter_mut() {
            if !devices.cpucon.is_cpu_running(*cpuid) {
                continue;
            }

            // XXX: armv4t_emu doesn't currently expose any way to differentiate between
            // instruction-fetch reads, and regular reads. Therefore, it's impossible to
            // enforce MMU "execute" protection bits...

            // FIXME: this approach is kinda gross. Maybe add a some "ctx" to `Memory`?
            devices.cpuid.set_cpuid(*cpuid);
            devices.memcon.set_cpuid(*cpuid);
            devices.mailbox.set_cpuid(*cpuid);

            let cpuid = *cpuid;
            let step_pc = cpu.reg_get(cpu.mode(), reg::PC);
            let trace_enabled = !diag_trace_addrs.is_empty()
                && diag_focus_range
                    .map(|(start, end)| start <= step_pc && step_pc <= end)
                    .unwrap_or(true);
            let mut sniff_addrs = sniff_memory.0.to_vec();
            if trace_enabled {
                for &addr in &diag_trace_addrs {
                    if !sniff_addrs.contains(&addr) {
                        sniff_addrs.push(addr);
                    }
                }
            }

            let mut sniffer = MemSniffer::new(devices, &sniff_addrs, |access| {
                if sniff_memory.0.contains(&access.offset) {
                    sniff_memory.1(cpuid, access);
                }
                if trace_enabled && diag_trace_addrs.contains(&access.offset) {
                    eprintln!("[watchdog] TRACE {} pc={:#010x} {}", cpuid, step_pc, access);
                }
            });
            let mut mem = MemoryAdapter::new(&mut sniffer);
            cpu.step(&mut mem);
            if let Some((access, e)) = mem.exception.take() {
                e.resolve(
                    "MMIO",
                    MemExceptionCtx {
                        pc: cpu.reg_get(cpu.mode(), reg::PC),
                        access,
                        in_device: format!("{}, {}", cpuid, devices.probe(access.offset)),
                    },
                )?;
            }
        }

        if self.skip_irq_check {
            return Ok(true);
        }

        // TODO: don't run this on every cycle?
        self.executor.run_until_stalled();

        // XXX: this is terrible. truly god awful. it _really_ needs to be rewritten,
        // reorganized, and moved somewhere more appropriate.
        if self.dma_pending.check() {
            self.dma_pending.clear();
            if devices.dmacon.do_ide_dma() {
                let (kind, addr) = match (devices.eidecon).do_dma() {
                    Ok(tup) => tup,
                    Err(_) => panic!("asd"),
                };

                use crate::memory::MemAccessKind;
                match kind {
                    MemAccessKind::Read => {
                        let val = (devices.eidecon.as_ide())
                            .read16(devices::ide::IdeReg::Data)
                            .unwrap();
                        devices.w16(addr, val).unwrap();
                    }
                    MemAccessKind::Write => {
                        let val = devices.r16(addr).unwrap();
                        (devices.eidecon.as_ide())
                            .write16(devices::ide::IdeReg::Data, val)
                            .unwrap();
                    }
                    MemAccessKind::Execute => {
                        panic!("Unsupported execute DMA");
                    }
                }
            }
        }

        // TODO?: explore adding callbacks to the signaling system
        if self.gpio_changed.check_and_clear() {
            devices.gpio_abcd.lock().unwrap().update();
            devices.gpio_efgh.lock().unwrap().update();
            devices.gpio_ijkl.lock().unwrap().update();
        }
        if self.i2c_changed.check_and_clear() {
            devices.opto.on_change();
        }

        if self.irq_pending.check() {
            self.irq_pending.clear();
        }

        devices.eidecon.update_irq_latch();

        use armv4t_emu::Exception;

        let (cpu_status, cop_status) = devices.intcon.interrupt_status();

        for (core, cpuid, status) in [
            (&mut self.cpu, CpuId::Cpu, cpu_status),
            (&mut self.cop, CpuId::Cop, cop_status),
        ]
        .iter_mut()
        {
            if status.irq && core.irq_enable() {
                devices.cpucon.wake_on_interrupt(*cpuid);
                core.exception(Exception::Interrupt);
            }
            if status.fiq && core.fiq_enable() {
                devices.cpucon.wake_on_interrupt(*cpuid);
                core.exception(Exception::FastInterrupt);
            }
        }

        Ok(true)
    }

    /// Run the system, returning successfully on "graceful exit"
    /// (e.g: power-off).
    pub fn run(&mut self) -> FatalMemResult<()> {
        let dummy_sniff_memory = |_, _| {};
        while self.step(BlockMode::Blocking, (&[], dummy_sniff_memory))? {}
        Ok(())
    }

    /// Run the system, returning successfully on "graceful exit" (e.g:
    /// power-off). This method will return after the specified number of cycles
    /// have been executed.
    pub fn run_cycles(&mut self, cycles: usize) -> FatalMemResult<()> {
        let dummy_sniff_memory = |_, _| {};
        for _ in 0..cycles {
            self.step(BlockMode::Blocking, (&[], dummy_sniff_memory))?;
        }
        Ok(())
    }

    /// Freeze the system such that `step` becomes a noop. Called prior to
    /// spawning a "post-mortem" GDB session.
    ///
    /// WARNING - THERE IS NO WAY TO "THAW" A FROZEN SYSTEM!
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// Return the system's RenderCallback method.
    pub fn render_callback(&self) -> RenderCallback {
        self.devices.hd66753.render_callback()
    }
}

/// The main Ipod4g memory bus.
///
/// This struct is the "top-level" implementation of the [Memory] trait for the
/// Ipod4g, and maps the entire 32 bit address space to the Ipod4g's various
/// devices.
#[derive(Debug)]
pub struct Ipod4gBus {
    pub sdram: devices::AsanRam,
    pub fastram: devices::AsanRam,
    pub cpuid: devices::CpuIdReg,
    pub flash: devices::Flash,
    pub cpucon: devices::CpuCon,
    pub hd66753: devices::Hd66753,
    pub timer1: devices::CfgTimer,
    pub timer2: devices::CfgTimer,
    pub usec_timer: devices::UsecTimer,
    pub gpio_abcd: ArcMutexDevice<devices::GpioBlock>,
    pub gpio_efgh: ArcMutexDevice<devices::GpioBlock>,
    pub gpio_ijkl: ArcMutexDevice<devices::GpioBlock>,
    pub gpio_mirror_abcd: devices::GpioBlockAtomicMirror,
    pub gpio_mirror_efgh: devices::GpioBlockAtomicMirror,
    pub gpio_mirror_ijkl: devices::GpioBlockAtomicMirror,
    pub i2ccon: devices::I2CCon,
    pub opto: devices::OptoWheel,
    pub ppcon: devices::PPCon,
    pub devcon: devices::DevCon,
    pub intcon: devices::IntCon,
    pub eidecon: devices::EIDECon,
    pub memcon: devices::MemCon,
    pub cachecon: devices::CacheCon,
    pub i2s: devices::I2SCon,
    pub mailbox: devices::Mailbox,
    pub dmacon: devices::DmaCon,
    pub serial0: devices::Serial,
    pub serial1: devices::Serial,
    pub evp: devices::Evp,

    pub mystery_irq_con: devices::Stub,
    pub mystery_lcd_con: devices::Stub,
    pub mystery_flash_stub: devices::Stub,
    pub firewire: devices::Stub,
    pub total_mystery: devices::Stub,
    pub pwmcon: devices::PWMCon,

    pub pp5002_serial_stub: devices::Stub,
}

impl Ipod4gBus {
    #[allow(clippy::redundant_clone)] // Makes the code cleaner in this case
    fn new(
        task_spawner: Spawner,
        irq_pending: irq::Pending,
        dma_pending: irq::Pending,
    ) -> Ipod4gBus {
        let (ide_irq_tx, ide_irq_rx) = irq::new(irq_pending.clone(), "IDE");
        let (timer1_irq_tx, timer1_irq_rx) = irq::new(irq_pending.clone(), "Timer1");
        let (timer2_irq_tx, timer2_irq_rx) = irq::new(irq_pending.clone(), "Timer2");
        let (gpio0_irq_tx, gpio0_irq_rx) = irq::new(irq_pending.clone(), "GPIO0");
        let (gpio1_irq_tx, gpio1_irq_rx) = irq::new(irq_pending.clone(), "GPIO1");
        let (gpio2_irq_tx, gpio2_irq_rx) = irq::new(irq_pending.clone(), "GPIO2");
        let (i2c_irq_tx, i2c_irq_rx) = irq::new(irq_pending.clone(), "I2C");

        let (ide_dmarq_tx, ide_dmarq_rx) = irq::new(dma_pending.clone(), "IDE DMA");

        // mailbox is the only core-specific IRQ in the system, which is kinda neat
        let (mbx_cpu_irq_tx, mbx_cpu_irq_rx) = irq::new(irq_pending.clone(), "Mailbox (CPU)");
        let (mbx_cop_irq_tx, mbx_cop_irq_rx) = irq::new(irq_pending.clone(), "Mailbox (COP)");

        let gpio_abcd = ArcMutexDevice::new(GpioBlock::new(gpio0_irq_tx, ["A", "B", "C", "D"]));
        let gpio_efgh = ArcMutexDevice::new(GpioBlock::new(gpio1_irq_tx, ["E", "F", "G", "H"]));
        let gpio_ijkl = ArcMutexDevice::new(GpioBlock::new(gpio2_irq_tx, ["I", "J", "K", "L"]));

        let gpio_mirror_abcd = gpio_abcd.clone();
        let gpio_mirror_efgh = gpio_efgh.clone();
        let gpio_mirror_ijkl = gpio_ijkl.clone();

        let mut intcon = IntCon::new();
        intcon
            .register(0, timer1_irq_rx)
            .register(1, timer2_irq_rx)
            .register_core_specific(4, mbx_cpu_irq_rx, mbx_cop_irq_rx)
            // .register(10, i2s_irq_rx)
            // .register(20, usb_irq_rx)
            .register(23, ide_irq_rx)
            // .register(25, firewire_irq_rx)
            // .register(26, dma_irq_rx)
            .register(32, gpio0_irq_rx)
            .register(33, gpio1_irq_rx)
            .register(34, gpio2_irq_rx)
            // .register(36, ser0_irq_rx)
            // .register(37, ser1_irq_rx)
            .register(40, i2c_irq_rx);

        let dmacon = DmaCon::new(ide_dmarq_rx);

        let mut i2ccon = I2CCon::new(i2c_irq_tx.clone());
        i2ccon.register_device(0x08, Box::new(i2c::Pcf5060x::new()));

        use devices::*;
        Ipod4gBus {
            sdram: AsanRam::new(32 * 1024 * 1024, true), // 32 MB
            fastram: AsanRam::new(96 * 1024, true),      // 96 KB
            cpuid: CpuIdReg::new(),
            flash: Flash::new(),
            cpucon: CpuCon::new(task_spawner.clone()),
            hd66753: Hd66753::new(),
            timer1: CfgTimer::new("1", timer1_irq_tx, task_spawner.clone()),
            timer2: CfgTimer::new("2", timer2_irq_tx, task_spawner),
            usec_timer: UsecTimer::new(),
            gpio_abcd,
            gpio_efgh,
            gpio_ijkl,
            gpio_mirror_abcd: GpioBlockAtomicMirror::new(gpio_mirror_abcd),
            gpio_mirror_efgh: GpioBlockAtomicMirror::new(gpio_mirror_efgh),
            gpio_mirror_ijkl: GpioBlockAtomicMirror::new(gpio_mirror_ijkl),
            i2ccon,
            opto: OptoWheel::new(i2c_irq_tx),
            ppcon: PPCon::new(),
            devcon: DevCon::new(),
            intcon,
            eidecon: EIDECon::new(ide_irq_tx, ide_dmarq_tx),
            memcon: MemCon::new(),
            cachecon: CacheCon::new(),
            i2s: I2SCon::new(),
            mailbox: Mailbox::new(mbx_cpu_irq_tx, mbx_cop_irq_tx),
            dmacon,
            serial0: Serial::new("0"),
            serial1: Serial::new("1"),
            evp: Evp::new(),

            mystery_irq_con: Stub::new("Mystery IRQ Con?"),
            mystery_lcd_con: Stub::new("Mystery LCD Con?"),
            mystery_flash_stub: Stub::new("Mystery FlashROM Con?"),
            firewire: Stub::new("Firewire Con?"),
            total_mystery: Stub::new("<total mystery>"),
            pwmcon: PWMCon::new(),

            pp5002_serial_stub: Stub::new("PP5002 serial stub"),
        }
    }
}

macro_rules! mmap {
    (
        RAM {
            $($start_ram:literal $(..= $end_ram:literal)? => $ram:ident,)*
        }
        DEVICES {
            $($start_dev:literal $(..= $end_dev:literal)? => $dev:ident,)*
        }
    ) => {
        macro_rules! impl_mem_r {
            ($fn:ident, $ret:ty) => {
                fn $fn(&mut self, addr: u32) -> MemResult<$ret> {
                    let mut addr = addr;
                    if (0x00..0x1F).contains(&addr) && self.cachecon.local_evt {
                        addr = addr | 0x6000_f100;
                    }

                    let (phys_addr, prot) = self.memcon.virt_to_phys(addr, MemAccessKind::Read);
                    if !prot.r {
                        return Err(MemException::MmuViolation)
                    }

                    match phys_addr {
                        $($start_ram$(..=$end_ram)? => self.$ram.$fn(phys_addr - $start_ram),)*
                        $($start_dev$(..=$end_dev)? => self.$dev.$fn(phys_addr - $start_dev),)*
                        _ => Err(MemException::Unexpected),
                    }
                }
            };
        }

        macro_rules! impl_mem_w {
            ($fn:ident, $val:ty) => {
                fn $fn(&mut self, addr: u32, val: $val) -> MemResult<()> {
                    let (phys_addr, prot) = self.memcon.virt_to_phys(addr, MemAccessKind::Write);
                    if !prot.w {
                        return Err(MemException::MmuViolation)
                    }

                    match phys_addr {
                        $($start_ram$(..=$end_ram)? => self.$ram.$fn(phys_addr - $start_ram, val),)*
                        $($start_dev$(..=$end_dev)? => self.$dev.$fn(phys_addr - $start_dev, val),)*
                        _ => Err(MemException::Unexpected),
                    }
                }
            };
        }

        macro_rules! impl_mem_x {
            ($fn:ident, $ret:ty) => {
                fn $fn(&mut self, addr: u32) -> MemResult<$ret> {
                    let phys_addr = if (0x00..0x1F).contains(&addr) && self.cachecon.local_evt {
                        match self.evp.r32(addr) {
                            Ok(val) => val,
                            Err(e) => {
                                return Err(e);
                            }
                        }
                    } else {
                        let (final_addr, prot) = self.memcon.virt_to_phys(addr, MemAccessKind::Execute);
                        if !prot.x {
                            return Err(MemException::MmuViolation)
                        }
                        final_addr
                    };

                    match phys_addr {
                        $($start_ram$(..=$end_ram)? => self.$ram.$fn(phys_addr - $start_ram),)*
                        $($start_dev$(..=$end_dev)? => self.$dev.$fn(phys_addr - $start_dev),)*
                        _ => Err(MemException::Unexpected),
                    }
                }
            };
        }

        impl Device for Ipod4gBus {
            fn kind(&self) -> &'static str {
                "Ipod4g"
            }

            fn probe(&self, addr: u32) -> Probe {
                let (addr, _) = self.memcon.virt_to_phys(addr, MemAccessKind::Read);
                match addr {
                    $($start_ram$(..=$end_ram)? => {
                        Probe::from_device(&self.$ram, addr - $start_ram)
                    })*
                    $($start_dev$(..=$end_dev)? => {
                        Probe::from_device(&self.$dev, addr - $start_dev)
                    })*
                    _ => Probe::Unmapped,
                }
            }
        }

        impl Memory for Ipod4gBus {
            impl_mem_r!(r8, u8);
            impl_mem_r!(r16, u16);
            impl_mem_r!(r32, u32);
            impl_mem_w!(w8, u8);
            impl_mem_w!(w16, u16);
            impl_mem_w!(w32, u32);
            impl_mem_x!(x16, u16);
            impl_mem_x!(x32, u32);
        }
    };
}

mmap! {
    RAM {
        0x1000_0000..=0x11ff_ffff => sdram,
        0x4000_0000..=0x4001_7fff => fastram,
    }

    DEVICES {
        0x0000_0000..=0x000f_ffff => flash,
        0x6000_0000..=0x6000_0fff => cpuid,
        0x6000_1000..=0x6000_102f => mailbox,
        0x6000_4000..=0x6000_41ff => intcon,
        0x6000_5000..=0x6000_5007 => timer1,
        0x6000_5008..=0x6000_500f => timer2,
        0x6000_5010..=0x6000_5013 => usec_timer,
        0x6000_6000..=0x6000_6fff => devcon,
        0x6000_7000..=0x6000_7fff => cpucon,
        0x6000_a000..=0x6000_bfff => dmacon,
        0x6000_c000..=0x6000_cfff => cachecon,
        0x6000_d000..=0x6000_d07f => gpio_abcd,
        0x6000_d080..=0x6000_d0ff => gpio_efgh,
        0x6000_d100..=0x6000_d17f => gpio_ijkl,
        0x6000_d800..=0x6000_d87f => gpio_mirror_abcd,
        0x6000_d880..=0x6000_d8ff => gpio_mirror_efgh,
        0x6000_d900..=0x6000_d97f => gpio_mirror_ijkl,

        0x6400_4000..=0x6400_41ff => intcon, // i guess there's a mirror?

        0x7000_0000..=0x7000_1fff => ppcon,
        0x7000_3000..=0x7000_301f => hd66753,
        0x7000_6000..=0x7000_6020 => serial0,
        0x7000_6040..=0x7000_6060 => serial1,
        0x7000_a000..=0x7000_a03f => pwmcon,
        0x7000_c000..=0x7000_c0ff => i2ccon,
        0x7000_c100..=0x7000_c1ff => opto,
        0x7000_2800..=0x7000_28ff => i2s,
        0xc300_0000..=0xc300_0fff => eidecon,
        0xf000_0000..=0xf000_ffff => memcon,

        0x6000_f000..=0x6000_f01f => evp, // Tegra drivers mention 0x6000F1xx but 0x6000F0xx is mentioned in PP5020 RE litterature
        0x6000_f100..=0x6000_f11f => evp, // I assume 0x6000F0xx and 0x6000F1xx are mirrored? Maybe one is used for the main CPU,
                                          // the other is used for COP?

        // all the stubs

        0x6000_1038 => mystery_irq_con,
        0x6000_111c => mystery_irq_con,
        0x6000_1128 => mystery_irq_con,
        0x6000_1138 => mystery_irq_con,
        0x6000_3000..=0x6000_30ff => total_mystery,
        0x6000_9000..=0x6000_90ff => total_mystery,
        // Diagnostics program reads from address, and write back 0x10000000
        0x7000_3800 => total_mystery,
        0xc031_b1d8 => mystery_flash_stub,
        0xc031_b1e8 => mystery_flash_stub,
        // Diagnostics program writes 0xffffffff
        0xc600_008c => firewire,
        0xffff_fe00..=0xffff_ffff => mystery_flash_stub,

        // PP5002 addresses, I know, but iPodLinux uses that
        0xc000_6000..=0xc000_6020 => pp5002_serial_stub,
        0xc000_6040..=0xc000_6060 => pp5002_serial_stub,
    }
}
