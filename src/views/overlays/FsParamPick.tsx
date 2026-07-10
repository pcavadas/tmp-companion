// src/views/overlays/FsParamPick.tsx — the Set up step's per-footswitch
// "which block parameter to level" picker.
//
// A footswitch can act on several blocks, and a block exposes several levelable
// params. Adjusting a LOUDNESS-only param (level / output / mix…) changes volume
// without touching the sound; gain / tone / drive also change the TONE — which
// leveling tries to avoid. So the default is the first loudness-only candidate, the
// menu marks it "Recommended", and tone-affecting candidates are flagged. With a
// single candidate there is nothing to choose: the control renders read-only (it
// still shows what is being adjusted).
//
// Click-only. The menu reuses the wizard's card-portaled dropdown (PickPortalMenu via
// DialogCardCtx) so it flips ABOVE near the fixed frame's bottom edge — same machinery
// as the sibling instrument/target `Pick`.

import { useContext, useLayoutEffect, useRef, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Tag } from "../../ui/Tag";
import { BlockArt } from "../../ui/BlockArt";
import { blockArtTile } from "../../models/blockArt";
import { defaultParamIndex, isLoudnessParam } from "../level/leveling";
import { DialogCardCtx } from "./wizardContext";
import { PickPortalMenu } from "./PickPortalMenu";
import type { LevelParamCandidate } from "../../lib/types";

export interface FsParamPickProps {
  /** The footswitch's levelable-parameter candidates (the backend `level_params`). */
  params: LevelParamCandidate[];
  /** Selected candidate index. */
  index: number;
  onChange: (i: number) => void;
}

/** Friendly labels for the technical parameter ids (fallback: capitalize the id). */
const PARAM_LABELS: Partial<Record<string, string>> = {
  level: "Level",
  outputLevel: "Output level",
  output: "Output",
  mix: "Mix",
  volume: "Volume",
  gain: "Gain",
  drive: "Drive",
  tone: "Tone",
  fuzz: "Fuzz",
  treble: "Treble",
  bass: "Bass",
  presence: "Presence",
};
function paramLabel(p: string): string {
  return PARAM_LABELS[p] ?? (p ? p.charAt(0).toUpperCase() + p.slice(1) : "");
}

interface Anchor {
  left: number;
  below: number;
  above: number;
  width: number;
  cardW: number;
  cardH: number;
}

