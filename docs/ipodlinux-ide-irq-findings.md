# iPodLinux IDE IRQ findings

## Symptom

ZeroSlackr/iPodLinux reached IDE probing and filesystem reads, but the guest repeatedly printed:

```text
hda: lost interrupt
```

That indicated the guest was issuing ATA reads but not reliably observing the completion interrupt.

## Root cause

The PP EIDE interrupt latch and the ATA device INTRQ line were modeled as the same signal.

Real hardware has two related but distinct concepts:

- ATA INTRQ: asserted by the drive and cleared when software reads task-file `Status`.
- PP EIDE IRQ latch/status: surfaced through the PP EIDE config register and cleared by writing the latch/ack bits there.

Conflating them created two failure modes:

- clearing the PP config register could accidentally clear the drive interrupt before Linux consumed task-file `Status`, causing lost IDE interrupts;
- refusing to clear from the PP config register could leave the platform IRQ stuck and cause interrupt storms.

## Fix

`EIDECon` now owns a platform IRQ latch separate from the ATA controller IRQ:

- the generic IDE controller asserts a private ATA IRQ line;
- `EIDECon::update_irq_latch()` edge-samples that private ATA line;
- the PP interrupt controller sees only the EIDE latch;
- PP EIDE config writes clear only the EIDE latch;
- task-file `Status` reads still clear ATA INTRQ inside the generic IDE controller.

This matches the expected split between platform interrupt acknowledgement and ATA interrupt acknowledgement.

## Test helper

`clicky-desktop` also has `--autorun-zeroslackr`, which synthesizes the iPodLoader menu sequence for ZeroSlackr:

```text
Down, Down, Action
```

The synthetic button presses are held across UI frames so the emulated CPU can sample them reliably. Note this is wall-clock based and races the full-speed emulator thread; for ZeroSlackr the preferred approach is now `default=4` auto-boot in `loader.cfg` (see `docs/IPOD_LINUX_BRINGUP.md`).

## Hang watchdog / PC sampler (bring-up diagnostics)

Two env-gated probes were added to `Ipod4g::step` to diagnose boot hangs
without a GDB session. Both are completely inert unless their env var is set:

- `CLICKY_WATCHDOG_MS=<ms>` - stall detector. It tracks recent (cpu_pc, cop_pc)
  pairs; if both cores stay within a small set of 64KB pages for `<ms>` host
  wall-clock time, it prints a one-shot dump of both cores' registers and the
  live instructions around each PC (read through the system bus, so memcon
  translation is applied and the dump reflects what the CPU actually executes).
- `CLICKY_SAMPLE_MS=<ms>` - independent periodic sampler that logs both cores'
  PC, CPSR, running-state, and r0-r3. Useful for watching boot trajectory and
  marching pointers.

A `ctl_raw(cpu)` getter was added to `CpuCon` to read the raw CPU/COP control
word for diagnostics.

## COP park model (confirmed)

The HLE bootloader runs the COP from the kernel entry until it parks itself via
`COP_CTL = PROC_SLEEP (0x80000000)`. For the ZeroSlackr prebuilt kernel the COP
parks inside the kernel's own trampoline (a `ldr pc,[pc,#...]` wake stub, one
instruction past the `COP_CTL` sleep write), which is exactly where real
hardware resumes it. The COP is not woken through the mailbox `COP_QUEUE`
registers on this code path (those writes are stubbed but are not required for
the observed boot).

## Validation

Validated with:

```bash
cargo check -p clicky-desktop
RUST_LOG=error cargo run --release -p clicky-desktop -- \
  --autorun-zeroslackr \
  --hle=target/ipod-linux/ipodloader2_test.bin \
  --hdd=mem:file=target/ipod-linux/zeroslackr_256m.img
```

The autorun path selected ZeroSlackr without manual input and reached the IDE probe/read path.
