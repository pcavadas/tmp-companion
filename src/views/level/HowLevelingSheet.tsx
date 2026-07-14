// src/views/level/HowLevelingSheet.tsx — the "How leveling works" explainer.
//
// A calm, read-only one-screen sheet opened from the "How it works" button in the Level
// footer status bar. Recreated from the Claude Design handoff (reference/HowLevelingSheet.jsx) with
// this codebase's theme tokens + Icon + Button. No inputs, no persistence, no fetch.
//
// One deliberate copy change from the handoff: the "Two amps in parallel" case SETS
// "Both amps' volume" (not the handoff's "Output level") — the rebalance algorithm
// adjusts both amps' outputLevel (same control as the scene case), then balances them;
// it never touches the preset's master level, and "Output level" collided with the
// whole-preset row.

import { Fragment } from "react";

import { useTheme } from "../../theme/ThemeContext";
import {
  Dialog,
  DialogHeader,
  DialogBody,
  DialogFooter,
} from "../../ui/Dialog";
import { Button } from "../../ui/primitives";
import { Icon } from "../../ui/Icon";
import type { IconName } from "../../ui/Icon";

// The one rhythm every run follows: play → measure → set.
const LEVEL_BEATS: readonly (readonly [IconName, string, string])[] = [
  ["wave", "Plays a test tone", "through your rig"],
  ["gauge", "Measures how loud", "the real output is"],
  ["settings", "Sets one control", "to hit your target"],
];

// The four cases, each naming the single control leveling adjusts.
const LEVEL_CASES: readonly { name: string; sub: string; sets: string }[] = [
  {
    name: "The whole preset",
    sub: "an alternate preset from your list",
    sets: "Output level",
  },
  {
    name: "A scene",
    sub: "an alternate sound on a footswitch",
    sets: "Amp volume",
  },
  {
    name: "A footswitch",
    sub: "a switch that turns a block on or off",
    sets: "A block parameter",
  },
  {
    name: "Two amps in parallel",
    sub: "two amps running at once",
    sets: "Both amps’ volume",
  },
];

function LevelBeatStrip() {
  const { t } = useTheme();
  return (
    <div style={{ display: "flex", alignItems: "stretch", gap: t.space4 }}>
      {LEVEL_BEATS.map(([icon, title, sub], i) => (
        <Fragment key={title}>
          <div
            style={{
              flex: 1,
              display: "flex",
              flexDirection: "column",
              gap: t.space3,
              padding: `${String(t.space5)}px ${String(t.space6)}px`,
              background: t.bgAlt,
              border: `0.5px solid ${t.hairline}`,
              borderRadius: 9,
            }}
          >
            <div
              style={{ display: "flex", alignItems: "center", gap: t.space4 }}
            >
              <span
                style={{
                  width: 22,
                  height: 22,
                  borderRadius: 6,
                  background: t.accentSoft,
                  display: "grid",
                  placeItems: "center",
                  flexShrink: 0,
                }}
              >
                <Icon name={icon} size={13} stroke={t.accentDeep} />
              </span>
              <span
                style={{
                  fontFamily: t.mono,
                  fontSize: 9,
                  letterSpacing: "0.12em",
                  color: t.faint,
                }}
              >
                {i + 1}
              </span>
            </div>
            <span
              style={{
                fontFamily: t.sans,
                fontSize: 13,
                fontWeight: 600,
                color: t.ink,
                lineHeight: 1.2,
              }}
            >
              {title}
            </span>
            <span
              style={{
                fontFamily: t.sans,
                fontSize: 11.5,
                color: t.mutedInk,
                lineHeight: 1.3,
              }}
            >
              {sub}
            </span>
          </div>
          {i < LEVEL_BEATS.length - 1 && (
            <span
              style={{
                alignSelf: "center",
                color: t.faint,
                flexShrink: 0,
                display: "flex",
              }}
            >
              <Icon name="chev-right" size={14} stroke={t.faint} />
            </span>
          )}
        </Fragment>
      ))}
    </div>
  );
}

interface HowLevelingSheetProps {
  onClose: () => void;
}

