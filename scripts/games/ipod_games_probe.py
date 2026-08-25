#!/usr/bin/env python3
"""Inspect Apple clickwheel game bundles and firmware images.

This script is intended to support the `clickwheel-games` bring-up effort by
making it easy to inventory `Games_RO` packages and inspect their executable /
firmware metadata without needing any external tooling.
"""

from __future__ import annotations

import argparse
import json
import plistlib
import re
import struct
import sys
from collections import Counter
from pathlib import Path
from typing import Iterable

ASCII_RE = re.compile(rb"[\x20-\x7e]{4,}")
IMPORT_RE = re.compile(r"^\)([A-Za-z][A-Za-z0-9_]+)$")
INTERESTING_PATH_BITS = (
    ".sav",
    ".dat",
    "games_ro",
    "gamedata_rw",
    "gamestats_wo",
    "game/",
    "resources/",
    "localization/",
)
PATHISH_RE = re.compile(r"^[A-Za-z0-9_ ./\\:%+\-()]+$")
KNOWN_PATH_MARKERS = (
    "audio/",
    "fonts/",
    "data/",
    "game/",
    "gamedata/",
    "localization/",
    "media/",
    "resources/",
    "save/",
    "sounds/",
    "usertrivia/",
)
KNOWN_PATH_EXTENSIONS = (
    ".bin",
    ".dat",
    ".ipd",
    ".jpg",
    ".lcd5",
    ".m4a",
    ".mp3",
    ".pix",
    ".raw",
    ".rlb",
    ".ro",
    ".sav",
    ".strings",
    ".txt",
    ".wav",
    ".xml",
)
INTERESTING_FILE_SUFFIXES = (
    ".bin",
    ".sinf",
    ".rlb",
    ".ro",
    ".ipd",
    ".pix",
    ".raw",
    ".lcd5",
    ".wav",
    ".m4a",
    ".mp3",
    ".strings",
    ".txt",
    ".jpg",
)


def read_ascii_strings(data: bytes) -> list[str]:
    return [m.group().decode("ascii", errors="ignore") for m in ASCII_RE.finditer(data)]


def uniq(seq: Iterable[str]) -> list[str]:
    seen: set[str] = set()
    out: list[str] = []
    for item in seq:
        if item in seen:
            continue
        seen.add(item)
        out.append(item)
    return out


def is_pathish_string(s: str) -> bool:
    lower = s.lower()
    if lower.startswith("save"):
        return True
    if any(bit in lower for bit in INTERESTING_PATH_BITS):
        return True
    if "/" in s or "\\" in s:
        if not PATHISH_RE.match(s):
            return False
        if any(marker in lower for marker in KNOWN_PATH_MARKERS):
            return True
        if any(ext in lower for ext in KNOWN_PATH_EXTENSIONS):
            return True
    return False


class ProbeError(RuntimeError):
    pass


def parse_eapp_header(exe_path: Path) -> dict:
    data = exe_path.read_bytes()
    if len(data) < 0x28:
        raise ProbeError(f"{exe_path} is too small to contain an eapp header")

    magic = data[:4].decode("ascii", errors="replace")
    header_words = struct.unpack("<9I", data[4:0x28])
    code_word = struct.unpack("<I", data[0x28:0x2C])[0] if len(data) >= 0x2C else None

    strings = read_ascii_strings(data)
    early_strings = read_ascii_strings(data[:0x4000])
    imports = uniq(
        match.group(1)
        for s in early_strings
        if (match := IMPORT_RE.match(s)) is not None and any(ch.islower() for ch in match.group(1))
    )
    interesting_paths = uniq(s for s in strings if is_pathish_string(s))

    return {
        "path": str(exe_path),
        "size": len(data),
        "magic": magic,
        "load_addr_guess": header_words[0],
        "format_version_guess": header_words[1],
        "header_size": header_words[2],
        "raw_header_words": header_words,
        "first_code_word": code_word,
        "imports": imports,
        "interesting_paths": interesting_paths,
    }


def parse_game_manifest(game_dir: Path) -> dict:
    manifest_path = game_dir / "Manifest.plist"
    with manifest_path.open("rb") as f:
        manifest = plistlib.load(f)

    platforms = manifest.get("Platforms") or []
    files = manifest.get("Files") or []
    platform = platforms[0] if platforms else {}

    exe_rel = platform.get("ExecutablePath")
    exe_path = game_dir / exe_rel if exe_rel else None
    launch_art = platform.get("LaunchingArtwork")

    ext_counts = Counter()
    for entry in files:
        path = entry.get("Path")
        if not path:
            continue
        suffix = Path(path).suffix.lower() or "<none>"
        ext_counts[suffix] += 1

    result = {
        "id": game_dir.name,
        "name": manifest.get("Name"),
        "guid": manifest.get("GUID"),
        "version": manifest.get("Version"),
        "build_identifier": manifest.get("BuildIdentifier"),
        "platforms": platforms,
        "file_count": len(files),
        "file_extensions": dict(sorted(ext_counts.items())),
        "launch_art": launch_art,
        "executable": exe_rel,
    }

    if exe_path and exe_path.exists():
        result["eapp"] = parse_eapp_header(exe_path)
    else:
        result["eapp"] = None

    return result


