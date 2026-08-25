# iPodLinux bring-up status

This is a checkpoint for the iPod 4G grayscale iPodLinux bring-up in clicky.
Generated firmware, HDD, rootfs, and patched historical Linux 2.4 build-tree artifacts live under ignored `target/ipod-linux/`.

## Current milestone: ZeroSlackr prebuilt image

The previous (from-scratch Linux 2.4) effort is parked in favour of booting the
**ZeroSlackr** prebuilt iPodLinux distribution, which lives in
`target/ipod-linux/zeroslackr_256m.img` and is loaded via the existing HLE
bootloader (`target/ipod-linux/ipodloader2_test.bin`).

The previous in-tree IDE/IRQ bring-up fixes (see
`docs/ipodlinux-ide-irq-findings.md`) are confirmed working under the
ZeroSlackr payload: the kernel performs thousands of clean multi-sector
`READ_SECTORS` (`Command=0x21`) transfers with no lost interrupts and reaches
ext2.

## ZeroSlackr image layout

The 256MB image has two partitions:

- partition 1 (5MB, firmware) - the iPodLoader2 firmware
- partition 2 (250MB, FAT32, label `ZEROSLACKR`) - everything else

ZeroSlackr's `loader.cfg` selects the kernel payload by menu index. The menu
indices are offset by one from the `loader.cfg` entry order (there is a title
slot), so:

- `default=1` -> Rockbox
- `default=2` -> Apple OS
- `default=3` -> Apple OS (observed)
- `default=4` -> **ZeroSlackr** (confirmed by visual inspection)

The image's `loader.cfg` has been edited to `default=4` so the loader
**auto-boots ZeroSlackr** after its 5s timeout, with no keypresses. This
replaces the older wall-clock based `--autorun-zeroslackr` flag, which was
unreliable because it raced the full-speed emulator thread.

**Important:** `target/ipod-linux/zeroslackr_256m.img` is ignored, so this
change is local-only and is **not** part of the git diff. To reproduce it on a
fresh checkout, patch `loader.cfg` inside the FAT32 partition with `mtools`:

```bash
PATH=/opt/homebrew/bin:$PATH \
  mtype -i target/ipod-linux/zeroslackr_256m.img@@$((12288*512)) ::/loader.cfg \
  > /tmp/loader.cfg
sed -i '' 's/^default=.*/default=4/' /tmp/loader.cfg
PATH=/opt/homebrew/bin:$PATH \
  mcopy -o -i target/ipod-linux/zeroslackr_256m.img@@$((12288*512)) \
  /tmp/loader.cfg ::/loader.cfg
```

The ZeroSlackr kernel command line is:

```text
ZeroSlackr @ (hd0,1)/boot/vmlinux root=/dev/hda2 rootfstype=vfat rw quiet
```

i.e. it mounts the **FAT32 partition 2** as the initial root (`/dev/hda2`),
then loopback-mounts `/boot/userland.ext3` (8MB) as the real userland. The
earlier from-scratch effort was chasing `root=/dev/hda3` ext2, which is the
commented-out `#iPodLinux` entry and is not what ZeroSlackr uses.

## Boot trajectory (observed)

With `default=4`, `RUST_LOG=error,IDE=info`, and the bring-up watchdog
(`CLICKY_WATCHDOG_MS` / `CLICKY_SAMPLE_MS`, see
`docs/ipodlinux-ide-irq-findings.md`), the boot proceeds as follows:

1. HLE firmware parse + os-image copy into SDRAM (`addr=0x10000000`,
   `entry_offset=0x200`). Note `0x40000000` is a mirror of SDRAM at
   `0x10000000`.
2. COP HLE preflight runs the COP from the kernel entry until it parks itself
   via `COP_CTL`. For the ZeroSlackr kernel the COP parks inside the kernel's
   own park trampoline (`ldr pc,[pc,#...]` wake stub, one instruction past the
   `COP_CTL = PROC_SLEEP` write) - this is correct and matches real hardware.
3. The CPU reaches `stext` at `0x10008000`, runs the CPU-vs-COP `PROC_ID`
   detection, then spends ~50s of host wall-clock on heavy early kernel init in
   SDRAM (`0x4000xxxx`): memory init, hash/page-table builds, bootmem. These
   are legitimate finite loops (e.g. `umull`+shift hash-to-bucket); they look
   like stalls under single-step emulation but do make forward progress.
4. Kernel self-decompression/relocation: the PC passes through
   `0x143820` / `0x370f0` / `0xbefc` / `0x144684` / `0x14491c`.
5. **Current blocker:** after relocation the CPU parks in a tight busy-wait at
   `0x188ac`-`0x188c4` (SVC mode) that never exits. Across every sample
   `r0=0x002f6000` and `r1=0x00162000` are fixed (they look like SDRAM region
   bounds), with `r3` alternating around `0x001765ac`. The COP remains parked at
   `0x0001cb9c`. This is a genuine busy-wait on a memory/register condition
   that never asserts, not slow work.

