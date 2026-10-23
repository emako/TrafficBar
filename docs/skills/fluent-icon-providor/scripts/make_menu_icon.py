#!/usr/bin/env python3
"""Build menu light/dark ICOs from Fluent System Icons Regular SVGs via svg2png."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path

SIZES = (16, 20, 24, 28, 32)
LIGHT = "#212121"
DARK = "#FFFFFF"
FILL_RE = re.compile(
    r'fill="(#(?:212121|000000|000|111111|1[aA]1[aA]1[aA]|333333|black)|black)"',
    re.IGNORECASE,
)


def latest_fluent_regular_dir() -> Path:
    root = Path.home() / ".nuget" / "packages" / "fluentui.systemicons.assets"
    if not root.is_dir():
        raise SystemExit(f"NuGet package not found: {root}")
    versions = []
    for p in root.iterdir():
        if not p.is_dir():
            continue
        try:
            versions.append((tuple(int(x) for x in p.name.split(".")), p))
        except ValueError:
            continue
    if not versions:
        raise SystemExit(f"no versions under {root}")
    versions.sort(key=lambda t: t[0], reverse=True)
    svg_dir = versions[0][1] / "assets" / "svg" / "FluentSystemIcons-Regular"
    if not svg_dir.is_dir():
        raise SystemExit(f"Regular SVG dir missing: {svg_dir}")
    return svg_dir


def pick_source_svg(svg_dir: Path, glyph: str, target: int) -> Path:
    """Prefer exact size; else nearest available (prefer larger)."""
    exact = svg_dir / f"ic_fluent_{glyph}_{target}_regular.svg"
    if exact.is_file():
        return exact
    available = []
    for p in svg_dir.glob(f"ic_fluent_{glyph}_*_regular.svg"):
        m = re.search(rf"ic_fluent_{re.escape(glyph)}_(\d+)_regular\.svg$", p.name)
        if m:
            available.append((int(m.group(1)), p))
    if not available:
        raise SystemExit(f"no SVGs for glyph {glyph!r} in {svg_dir}")
    # closest size; prefer >= target, then larger
    available.sort(
        key=lambda t: (abs(t[0] - target), 0 if t[0] >= target else 1, -t[0])
    )
    chosen = available[0][1]
    print(f"  note: size {target} missing; using {chosen.name}")
    return chosen


def recolor_svg(text: str, color: str) -> str:
    # Fluent Regular uses fill="#212121" on paths; keep fill="none".
    def repl(m: re.Match[str]) -> str:
        return f'fill="{color}"'

    out, n = FILL_RE.subn(repl, text)
    if n == 0 and f'fill="{color}"' not in out:
        # last resort: any fill="#......" that is not none
        out2, n2 = re.subn(
            r'fill="#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})"',
            f'fill="{color}"',
            out,
        )
        if n2 == 0:
            raise SystemExit("could not recolor SVG fills")
        out = out2
    return out


def resolve_svg2png(svg2png_root: Path) -> list[str]:
    dist = svg2png_root / "dist" / "cli.js"
    cli_ts = svg2png_root / "src" / "cli.ts"
    if dist.is_file():
        return ["node", str(dist)]
    if cli_ts.is_file():
        return ["npx", "--prefix", str(svg2png_root), "ts-node", str(cli_ts)]
    raise SystemExit(
        f"svg2png CLI not found under {svg2png_root} (need dist/cli.js or src/cli.ts)"
    )


def run_svg2png(cmd_base: list[str], svg: Path, png: Path, size: int) -> None:
    cmd = cmd_base + [
        str(svg),
        "-o",
        str(png),
        "--width",
        str(size),
        "--height",
        str(size),
    ]
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        raise SystemExit(f"svg2png failed for {svg} -> {png}")


def build_theme(
    *,
    svg_dir: Path,
    glyph: str,
    color: str,
    theme: str,
    work: Path,
    cmd_base: list[str],
) -> Path:
    theme_dir = work / theme
    theme_dir.mkdir(parents=True, exist_ok=True)
    for size in SIZES:
        src = pick_source_svg(svg_dir, glyph, size)
        colored = recolor_svg(src.read_text(encoding="utf-8"), color)
        svg_out = theme_dir / f"{size}.svg"
        svg_out.write_text(colored, encoding="utf-8")
        png_out = theme_dir / f"{size}.png"
        print(f"  [{theme}] {size}px")
        run_svg2png(cmd_base, svg_out, png_out, size)
    return theme_dir


def pack(pack_script: Path, png_dir: Path, out_ico: Path) -> None:
    r = subprocess.run(
        [sys.executable, str(pack_script), "--out", str(out_ico), "--dir", str(png_dir)],
        capture_output=True,
        text=True,
    )
    sys.stdout.write(r.stdout)
    if r.returncode != 0:
        sys.stderr.write(r.stderr)
        raise SystemExit(f"pack failed: {out_ico}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--glyph", required=True, help="Fluent glyph, e.g. settings")
    ap.add_argument("--stem", help="output stem (default: same as --glyph)")
    ap.add_argument(
        "--out-dir",
        type=Path,
        default=Path("src/assets/icons/menu"),
        help="menu icons directory",
    )
    ap.add_argument(
        "--svg2png",
        type=Path,
        default=Path(r"D:\GitHub\svg2png"),
        help="svg2png repo root",
    )
    ap.add_argument(
        "--svg-dir",
        type=Path,
        help="override FluentSystemIcons-Regular directory",
    )
    args = ap.parse_args()
    stem = args.stem or args.glyph
    svg_dir = args.svg_dir or latest_fluent_regular_dir()
    print(f"SVG source: {svg_dir}")
    cmd_base = resolve_svg2png(args.svg2png)
    print(f"svg2png: {' '.join(cmd_base)}")

    pack_script = Path(__file__).with_name("pack_menu_ico.py")
    args.out_dir.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="menu_ico_") as tmp:
        work = Path(tmp)
        light_dir = build_theme(
            svg_dir=svg_dir,
            glyph=args.glyph,
            color=LIGHT,
            theme="light",
            work=work,
            cmd_base=cmd_base,
        )
        dark_dir = build_theme(
            svg_dir=svg_dir,
            glyph=args.glyph,
            color=DARK,
            theme="dark",
            work=work,
            cmd_base=cmd_base,
        )
        light_ico = args.out_dir / f"{stem}_light.ico"
        dark_ico = args.out_dir / f"{stem}_dark.ico"
        pack(pack_script, light_dir, light_ico)
        pack(pack_script, dark_dir, dark_ico)

    print(f"wrote {light_ico}")
    print(f"wrote {dark_ico}")


if __name__ == "__main__":
    main()
