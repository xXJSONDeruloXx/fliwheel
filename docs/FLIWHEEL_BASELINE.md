# fliwheel baseline and reference map

## Purpose

`fliwheel` is the focused decrypted-game branch of the iPod preservation work.
The first milestone is behavioral parity with the HLE already present in the
local `clicky` checkout. Later work can improve correctness and playability in
small, measured steps.

## Imported baseline

The source was copied from:

```text
repository: https://github.com/xXJSONDeruloXx/clicky
branch: clickwheel-games
commit: d1d735973f404ca53cd3e4b9f6e4e3dcb38b4df1
subject: feat: add Lost render-call patch experiment
```

The copy intentionally excludes `target/`, `data/`, and `docs/sc_info/`.
Those directories contain build output, saves/sidecars, FairPlay material, or
identity-bearing files. Decrypted games, firmware images, and run captures are
also local inputs rather than repository contents.

The active Cargo workspace now contains only `core` (`fliwheel-core`) and
`desktop` (`fliwheel-desktop`). The inherited full-device, web, bootloader, and
FairPlay implementation material is archived for provenance and is not part of
the decrypted-game build.

## Sibling references reviewed

| Repository | Local checkout | Starting commit | Relevance |
| --- | --- | --- | --- |
| `siggifly/ipod-emulator` | `~/Developer/ipod-emulator` | `f28c21364c459ca847909dc7201a3a0bddc5e340` | Full 5.5G firmware boot, ARMv4T machine, RetailOS, co-processor display, click wheel, and Brick path. |
| `siggifly/ipod-games` | `~/Developer/ipod-games` | `9bc6af446744b72df9d5d020793257d233983252` | `.ipg` structure, EAPP format, framework registry, ABI inventory, and HLE strategy. |
| `siggifly/ipod-drm-private` | `~/Developer/ipod-drm-private` | `17e676970686cdf82304e932e15efedc44496d8b` | FairPlay/keybag structure and the authorization/decryption boundary. |
| `siggifly/ipod-usb` | `~/Developer/ipod-usb` | `238112561e9c003b7622aaac7a4a60d560b49dd2` | Virtual iPod over USB, iTunes checkpoint protocol, and authorization workflow. |
| `siggifly/ipod-usb-private` | `~/Developer/ipod-usb-private` | `ea42f43472d6011bb2267810f55585f83bf71beb` | Rust implementation of the virtual-device identity/checkpoint plumbing. |
| `siggifly/opod` | `~/Developer/opod` | `e8f5f0a21c54819812c8cbbee364f22d92a6f602` | Longer-term open-hardware/player context; not part of the current HLE runtime. |

## What transfers immediately

From `clicky`, the usable starting point is the EAPP HLE: ARM execution,
framework import traps, asynchronous file access, input events, OpenGLES
state/draw tracking, texture upload association, live rendering, framebuffer
capture, DMA overlays, and the per-game investigation history.

From `ipod-emulator`, the useful future references are the clean ARMv4T and
RetailOS models, the firmware filesystem/loader work, the co-processor transport
measurements, and the distinction between measured hardware behavior and
documented bypasses. Its full-firmware path is not merged into this baseline;
it is a separate oracle/reference until an integration boundary is justified.

From `ipod-games` and the DRM/USB repositories, the useful boundaries are that
the current runner consumes decrypted EAPP bundles, while encrypted `.ipg`
packages require FairPlay authorization and are not a first milestone here.

## Iteration order

1. Preserve the baseline and make the decrypted-game smoke test reproducible.
2. Turn each game's current behavior into a small regression checkpoint.
3. Improve one missing contract at a time: DMA completion, texture association,
   VBO/pointer-backed draws, render-server behavior, or input timing.
4. Re-run the full decrypted corpus after each change and record regressions.
5. Use the firmware emulator and firmware images as behavioral references where
   the HLE lacks an answer, without mixing full-firmware and direct-EAPP results.
