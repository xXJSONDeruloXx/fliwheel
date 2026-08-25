# Discord research notes

This is a synthesis of the `clicky` Discord discussions and the linked reference material that came up there.

## Channel map

- `general` / `general-main`:
  onboarding, firmware sourcing, boot paths, flash ROM, and project status.
- `dev`:
  day-to-day bring-up, HLE booting, MMIO crashes, and loader smoke tests.
- `flash-rom`:
  NOR/flash command interface, CFI, chip IDs, and boot-strap experiments.
- `retailos`:
  Apple RetailOS MMU/layout/reverse-engineering notes.
- `devices` / `architecture` / `rust-help`:
  mostly quiet.

## Key findings from the discussions

### Boot flow / firmware format

- Apple firmware is distributed as an IPSW zip containing a raw `Firmware-4.3.1.1` image.
- The `Firmware` image contains `osos` (main OS image) and `aupd` (flash-ROM updater).
- `clicky`'s HLE path can extract and boot `osos` directly without a physical flash-ROM dump.
- The iPod boot chain starts at flash/ROM at address `0x0`, then loads a second-stage image from disk.
- iPodLinux and Rockbox generally do **not** replace the stock Apple bootloader; they point the OS image at their own second-stage loader.
- Apple diagnostics are in flash ROM and are entered via the Select+Prev key combo.

### Hardware / platform notes

- The project targets the iPod 4G grayscale first because it uses the PP5020 family and a simpler LCD.
- The iPod 5G / later models are close relatives; the 5G uses PP5022C in software.
- The memory map and MMIO space are still only partially understood; several `0x60009xxx` and related regions remain mysterious.
- The `sysinfo_t` block matters for early boot:
  - `IsyS` tag at `0x40017f18` / `0x4001ff18`
  - pointer at `0x40017f1c` / `0x4001ff1c`
  - `boardHwSwInterfaceRev` is used for older sysinfo layouts
  - `sdram_zero2` is used for newer layouts / some 5G-ish paths
- `0x6000d060` is the GPIO port-A interrupt level register used by the hold-switch path in iPodLoader2.

### Flash ROM / NOR notes

- Flash is treated as NOR ROM most of the time, but it has a command interface.
- Retrieving chip ID uses CFI-style command sequences.
- Apple appears to reverse the flash address bus in some updater paths.
- `0x70000030` / `DEV_TIMING1` showed up as a gating flag in Flash ROM bootloader work; it was described in chat as a boot-progress / timing-related hack.
- AUPD appears to be the flash-ROM updater, not the flash ROM image itself.
- Boot-from-ROM vs boot-from-ATA paths may initialize device state differently, which may explain some hangs after jumping into loaded images.

### RetailOS notes

- RetailOS appears to load `osos` at `0x10000000` on the 5G-family devices.
- RTXC 3.2 was suggested as a likely RTOS, though that is not confirmed.
- A framebuffer address of `0x108867b0` was reported from a 5G RetailOS session.
- RetailOS and the boot ROM expose useful debug facilities:
  - semihosting SWIs (`swi 0x123456`)
  - UART / SER0 output
  - boot ROM debug menu / peek-poke style commands
- In `clicky`, the RetailOS HLE boot currently reaches real MMU setup and then dies on an unmapped read at `0x0433bf14` with PC around `0x000206e0`.
- That looks more like missing early state / MMU mapping than a bad firmware file.

### iPodLoader2 / loader behavior

- A recurring bug was a GPIO write-size mismatch in iPodLoader2.
- The relevant discussion converged on the idea that PP502x GPIOs have an atomic bitwise-write region and that `clicky` should model it correctly.
- After the loader fix, the iPodLoader2 smoke test booted cleanly in `clicky`.

## Useful external resources discussed

### iPodLinux

- `http://www.ipodlinux.org/Firmware.html`
  - Firmware format and boot process.
  - Confirms the bootloader-at-0x0 / second-stage-loader model.
- `http://www.ipodlinux.org/GPIO.html`
  - GPIO mapping / button matrix reference.
- `http://www.ipodlinux.org/Generations/`
  - Hardware comparisons across iPod models.

### Rockbox

- `https://www.rockbox.org/wiki/IpodFlash`
  - Flash-ROM dump / boot ROM notes.
- `https://www.rockbox.org/wiki/IpodPort`
  - Supported iPod generations and port status.
- `https://www.rockbox.org/wiki/PortalPlayer502x`
  - PP502x register / GPIO / MMIO details.

### Mirrored local docs already in repo

- `resources/documentation/memory_controller.txt`
  - Memory controller and cache mapping details.
- `resources/documentation/Rockbox/pp5020.h`
  - GPIO atomic write macros, PP5020 init regs, and the `0x800` alias trick.
- `resources/NOTES.md`
  - Apple boot chain / HLE bootloader reasoning and flash-ROM strategy.
- `resources/documentation/iPodLinux/*`
  - Archived iPodLinux docs and PDFs, including firmware / flash-decryption / PP50xx references.

### Source / repo references

- `github.com/iPodLinux/iPodLinux-SVN`
  - `apps/ipod/getflash/getflash.c`
  - `apps/ipod/ipodloader2/ipodhw.c`
- `github.com/crozone/ipodloader2`
  - `keypad.c` and `bootloader.h` for the GPIO write-size issue.
- `github.com/Rockbox/rockbox`
  - `apps/plugins/iriver_flash.c`
  - `firmware/export/config/ipodvideo.h`
- `github.com/freemyipod/wInd3x`
  - Mentioned as a way to decrypt firmware on-device for newer iPods.
- `https://daniel.haxx.se/sansa/memory_controller.txt`
  - The best public MMU/cache writeup we found.

## What seems most solid

- The iPod boot chain in `clicky` is now good enough to reach real RetailOS initialization.
- The early-MMU / memory-controller behavior is close to the real PP502x model, but not complete.
- Flash-ROM/bootloader work is still useful for debugging, but HLE is enough to make progress.
- Loader2 and Rockbox are good canaries for missing MMIO details, especially GPIO and cache/MMU setup.

## Open questions

- What exact mapping is expected around `0x0433bf14` in RetailOS early boot?
- Which MMIO regions in the `0x60009xxx` / adjacent space are still missing?
- How much of the flash-ROM updater handshake is real hardware timing vs emulator artifact?
- Does the RetailOS path need more sysinfo fields or more accurate PP502x mapping metadata?
