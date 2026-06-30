#!/usr/bin/env python3
"""Bake the macOS app icon (the level-meter mark) PROCEDURALLY with Pillow.

The original SVG→PNG pipeline (`make-rounded-icns.py`) was deleted and is
unrecoverable, and no SVG rasterizer is installed — so this redraws the mark
directly: a terracotta (#d97757) INSET squircle (art body 824/1024, ~10%
transparent margin) with an Apple-style rounded corner + 3 white bottom-aligned
bars in a 6:11:8 rhythm. The inset + rounder corner make it read smaller and
less boxy in the Dock next to Apple's icons (CLAUDE.md: the grid-inset waiver is
reverted — the Dock-size concern returned via user report).

Emits ONLY the macOS-facing assets:
  • desktop ladder 32 / 64 / 128 / 128@2x(256) / icon.png(1024)
  • icon.icns  (packed via `iconutil` from a generated .iconset)
  • dock.png   (baked 256px — the dev Dock can't round programmatically-set icons
                pre-macOS 26, so the squircle is baked into the pixels)

Does NOT touch iOS / Android / Windows / source.png (full-bleed / own-mask).

Run:  python3 scripts/bake_icon.py
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw

ICONS = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"

S = 1024  # design canvas
SS = 4  # supersample factor for clean AA edges
MARGIN = 100  # ~10% inset → body 824 (Apple macOS grid)
BODY = S - 2 * MARGIN  # 824
RADIUS = round(0.2237 * BODY)  # ≈184 — the macOS Big Sur squircle corner
BS = BODY / S  # 0.8047 — shrink the full-bleed composition into the body

TERRA = (217, 119, 87, 255)  # #d97757
WHITE = (255, 255, 255, 255)

# Full-bleed bar geometry (1024-space, bottom-aligned at y=768), from
# tmp-icon-fullbleed.svg — heights 279.27 : 512 : 372.36 ≈ 6 : 11 : 8.
BARS = [
    (256.00, 488.73, 102.40, 279.27),
    (460.80, 256.00, 102.40, 512.00),
    (665.60, 395.64, 102.40, 372.36),
]
BAR_RX = 16.38


def render(px: int) -> Image.Image:
    """Render the inset squircle + bars at `px`×`px`, supersampled then downscaled."""
    k = (px * SS) / S  # 1024-space → supersampled pixels
    img = Image.new("RGBA", (px * SS, px * SS), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # terracotta squircle body, inset by MARGIN in 1024-space
    d.rounded_rectangle(
        [MARGIN * k, MARGIN * k, (S - MARGIN) * k, (S - MARGIN) * k],
        radius=RADIUS * k,
        fill=TERRA,
    )
    # 3 white bars — the full-bleed composition scaled into the body, keeping its
    # relative margins (bars float in the middle band, not touching the edges).
    for bx, by, bw, bh in BARS:
        x0 = MARGIN + bx * BS
        y0 = MARGIN + by * BS
        d.rounded_rectangle(
            [x0 * k, y0 * k, (x0 + bw * BS) * k, (y0 + bh * BS) * k],
            radius=BAR_RX * BS * k,
            fill=WHITE,
        )
    return img.resize((px, px), Image.LANCZOS)


def main() -> int:
    if not ICONS.is_dir():
        print(f"icons dir not found: {ICONS}", file=sys.stderr)
        return 1

    # desktop ladder (inset). NOT iOS/Android/Windows/source.png.
    ladder = {
        "32x32.png": 32,
        "64x64.png": 64,
        "128x128.png": 128,
        "128x128@2x.png": 256,
        "icon.png": 1024,
        "dock.png": 256,
    }
    for name, px in ladder.items():
        render(px).save(ICONS / name)
        print(f"  {name}  ({px}px)")

    # icon.icns via iconutil (needs a named .iconset)
    iconset_sizes = {
        "icon_16x16.png": 16,
        "icon_16x16@2x.png": 32,
        "icon_32x32.png": 32,
        "icon_32x32@2x.png": 64,
        "icon_128x128.png": 128,
        "icon_128x128@2x.png": 256,
        "icon_256x256.png": 256,
        "icon_256x256@2x.png": 512,
        "icon_512x512.png": 512,
        "icon_512x512@2x.png": 1024,
    }
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "icon.iconset"
        iconset.mkdir()
        for name, px in iconset_sizes.items():
            render(px).save(iconset / name)
        subprocess.run(
            ["iconutil", "-c", "icns", str(iconset), "-o", str(ICONS / "icon.icns")],
            check=True,
        )
    print("  icon.icns (iconutil)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
