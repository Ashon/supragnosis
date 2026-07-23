#!/usr/bin/env python3
"""Renders the supragnosis app icon set from the canonical design (icon.svg documents it;
this script IS the renderer - same geometry constants, so the two must be edited together).

macOS app-icon geometry: a 1024px transparent canvas with a centered rounded tile of 824px
(margin 100px, corner radius 185px) - the dock supplies no mask, so a full-bleed square looks
wrong next to every other icon. The asterisk mark is drawn as 5 round-capped spokes.

Pure-stdlib rasterizer (signed-distance coverage, 1px analytic anti-aliasing) so the render
is reproducible with no image libraries; resizing (sips) and icns assembly (iconutil) use the
macOS system tools. Usage: python3 generate.py
"""

import math
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

SIZE = 1024
MARGIN = 100.0  # transparent border: tile = 824/1024 of the canvas (Apple icon grid)
RADIUS = 185.0  # tile corner radius
TILE_RGB = (0x0C, 0x0E, 0x14)  # the viewer's candlelight background
GOLD_RGB = (0xF0, 0xC4, 0x69)  # the asterisk mark
RAY_LEN = 193.0  # spoke length from center (55% mark diameter relative to the tile)
RAY_W = 71.0  # spoke stroke width (round caps)
ANGLES = [-90, -18, 54, 126, 198]  # 5 spokes, 72 degree steps, one arm up

# Menu-bar (tray) template image: the bare mark, no tile, black-on-transparent - macOS renders a
# template purely from alpha, adapting to light/dark menu bars. Larger than the tiled mark so it
# holds presence at 22pt.
TRAY_RAY_LEN = 380.0
TRAY_RAY_W = 140.0
TRAY_PNG = ("tray.png", 44)  # 22pt @2x

# (iconset entry name, pixel size) - everything iconutil wants, tauri reuses a subset.
ICONSET = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
]
TAURI_PNGS = [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256)]


def sd_round_rect(px, py, c, half, r):
    qx = abs(px - c) - (half - r)
    qy = abs(py - c) - (half - r)
    return math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - r


def sd_segment(px, py, ax, ay, bx, by):
    pax, pay = px - ax, py - ay
    bax, bay = bx - ax, by - ay
    h = max(0.0, min(1.0, (pax * bax + pay * bay) / (bax * bax + bay * bay)))
    return math.hypot(pax - bax * h, pay - bay * h)


def coverage(d):
    """1px analytic anti-aliasing over the signed distance."""
    return max(0.0, min(1.0, 0.5 - d))


def spokes(ray_len):
    c = SIZE / 2.0
    out = []
    for a in ANGLES:
        t = math.radians(a)
        out.append((c, c, c + ray_len * math.cos(t), c + ray_len * math.sin(t)))
    return out


def render_tray_1024():
    """Bare mark, black, alpha = coverage (a macOS template image)."""
    c = SIZE / 2.0
    rays = spokes(TRAY_RAY_LEN)
    reach = TRAY_RAY_LEN + TRAY_RAY_W
    rows = []
    for y in range(SIZE):
        py = y + 0.5
        row = bytearray([0])
        for x in range(SIZE):
            px = x + 0.5
            a_mark = 0.0
            if math.hypot(px - c, py - c) <= reach:
                for ax, ay, bx, by in rays:
                    a_mark = max(
                        a_mark, coverage(sd_segment(px, py, ax, ay, bx, by) - TRAY_RAY_W / 2.0)
                    )
                    if a_mark >= 1.0:
                        break
            row += bytes((0, 0, 0, round(255 * a_mark)))
        rows.append(bytes(row))
    return b"".join(rows)


def render_1024():
    c = SIZE / 2.0
    half = c - MARGIN
    rays = spokes(RAY_LEN)
    mark_reach = RAY_LEN + RAY_W  # quick-reject radius for the spoke tests
    rows = []
    for y in range(SIZE):
        py = y + 0.5
        row = bytearray([0])  # PNG filter type 0
        for x in range(SIZE):
            px = x + 0.5
            a_tile = coverage(sd_round_rect(px, py, c, half, RADIUS))
            if a_tile == 0.0:
                row += b"\x00\x00\x00\x00"
                continue
            a_mark = 0.0
            if math.hypot(px - c, py - c) <= mark_reach:
                for ax, ay, bx, by in rays:
                    a_mark = max(a_mark, coverage(sd_segment(px, py, ax, ay, bx, by) - RAY_W / 2.0))
                    if a_mark >= 1.0:
                        break
            row += bytes(
                (
                    round(TILE_RGB[0] + (GOLD_RGB[0] - TILE_RGB[0]) * a_mark),
                    round(TILE_RGB[1] + (GOLD_RGB[1] - TILE_RGB[1]) * a_mark),
                    round(TILE_RGB[2] + (GOLD_RGB[2] - TILE_RGB[2]) * a_mark),
                    round(255 * a_tile),
                )
            )
        rows.append(bytes(row))
    return b"".join(rows)


def write_png(path, raster):
    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    ihdr = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)  # 8-bit RGBA
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raster, 9))
        + chunk(b"IEND", b"")
    )
    Path(path).write_bytes(png)


def main():
    here = Path(__file__).resolve().parent
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp) / "icon_1024.png"
        print("rendering 1024px master (pure-python SDF rasterizer)...")
        write_png(base, render_1024())

        iconset = Path(tmp) / "supragnosis.iconset"
        iconset.mkdir()
        for name, px in ICONSET:
            subprocess.run(
                ["sips", "-z", str(px), str(px), str(base), "--out", str(iconset / name)],
                check=True,
                capture_output=True,
            )
        for name, px in TAURI_PNGS:
            subprocess.run(
                ["sips", "-z", str(px), str(px), str(base), "--out", str(here / name)],
                check=True,
                capture_output=True,
            )
        subprocess.run(
            ["iconutil", "-c", "icns", str(iconset), "-o", str(here / "icon.icns")],
            check=True,
        )

        print("rendering tray template mark...")
        tray_base = Path(tmp) / "tray_1024.png"
        write_png(tray_base, render_tray_1024())
        name, px = TRAY_PNG
        subprocess.run(
            ["sips", "-z", str(px), str(px), str(tray_base), "--out", str(here / name)],
            check=True,
            capture_output=True,
        )
    print("wrote", ", ".join(n for n, _ in TAURI_PNGS), "tray.png, and icon.icns")


if __name__ == "__main__":
    sys.exit(main())
