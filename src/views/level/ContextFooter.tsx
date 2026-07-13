// src/views/level/ContextFooter.tsx — the Level-view selection-aware footer.
//
// Renders inside the shared ActionBar (the one bottom-bar style across the app — Copy's
// two steps use it too). Two states (Level is the ONLY batch action; the list selects
// scenes via the scene tree, the dialog only configures them):
//   • idle (no selection) — a faint hint on the left + a per-scene-type targets readout
//     on the right (the store's named targets).
//   • selection — `{presetCount} preset(s) · {sceneCount} scene(s) selected` + the
//     primary **Level N preset(s)…** button (once `ready`), else the disabled
//     **"Reading presets…"** pill while the background scene load runs.

import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { Icon } from "../../ui/Icon";
import { ActionBar } from "../../ui/ActionBar";
import { ReadingPill } from "../../ui/ReadingPill";
import type { Store } from "../../lib/types";

export interface ContextFooterProps {
  store: Store | null;
  /** Distinct presets with any selected scene. */
  presetCount: number;
  /** Total selected scene keys (Base counts as a scene). */
  sceneCount: number;
  /** Background scene load settled — gates the Level button vs the pill. */
  ready: boolean;
  onLevel: () => void;
}

export function ContextFooter({
  store,
  presetCount,
  sceneCount,
  ready,
  onLevel,
}: ContextFooterProps) {
  const { t } = useTheme();
  const count = presetCount;

  if (count === 0) {
    const targets = store?.targets ?? [];
    return (
      <ActionBar
        left={
          <span
            style={{ fontFamily: t.mono, fontSize: t.fsMeta, color: t.faint }}
          >
            Select presets to level · click a row to pick it
          </span>
        }
        right={
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMeta,
              color: t.mutedInk,
              display: "inline-flex",
              gap: t.space4,
              alignItems: "center",
            }}
          >
            <Icon name="gauge" size={13} stroke={t.accentDeep} />
            {targets.length === 0
              ? "Targets · none set"
              : `Targets · ${targets.map((tg) => `${tg.name} ${tg.lufs.toFixed(0)}`).join(" · ")}`}
          </span>
        }
      />
    );
  }

  const noun = count === 1 ? "preset" : "presets";
  const sceneNoun = sceneCount === 1 ? "scene" : "scenes";
  return (
    <ActionBar
      left={
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsLabel,
            color: t.ink2,
            whiteSpace: "nowrap",
          }}
        >
          <strong style={{ color: t.ink }}>{count}</strong> {noun}
          <span style={{ color: t.mutedInk }}>
            {" · "}
            <strong style={{ color: t.ink }}>{sceneCount}</strong> {sceneNoun}{" "}
            selected
          </span>
        </span>
      }
      right={
        ready ? (
          <Button variant="primary" small icon="gauge" onClick={onLevel}>
            {`Level ${String(count)} ${noun}…`}
          </Button>
        ) : (
          <ReadingPill />
        )
      }
    />
  );
}

export default ContextFooter;
