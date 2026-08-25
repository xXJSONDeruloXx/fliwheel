# RetailOS bring-up notes

This is a scratch journal for the Apple RetailOS path in `clicky`.
See also: `docs/DISCORD_RESEARCH.md` for the broader Discord + linked-resource notes.

## Input firmware

- Source IPSW: `/Users/kurt/Downloads/iPod_4.3.1.1.ipsw`
- It is a ZIP archive containing:
  - `Firmware-4.3.1.1`
  - `manifest.plist`
- Extracted firmware artifact used for testing:
  - `target/ipod-linux/retailos/Firmware-4.3.1.1`

## What we learned from the firmware

- The firmware parses as **format version 3** in `clicky`'s HLE loader.
- The IPSW contains both:
  - `osos` (main OS image)
  - `aupd` (Apple updater image)
- `clicky`'s HLE path extracted and loaded the `osos` image.

## Runtime behavior

- The HLE bootloader seeded the usual iPod 4G state:
  - `sysinfo_t` at `0x4000_ff18`
  - `IsyS` tag at `0x4001_7f18`
  - `boardHwSwInterfaceRev = 0x50014`
  - Hold bit asserted
- Early RetailOS setup ran far enough to program the PP5020 memory controller.
- Observed early MMU mappings included:
  - logical `0x3bf0` -> physical `0x3a88`
  - logical `0x3a00` -> physical `0x10000f84`
- The boot then died on an unmapped read:
  - faulting address: `0x0433bf14`
  - PC at crash: `0x000206e0`

## Current interpretation

This looks like "RetailOS is executing real code, but we are still missing something in early hardware/MMU state" rather than a bad firmware file.
The likely next leads are:

1. Decode the code path around `0x000206e0` / `0x0433bf14`.
2. Compare the RetailOS early init sequence with the HLE state we seed.
3. Check whether the crash is caused by an incomplete memory map, missing sysinfo fields, or another stubbed PP5020 block.
4. Re-run with more targeted MMIO / memory tracing once the next suspect register block is known.

## Notes

- The generated HDD image used for testing was `ipodhd.img`.
- The current run used the extracted firmware directly via HLE; the HDD image was not the source of the crash.
- The existing `README.md` roadmap now marks the iPodLinux bootloader smoke tests as complete.