## Next priorities

1. Dump the live instructions at the `0x188ac`-`0x188c4` loop (the watchdog
   already reads code through the system bus) and identify the polled
   condition: most likely a hardware register (timer / cache flush / COP
   handshake) that is stubbed or modelled incorrectly, or a flag the COP is
   supposed to set.
2. If the loop polls the COP, investigate why the kernel does not wake the COP
   (the mailbox `COP_QUEUE` writes are stubbed; confirm whether they are needed
   for this code path).
3. Once the `0x188xx` wait resolves, continue to root mount, `userland.ext3`
   loopback mount, and PodZilla/uClibc userspace.

## Bring-up tools

- `CLICKY_WATCHDOG_MS=<ms>` - hang watchdog with tight-loop detection and a
  live code/register dump through the system bus. Inert unless set.
- `CLICKY_SAMPLE_MS=<ms>` - periodic PC + register sampler. Inert unless set.
- `clicky-desktop --autorun-zeroslackr` - synthesizes the iPodLoader menu
  keys (kept for reference; `default=4` auto-boot is now preferred).

---

# Previous effort: from-scratch Linux 2.4 (parked)

The sections below describe the earlier in-tree bring-up of a patched Linux
2.4 against a hand-built ext2 rootfs. That path reached ext2 `iget` of the root
inode before being superseded by the ZeroSlackr prebuilt approach above.

## Current milestone

The emulator now gets iPodLinux well past early hardware initialization and into the root filesystem mount path:

- HLE firmware boots the iPodLinux loader/kernel image.
- Kernel reaches `prepare_namespace` with bring-up shims for the currently broken Linux 2.4 `context_thread`/keventd path.
- Static initcalls are restored in the ignored kernel tree, so ext2 and IDE init run normally.
- IDE probing and special commands complete; the previous ATA/IRQ stalls are fixed in tracked source.
- `/dev/root` resolves to the HDD's third partition (`/dev/hda3`, `0x0303`).
- `ext3` is tried first and fails as expected.
- `ext2_read_super` for `/dev/root` reads the superblock, matches magic `0xef53`, reads group descriptors, and reaches `iget(sb, EXT2_ROOT_INO)` for the root inode.

## Latest blocker

Root inode loading currently stalls after submitting the inode-table block read:

```text
ext2_read_super iget root inode
iget4 sb=0x0027bc00 ino=0x00000002
ext2_read_inode inode=0x002ff080 ino=0x00000002
ext2_read_inode bread dev=0x00000303 block=0x00000084 size=0x00000400
submit_bh rw=READ bh=0x00253f60
__wait_on_buffer bh=0x00253f60
__wait_on_buffer scheduling bh=0x00253f60
```

Earlier ext2 superblock and group-descriptor reads complete through IDE interrupt and `end_buffer_io_sync`, so the active question is why this later root-inode block read does not complete within the diagnostic window.

## Important ignored artifact shims

These are intentionally under ignored `target/ipod-linux/kernel-2.4-build/` and are not committed source changes:

- Linux 2.4 modern-toolchain build fixes.
- `copy_thread`/`memset` compatibility shims for old ARM assembly assumptions.
- `include/linux/init.h` `used` attributes to retain static initcalls with modern GCC.
- Temporary bring-up bypasses for `start_context_thread`, `flush_scheduled_tasks`, and selected thread-backed initcalls.
- Temporary `sys_mount` cleanup leak in `fs/namespace.c` to bypass old-kernel slab/free corruption while rootfs bring-up is diagnosed.

## Rootfs image note

The rootfs must be generated with Linux-2.4-compatible 128-byte ext2 inodes. The current ignored image was rebuilt with:

```bash
/opt/homebrew/bin/mke2fs -q -t ext2 -b 1024 -I 128 \
  -O filetype,sparse_super,large_file \
  -d target/ipod-linux/rootfs-src \
  -F target/ipod-linux/rootfs.ext2 32768

dd if=target/ipod-linux/rootfs.ext2 \
  of=target/ipod-linux/ipodlinux_hdd.img \
  bs=512 seek=47104 conv=notrunc status=none
```

Verification of partition 3 currently shows:

```text
magic=53ef inode_size=128 compat=0x38 incompat=0x2 ro_compat=0x3
```

## Next priorities

1. Instrument the specific root inode read path: `ext2_read_inode`, `bread`, `ll_rw_block`, `submit_bh`, `__wait_on_buffer`, IDE request completion, and `end_buffer_io_sync` for block `0x84`.
2. Determine whether the root-inode read is losing an IDE completion/interrupt, stuck in buffer wait state, or reading malformed/uninitialized data.
3. If root inode loads, continue to `d_alloc_root`, `prepare_namespace` success, `sys_chroot`, `run_init_process`, and then document/add real ARM/uClinux userspace as needed.