def probe_games_root(games_root: Path) -> list[dict]:
    if not games_root.is_dir():
        raise ProbeError(f"{games_root} is not a directory")

    out = []
    for game_dir in sorted(p for p in games_root.iterdir() if p.is_dir()):
        manifest_path = game_dir / "Manifest.plist"
        if not manifest_path.exists():
            continue
        out.append(parse_game_manifest(game_dir))
    return out


def parse_firmware(path: Path) -> dict:
    with path.open("rb") as f:
        stop = f.read(256)
        if len(stop) != 256:
            raise ProbeError(f"{path} is too small to be an Apple firmware image")

        magic_hi, dir_offset, ext_header_loc, format_version = struct.unpack("<IIHH", f.read(12))
        if magic_hi != struct.unpack(">I", b"[hi]")[0]:
            raise ProbeError(f"{path} does not look like a format-v3 Apple firmware image")

        f.seek(dir_offset + 0x200)
        images = []
        while True:
            record = f.read(40)
            if len(record) < 40:
                break
            dev, name, image_id, dev_offset, length, addr, entry_offset, checksum, vers, load_addr = struct.unpack(
                "<4s4sIIIIIIII", record
            )
            if dev == b"\0\0\0\0":
                break
            images.append(
                {
                    "dev": dev.decode("ascii", errors="replace"),
                    "name": name.decode("ascii", errors="replace"),
                    "id": image_id,
                    "dev_offset": dev_offset,
                    "length": length,
                    "addr": addr,
                    "entry_offset": entry_offset,
                    "checksum": checksum,
                    "vers": vers,
                    "load_addr": load_addr,
                }
            )

    return {
        "path": str(path),
        "dir_offset": dir_offset,
        "ext_header_loc": ext_header_loc,
        "format_version": format_version,
        "images": images,
    }


def print_games_human(games: list[dict]) -> None:
    for game in games:
        print(f"[{game['id']}] {game['name']} v{game['version']}")
        print(f"  executable: {game['executable']}")
        print(f"  launch art: {game['launch_art']}")
        platform = (game.get("platforms") or [{}])[0]
        if platform:
            print(
                "  platform: "
                f"id={platform.get('PlatformID')} "
                f"version={platform.get('PlatformVersion')} "
                f"build={platform.get('BuildID')}"
            )
        eapp = game.get("eapp")
        if eapp:
            print(
                "  eapp: "
                f"magic={eapp['magic']} "
                f"load=0x{eapp['load_addr_guess']:08x} "
                f"fmt={eapp['format_version_guess']} "
                f"hdr=0x{eapp['header_size']:x}"
            )
            if eapp["imports"]:
                print(f"  imports: {', '.join(eapp['imports'])}")
            if eapp["interesting_paths"]:
                for path in eapp["interesting_paths"][:8]:
                    print(f"  path: {path}")
                extra = len(eapp["interesting_paths"]) - 8
                if extra > 0:
                    print(f"  path: ... ({extra} more)")
        ext_counts = game.get("file_extensions") or {}
        if ext_counts:
            top = sorted(ext_counts.items(), key=lambda kv: (-kv[1], kv[0]))[:6]
            summary = ", ".join(f"{suffix}:{count}" for suffix, count in top)
            print(f"  assets: {summary}")
        print()


def print_firmware_human(info: dict) -> None:
    print(Path(info['path']).name)
    print(f"  format_version: {info['format_version']}")
    print(f"  dir_offset: 0x{info['dir_offset']:x}")
    print(f"  ext_header_loc: {info['ext_header_loc']}")
    for image in info["images"]:
        print(
            "  image: "
            f"{image['dev']}/{image['name']} "
            f"offset=0x{image['dev_offset']:x} "
            f"len=0x{image['length']:x} "
            f"addr=0x{image['addr']:08x} "
            f"entry=0x{image['entry_offset']:x}"
        )


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_games = sub.add_parser("games", help="inspect a Games_RO directory")
    p_games.add_argument("games_root", type=Path)

    p_fw = sub.add_parser("firmware", help="inspect an Apple firmware file")
    p_fw.add_argument("firmware", type=Path)

    args = parser.parse_args(argv)

    try:
        if args.cmd == "games":
            result = probe_games_root(args.games_root)
            if args.json:
                print(json.dumps(result, indent=2))
            else:
                print_games_human(result)
            return 0

        if args.cmd == "firmware":
            result = parse_firmware(args.firmware)
            if args.json:
                print(json.dumps(result, indent=2))
            else:
                print_firmware_human(result)
            return 0
    except ProbeError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2

    parser.error("unknown command")
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
