import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

const icon = (path: string) =>
  createHash("sha256")
    .update(readFileSync(join(process.cwd(), "src-tauri/icons", path)))
    .digest("hex");

describe("app icon handoff assets", () => {
  // Level-meter mark (terracotta squircle + 3 white bars, 6:11:8). macOS icon.icns +
  // dock.png are now the INSET baked squircle (art body 824/1024 + the rounder macOS
  // Big Sur corner, transparent corners) — re-baked from scratch by scripts/bake_icon.py
  // (Pillow, procedural) because the mark read oversized + boxy in the Dock next to
  // Apple's icons (the earlier "inset waived" decision is reverted). A plain Tauri .icns
  // is rendered as-is, so the rounding is baked into the pixels. iOS = full-bleed opaque;
  // Android adaptive carries a monochrome (themed-icon) layer — neither is inset.
  it("keeps the level-meter mark source and platform assets", () => {
    expect(icon("icon.icns")).toBe(
      "9392a13cd4685f23c16f337911f77e1dc412925504b78b33c0b97219ddbe165f",
    );
    expect(icon("tmp-icon-master.svg")).toBe(
      "2b08777bb79d8ed9362329c46d226ca0483769dfeee0d8536ea61d77fc84ece8",
    );
    expect(icon("tmp-icon-foreground.svg")).toBe(
      "471efaed227e8c78c3152d8cd45861aefad844faa94187fafc80686457610995",
    );
    expect(icon("tmp-icon-background.svg")).toBe(
      "6d1b9d1f877c15afb46423cb94064ef1dd895485851802d8443ab76321b7125d",
    );
    expect(icon("source.png")).toBe(
      "d2898056ffbdb49c88369ce76cfc6a128d8959afb9d6316503d936d0fd93cfd7",
    );
    expect(icon("dock.png")).toBe(
      "07058a43bacdf70a6c23050bfc0832f2047a7c9c64d155778ad4771fe03fbd28",
    );
    expect(icon("android/mipmap-xxxhdpi/ic_launcher.png")).toBe(
      "5529f9013c2a6f42da7acadcc40a962221dc6fce0c51e2591d240d676fc5cc03",
    );
    expect(icon("android/mipmap-xxxhdpi/ic_launcher_round.png")).toBe(
      "10091e404ae306c2ae88e554036f596cb8d62d89d406f3813e804aa1c21f4df9",
    );
    expect(icon("android/mipmap-xxxhdpi/ic_launcher_foreground.png")).toBe(
      "809bdde298fc305142ff711ddcb7fb36dd2833d3d693d3bdcc4724b7a72a171a",
    );
    expect(icon("android/mipmap-xxxhdpi/ic_launcher_background.png")).toBe(
      "6ab1e9e96924314e6f5f30cbbb843e1293a4789dfa5c443aaf2f0bcb2164a2d2",
    );
    expect(icon("android/mipmap-xxxhdpi/ic_launcher_monochrome.png")).toBe(
      "fe61fb7d530db540919c09de87c40c4fa30661500cb9991daa78daadf23669aa",
    );
  });
});
