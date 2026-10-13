// src/views/signalChain/BlockTile.tsx — one block of the signal-chain strip: its
// BlockArt (or a head-over-cab HalfStackArt), the terse caption, the bypass/edit
// styling, and — on the interactive Copy path only — the tap hit-box + edit badge.

import type { CSSProperties } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { plainButton } from "../../theme/tokens";
import { BlockArt, HalfStackArt } from "../../ui/BlockArt";
import { Icon } from "../../ui/Icon";
import { Skel } from "../../ui/Skeleton";
import { STRIP_LBL, type Size, type StripBlock } from "./stripTypes";

export interface BlockTileProps {
  b: StripBlock;
  size: Size;
  skeleton?: boolean;
}

export function BlockTile({ b, size, skeleton }: BlockTileProps) {
  const { t } = useTheme();
  const dims =
    size === "sm" ? { w: 56, art: 46, font: 8 } : { w: 70, art: 58, font: 9 };
  const byp = b.bypassed === true;
  if (skeleton) {
    return (
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          width: dims.w,
          flexShrink: 0,
        }}
      >
        <div
          style={{
            flex: 1,
            minHeight: dims.art,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <Skel w={dims.art} h={dims.art} r={9} />
        </div>
        <div
          style={{
            height: STRIP_LBL,
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "center",
            paddingTop: t.space3,
          }}
        >
          <Skel w={Math.round(dims.w * 0.52)} h={6} r={3} />
        </div>
      </div>
    );
  }
  // Copy-only interactive extensions. When `onClick` is set the tile becomes a
  // button with a rounded hit-box (terracotta when changed/selected) + an edit
  // badge; otherwise it renders exactly as the Level page expects (no box/badge).
  const interactive = b.onClick != null;
  const changed = b.change != null;
  const selected = b.selected === true;
  // The rounded hit-box accent (Copy only) — empty when non-interactive so the Level
  // page's tiles render untouched.
  const boxAccent: CSSProperties = !interactive
    ? {}
    : {
        borderRadius: 9,
        transition: "border-color .12s, background .12s",
        border: selected
          ? `2px solid ${t.accent}`
          : changed
            ? `1.5px solid ${t.stripChangedBorder}`
            : `1px solid ${t.stripGhost}`,
        background: selected
          ? t.accentSoft
          : changed
            ? t.stripChangedSoft
            : "transparent",
      };

  const tileStyle: CSSProperties = {
    display: "flex",
    flexDirection: "column",
    width: dims.w,
    flexShrink: 0,
    opacity: byp ? 0.34 : 1,
  };
  const content = (
    <>
      <div
        style={{
          flex: 1,
          minHeight: dims.art,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div
          style={{
            position: "relative",
            width: dims.art + (interactive ? 10 : 0),
            height: dims.art + (interactive ? 10 : 0),
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            ...boxAccent,
          }}
        >
          <div
            style={{
              position: "relative",
              width: dims.art,
              height: dims.art,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              filter: t.dropShadowStripArt,
            }}
          >
            {b.halfStack ? (
              // Amp that carries its own cab (combo/half-stack): head over cab, so the
              // strip mirrors the unit (Pro Control draws preset 003's HIWAY this way).
              // cabW is scaled so the stacked height ≈ the art box (no row reflow).
              <HalfStackArt
                topIcon={b.halfStack.topIcon}
                topTone={b.halfStack.topTone}
                topVariant={b.halfStack.topVariant}
                cabIcon={b.halfStack.cabIcon}
                cabTone={b.halfStack.cabTone}
                cabW={Math.round(dims.art * 0.62)}
              />
            ) : (
              <BlockArt
                icon={b.icon}
                tone={b.tone}
                lab={b.lab}
                variant={b.variant}
                footswitch={b.footswitch}
                bodyColor={b.body}
                accentColor={b.accent}
                panelColor={b.panel}
                size={dims.art}
                label={false}
              />
            )}
            {byp && (
              <div
                style={{
                  position: "absolute",
                  inset: 0,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                }}
              >
                <div
                  style={{
                    width: "92%",
                    height: 1.6,
                    background: t.ink2,
                    transform: "rotate(-28deg)",
                    borderRadius: 2,
                  }}
                />
              </div>
            )}
          </div>
          {changed && (
            <span
              title={b.change === "replaced" ? "replaced" : "added"}
              style={{
                position: "absolute",
                top: -7,
                right: -5,
                zIndex: 2,
                width: 15,
                height: 15,
                borderRadius: 999,
                background: t.accent,
                border: `1px solid ${t.onAccent}`,
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                boxShadow: t.shadowBadge,
              }}
            >
              <Icon
                name={b.change === "replaced" ? "refresh" : "plus"}
                size={b.change === "replaced" ? 10 : 9}
                stroke={t.onAccent}
                strokeWidth={2.4}
              />
            </span>
          )}
        </div>
      </div>
      <div
        style={{
          height: STRIP_LBL,
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "center",
          paddingTop: t.space2,
        }}
      >
        <span
          style={{
            fontFamily: t.mono,
            fontSize: dims.font,
            letterSpacing: "0.04em",
            color: selected ? t.stripInkSel : t.stripInk,
            lineHeight: 1,
            textTransform: "uppercase",
            whiteSpace: "nowrap",
          }}
        >
          {b.name}
        </span>
      </div>
    </>
  );
  // Interactive (Copy) tiles are real buttons; the Level strip's tiles stay plain divs.
  // e2e hook: `data-block-tile` is a stable selector for an interactive (Copy-path) block
  // tile, carrying the block's MODEL id so a test can match it model-exactly against
  // candidates (the display label differs between the tile and the candidate chip). Absent
  // on the non-interactive Level strip (no behavior/appearance change). Hover (`title`)
  // shows the fuller Pro-Control model name; the visible caption stays terse.
  return b.onClick ? (
    <button
      type="button"
      data-block-tile={b.model ?? b.name}
      title={b.fullName}
      aria-pressed={selected}
      onClick={b.onClick}
      style={plainButton(tileStyle)}
    >
      {content}
    </button>
  ) : (
    <div title={b.fullName} style={tileStyle}>
      {content}
    </div>
  );
}