export function HowLevelingSheet({ onClose }: HowLevelingSheetProps) {
  const { t } = useTheme();

  return (
    <Dialog size="md" onClose={onClose} label="How leveling works">
      <DialogHeader>
        <span
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: t.space4,
          }}
        >
          <span
            style={{
              width: 24,
              height: 24,
              borderRadius: 7,
              background: t.accentSoft,
              display: "grid",
              placeItems: "center",
            }}
          >
            <Icon name="gauge" size={15} stroke={t.accentDeep} />
          </span>
          <span style={{ fontFamily: t.serif, fontSize: 18, color: t.ink }}>
            How leveling works
          </span>
        </span>
        <button
          type="button"
          onClick={onClose}
          title="Close"
          aria-label="Close"
          style={{
            border: "none",
            background: "transparent",
            cursor: "pointer",
            display: "flex",
            padding: t.space2,
            color: t.faint,
          }}
        >
          <Icon name="x" size={16} />
        </button>
      </DialogHeader>

      <DialogBody>
        <div
          style={{
            fontFamily: t.sans,
            fontSize: 13.5,
            lineHeight: 1.5,
            color: t.ink2,
            marginBottom: t.space6,
          }}
        >
          Every level run does the same three things:
        </div>
        <LevelBeatStrip />
        <div
          style={{
            fontFamily: t.sans,
            fontSize: 13.5,
            lineHeight: 1.5,
            color: t.ink2,
            margin: `${String(t.space8)}px 0 ${String(t.space5)}px`,
          }}
        >
          The control it sets depends on what you&rsquo;re leveling:
        </div>
        <div
          style={{ display: "flex", flexDirection: "column", gap: t.space4 }}
        >
          {LEVEL_CASES.map((c) => (
            <div
              key={c.name}
              style={{
                display: "flex",
                alignItems: "center",
                gap: t.space6,
                padding: `${String(t.space5)}px ${String(t.space6)}px`,
                background: t.bgAlt,
                border: `0.5px solid ${t.hairline}`,
                borderRadius: 10,
              }}
            >
              <div style={{ flex: 1, minWidth: 0 }}>
                <div
                  style={{
                    fontFamily: t.serif,
                    fontSize: 15,
                    color: t.ink,
                    lineHeight: 1.2,
                  }}
                >
                  {c.name}
                </div>
                <div
                  style={{
                    fontFamily: t.sans,
                    fontSize: 11.5,
                    color: t.mutedInk,
                    marginTop: t.space1,
                  }}
                >
                  {c.sub}
                </div>
              </div>
              <div style={{ flexShrink: 0, textAlign: "right" }}>
                <div
                  style={{
                    fontFamily: t.mono,
                    fontSize: 8.5,
                    letterSpacing: "0.1em",
                    textTransform: "uppercase",
                    color: t.faint,
                  }}
                >
                  Sets
                </div>
                <div
                  style={{
                    fontFamily: t.sans,
                    fontSize: 12.5,
                    fontWeight: 600,
                    color: t.accentDeep,
                    marginTop: t.space1,
                  }}
                >
                  {c.sets}
                </div>
              </div>
            </div>
          ))}
        </div>
        <div
          style={{
            display: "flex",
            gap: t.space4,
            alignItems: "flex-start",
            padding: `${String(t.space5)}px ${String(t.space6)}px`,
            background: t.warnSoft,
            border: "0.5px solid rgba(167,70,31,0.32)",
            borderRadius: 10,
            marginTop: t.space6,
          }}
        >
          <span
            style={{
              color: t.warn,
              flexShrink: 0,
              display: "flex",
              paddingTop: t.space1,
            }}
          >
            <Icon name="warn-tri" size={15} stroke={t.warn} />
          </span>
          <span
            style={{
              fontFamily: t.sans,
              fontSize: 12.5,
              lineHeight: 1.5,
              color: t.ink2,
              textWrap: "pretty",
            }}
          >
            For two amps, it balances them to equal loudness first, then levels
            the pair. And if your target is out of reach, it sets the loudest it
            can &mdash; and tells you the exact level it reached.
          </span>
        </div>
      </DialogBody>

      <DialogFooter
        start={
          <span
            style={{
              fontFamily: t.mono,
              fontSize: 10,
              letterSpacing: "0.04em",
              color: t.faint,
            }}
          >
            Play &rarr; measure &rarr; set
          </span>
        }
      >
        <Button variant="primary" small onClick={onClose}>
          Got it
        </Button>
      </DialogFooter>
    </Dialog>
  );
}
