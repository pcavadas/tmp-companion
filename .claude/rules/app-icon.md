---
paths:
  - "src-tauri/icons/**"
---

# App icon rules

The mark is a flat terracotta (`#d97757`) macOS-squircle tile with three white bottom-aligned level bars in a 6:11:8 height rhythm.
The bundle is regenerated from the design handoff's two SVG masters, never hand-edited. macOS `icon.icns` and `dock.png` carry a baked full-bleed squircle (superellipse n≈5, transparent corners), because a Tauri-bundled `.icns` renders verbatim and the Dock does not round the icon `dock.rs` sets before macOS 26. iOS art is full-bleed, opaque and mask-free; never re-mask the iOS slots.
Android adaptive icons put the bars inside the safe zone with a `<monochrome>` layer; Windows is square full-bleed.
`bundle.icon` in `tauri.conf.json` lists `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns` and `icon.ico`; `source.png` is the archival master and `tmp-icon-*.svg` are the vector sources.
`src/__tests__/icon-assets.test.ts` pins the shipped assets' sha256 sums: recompute them whenever the bundle changes.
