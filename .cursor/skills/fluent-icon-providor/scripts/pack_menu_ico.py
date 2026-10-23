#!/usr/bin/env python3
"""Pack or verify TrafficBar menu ICOs (PNG frames: 16,20,24,28,32 in order)."""

from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

SIZES = (16, 20, 24, 28, 32)
PNG_SIG = b"\x89PNG\r\n\x1a\n"


def pack_ico(frames: list[tuple[int, bytes]]) -> bytes:
    """frames: list of (size, png_bytes) in display order."""
    if [s for s, _ in frames] != list(SIZES):
        raise SystemExit(f"expected sizes {list(SIZES)}, got {[s for s, _ in frames]}")
    for s, blob in frames:
        if not blob.startswith(PNG_SIG):
            raise SystemExit(f"{s}px payload is not PNG")
    count = len(frames)
    offset = 6 + 16 * count
    header = struct.pack("<HHH", 0, 1, count)
    directories: list[bytes] = []
    payloads: list[bytes] = []
    for size, blob in frames:
        ww = 0 if size >= 256 else size
        directories.append(struct.pack("<BBBBHHII", ww, ww, 0, 0, 1, 32, len(blob), offset))
        payloads.append(blob)
        offset += len(blob)
    return header + b"".join(directories) + b"".join(payloads)


def read_entries(path: Path) -> list[tuple[int, int, int, bool, int]]:
    data = path.read_bytes()
    _, typ, count = struct.unpack_from("<HHH", data, 0)
    if typ != 1:
        raise SystemExit(f"{path}: not an ICO (type={typ})")
    out: list[tuple[int, int, int, bool, int]] = []
    for i in range(count):
        off = 6 + i * 16
        w, h, _colors, _res, _planes, bpp, size, offset = struct.unpack_from(
            "<BBBBHHII", data, off
        )
        w = 256 if w == 0 else w
        h = 256 if h == 0 else h
        png = data[offset : offset + 8] == PNG_SIG
        out.append((w, h, bpp, png, size))
    return out


def verify(path: Path) -> None:
    ents = read_entries(path)
    ok = True
    expected = [(s, s, 32, True) for s in SIZES]
    got = [(w, h, bpp, png) for w, h, bpp, png, _ in ents]
    if got != expected:
        ok = False
        print(f"FAIL {path}")
        print(f"  expected: {expected}")
        print(f"  got:      {got}")
    else:
        print(f"OK {path}: {len(ents)} PNG frames {list(SIZES)}")
    if not ok:
        raise SystemExit(1)


def load_pngs_from_dir(directory: Path) -> list[tuple[int, bytes]]:
    frames: list[tuple[int, bytes]] = []
    for s in SIZES:
        p = directory / f"{s}.png"
        if not p.is_file():
            raise SystemExit(f"missing {p}")
        frames.append((s, p.read_bytes()))
    return frames


def load_pngs_from_args(pairs: list[str]) -> list[tuple[int, bytes]]:
    by_size: dict[int, bytes] = {}
    for item in pairs:
        if "=" not in item:
            raise SystemExit(f"--png expects SIZE=path, got {item!r}")
        size_s, path_s = item.split("=", 1)
        size = int(size_s)
        by_size[size] = Path(path_s).read_bytes()
    missing = [s for s in SIZES if s not in by_size]
    if missing:
        raise SystemExit(f"missing --png for sizes: {missing}")
    return [(s, by_size[s]) for s in SIZES]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--verify", type=Path, help="verify an existing menu ICO")
    ap.add_argument("--out", type=Path, help="output .ico path")
    ap.add_argument("--dir", type=Path, help="directory with 16.png..32.png")
    ap.add_argument(
        "--png",
        action="append",
        default=[],
        help="SIZE=path.png (repeat for 16/20/24/28/32)",
    )
    args = ap.parse_args()

    if args.verify:
        verify(args.verify)
        return

    if not args.out:
        ap.error("--out is required unless --verify")
    if args.dir:
        frames = load_pngs_from_dir(args.dir)
    elif args.png:
        frames = load_pngs_from_args(args.png)
    else:
        ap.error("provide --dir or --png")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(pack_ico(frames))
    verify(args.out)


if __name__ == "__main__":
    main()