export function FsParamPick({ params, index, onChange }: FsParamPickProps) {
  const { t } = useTheme();
  const [open, setOpen] = useState(false);
  const [anchor, setAnchor] = useState<Anchor | null>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  // Portal target captured in `openMenu` (an event handler) so render never reads
  // `ref.current` — mirrors Pick.tsx.
  const [cardEl, setCardEl] = useState<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const cardRef = useContext(DialogCardCtx);

  // Two-pass: render hidden to measure, then place (clamped horizontally, flipped
  // above when it would overflow the card bottom). Identical to Pick.
  useLayoutEffect(() => {
    if (!open || !anchor || !menuRef.current) return;
    const mw = menuRef.current.offsetWidth;
    const mh = menuRef.current.offsetHeight;
    const left = Math.min(
      Math.max(8, anchor.left),
      Math.max(8, anchor.cardW - mw - 8),
    );
    let top = anchor.below;
    if (top + mh > anchor.cardH - 8) top = Math.max(8, anchor.above - mh - 4);
    setPos({ left, top });
  }, [open, anchor]);

  const interactive = params.length > 1;
  const defIdx = defaultParamIndex(params);

  const openMenu = (e: React.MouseEvent) => {
    if (!interactive) return;
    e.stopPropagation();
    const card = cardRef?.current;
    if (!card || !triggerRef.current) return;
    const tr = triggerRef.current.getBoundingClientRect();
    const cr = card.getBoundingClientRect();
    setAnchor({
      left: tr.left - cr.left,
      below: tr.bottom - cr.top + 4,
      above: tr.top - cr.top,
      width: tr.width,
      cardW: cr.width,
      cardH: cr.height,
    });
    setCardEl(card);
    setPos(null);
    setOpen(true);
  };
  const close = () => {
    setOpen(false);
    setPos(null);
  };
  const pick = (i: number) => {
    close();
    onChange(i);
  };

  if (params.length === 0) return null;
  const safeIdx = index >= 0 && index < params.length ? index : 0;
  const cur = params[safeIdx];
  const curArt = blockArtTile(cur.fender_id);

  // Built only while the menu is open — the rows (and their per-candidate
  // `blockArtTile` lookups) are otherwise never rendered.
  const optionRows = open
    ? params.map((c, i) => {
        const on = i === safeIdx;
        const rec = i === defIdx;
        const loud = isLoudnessParam(c.parameter_id);
        const art = blockArtTile(c.fender_id);
        return (
          <div
            key={`${c.node_id}:${c.parameter_id}`}
            onClick={(e) => {
              e.stopPropagation();
              pick(i);
            }}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 11,
              padding: 8,
              borderRadius: 8,
              cursor: "pointer",
              background: on ? t.accentSoft : "transparent",
            }}
            onMouseEnter={(e) => {
              if (!on) e.currentTarget.style.background = t.hover;
            }}
            onMouseLeave={(e) => {
              if (!on) e.currentTarget.style.background = "transparent";
            }}
          >
            <span
              style={{
                width: 38,
                height: 38,
                flexShrink: 0,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              <BlockArt
                icon={art.icon}
                tone={art.tone}
                footswitch={art.footswitch}
                bodyColor={art.body}
                panelColor={art.panel}
                accentColor={art.accent}
                label={false}
                size={34}
              />
            </span>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "baseline", gap: 7 }}>
                <span
                  style={{
                    fontFamily: t.serif,
                    fontSize: 14,
                    color: t.ink,
                    whiteSpace: "nowrap",
                  }}
                >
                  {art.fullName ?? art.name}
                </span>
                <span style={{ color: t.faint }}>·</span>
                <span
                  style={{
                    fontFamily: t.sans,
                    fontSize: 12.5,
                    fontWeight: 500,
                    color: t.ink2,
                    whiteSpace: "nowrap",
                  }}
                >
                  {paramLabel(c.parameter_id)}
                </span>
              </div>
              <div style={{ marginTop: 3 }}>
                {rec ? (
                  <Tag tone="good" uppercase>
                    Recommended · loudness only
                  </Tag>
                ) : loud ? (
                  <span
                    style={{
                      fontFamily: t.sans,
                      fontSize: 10.5,
                      color: t.mutedInk,
                    }}
                  >
                    changes loudness only
                  </span>
                ) : (
                  <span
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 5,
                      fontFamily: t.sans,
                      fontSize: 10.5,
                      color: t.sevWarn,
                    }}
                  >
                    <Icon
                      name="warn-tri"
                      size={10}
                      stroke={t.sevWarn}
                      strokeWidth={1.7}
                    />
                    may change the tone
                  </span>
                )}
              </div>
            </div>
            {on && (
              <span style={{ flexShrink: 0 }}>
                <Icon
                  name="check"
                  size={15}
                  stroke={t.accentDeep}
                  strokeWidth={2}
                />
              </span>
            )}
          </div>
        );
      })
    : [];

  return (
    <div
      ref={triggerRef}
      style={{ position: "relative", width: "100%", minWidth: 0 }}
    >
      <div
        onClick={openMenu}
        title={
          interactive
            ? "Choose which block parameter is leveled"
            : "Only one option — nothing to choose"
        }
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          height: 26,
          padding: "0 8px",
          boxSizing: "border-box",
          border: `0.5px solid ${open ? t.accent : t.hairlineStrong}`,
          borderRadius: 6,
          background: interactive ? t.bg : t.bgAlt,
          cursor: interactive ? "pointer" : "default",
          whiteSpace: "nowrap",
          overflow: "hidden",
        }}
      >
        <span
          style={{
            width: 16,
            height: 16,
            flexShrink: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <BlockArt
            icon={curArt.icon}
            tone={curArt.tone}
            footswitch={curArt.footswitch}
            bodyColor={curArt.body}
            panelColor={curArt.panel}
            accentColor={curArt.accent}
            label={false}
            size={16}
          />
        </span>
        <span
          style={{
            flex: 1,
            minWidth: 0,
            fontFamily: t.sans,
            fontSize: 11,
            color: interactive ? t.ink2 : t.mutedInk,
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {paramLabel(cur.parameter_id)}
        </span>
        {interactive && (
          <Icon
            name="chev-down"
            size={11}
            stroke={open ? t.accentDeep : t.faint}
          />
        )}
      </div>

      {open && anchor && cardEl && (
        <PickPortalMenu
          cardEl={cardEl}
          menuRef={menuRef}
          left={pos ? pos.left : anchor.left}
          top={pos ? pos.top : anchor.below}
          visible={pos != null}
          minWidth={Math.max(anchor.width, 268)}
          onClose={close}
        >
          <div
            style={{
              padding: "4px 8px 8px",
              fontFamily: t.mono,
              fontSize: 9,
              letterSpacing: "0.12em",
              textTransform: "uppercase",
              color: t.faint,
            }}
          >
            Level this footswitch by adjusting
          </div>
          {optionRows}
        </PickPortalMenu>
      )}
    </div>
  );
}

export default FsParamPick;
