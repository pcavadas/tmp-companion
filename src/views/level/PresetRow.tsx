// src/views/level/PresetRow.tsx — one preset row (parent of its scenes).
//
// A preset is a PARENT of its scenes: the Base scene + each footswitch scene. The
// caret reveals them; the checkbox selects/clears the WHOLE preset (its state is
// DERIVED from the children — checked when all are on, indeterminate when some are).
// Grid `34px 26px 52px 1fr`, height 44:
//   • checkbox — selects all child scene keys (stops propagation).
//   • caret    — shown when the preset has FS scenes; inert + faint until `ready`.
//   • slot     — zero-padded to 3, muted (faint on empty).
//   • name + ACTIVE badge + right-side meta (loading… / base only / N scenes / K of N).
// Clicking the row body (not the caret) selects the whole preset. Empty slots render
// "—— empty ——" and aren't interactive. Clicking a row does NOT recall it on the
// unit — preset recall is owned by Pro Control / the footswitches, not this list.

import { useTheme } from "../../theme/ThemeContext";
import { Checkbox } from "../../ui/primitives";
import { Icon } from "../../ui/Icon";
import { Tag } from "../../ui/Tag";
import { Spinner } from "../../ui/Spinner";
import { slotLabel } from "../../lib/format";
import {
  baseKey,
  childKeys,
  footswitchName,
  fswKey,
  sceneKeyOf,
} from "./leveling";
import { SceneRow } from "./SceneRow";
import { RowCpu } from "./RowCpu";
import type { PresetRow as PresetRowData } from "../PresetList";
import type { FootswitchInfo, SceneInfo } from "../../lib/types";

const COLUMNS = "34px 26px 52px 1fr";

export interface PresetRowProps {
  row: PresetRowData;
  /** The unit's currently-loaded preset (terracotta left rule + ACTIVE badge). */
  active: boolean;
  /** Background scene load has settled — releases the caret + the real meta. */
  ready: boolean;
  /** This preset's scenes from the backup, or undefined while still unknown. */
  scenes: SceneInfo[] | undefined;
  /** This preset's levelable footswitches from the backup (undefined when none/unknown).
   *  Rendered as sibling rows to the scenes — same accent FS tag, name = the player's
   *  own footswitch label. */
  footswitches: FootswitchInfo[] | undefined;
  /** Whether this preset's drawer is expanded. */
  expanded: boolean;
  /** The full selection set (scene keys). */
  sel: Set<string>;
  /** Ticked WHOLE during the load (scenes not yet known) — drives the checkbox then. */
  pendingWhole: boolean;
  /** Real per-preset CPU % (from the startup backup graph); null hides the readout. */
  cpu: number | null;
  onTogglePreset: (slot: number) => void;
  onToggleExpand: (slot: number) => void;
  onToggleKey: (key: string) => void;
  /** Checkbox tooltip verb — the row is shared across tabs (Level "…to level",
   *  Doctor "…to check"). Defaults to the Level wording. */
  selectTitle?: string;
}

export function PresetRow({
  row,
  active,
  ready,
  scenes,
  footswitches,
  expanded,
  sel,
  pendingWhole,
  cpu,
  onTogglePreset,
  onToggleExpand,
  onToggleKey,
  selectTitle = "Select preset to level",
}: PresetRowProps) {
  const { t } = useTheme();
  const empty = row.empty;
  const scenesArr = scenes ?? [];
  const fswArr = footswitches ?? [];
  const hasFs = !empty && scenesArr.length > 0;
  const hasFootswitches = !empty && fswArr.length > 0;
  // Scenes and footswitches both hang under the caret — either one makes the preset a
  // parent with selectable children.
  const hasChildren = hasFs || hasFootswitches;

  // Checkbox state — derived from the children once scenes are known; during the
  // load it reflects the whole-preset "pending" tick (no indeterminate possible).
  // Scenes drive `known` (footswitches arrive in the SAME backup pass).
  const known = scenes !== undefined;
  const keys = known && !empty ? childKeys(row.slot, scenesArr, fswArr) : [];
  const selCount = keys.reduce((a, k) => a + (sel.has(k) ? 1 : 0), 0);
  const total = keys.length;
  const allOn = known ? total > 0 && selCount === total : pendingWhole;
  const indeterminate = known && selCount > 0 && selCount < total;

  // Right-side meta: while details load → "loading…"; after → a scene summary.
  let meta: React.ReactNode = null;
  if (empty) {
    meta = null;
  } else if (!ready) {
    meta = (
      <span
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 6,
          fontFamily: t.mono,
          fontSize: t.fsMicro,
          color: t.faint,
          whiteSpace: "nowrap",
          flexShrink: 0,
        }}
      >
        <Spinner size={10} stroke={t.faint} />
        loading…
      </span>
    );
  } else if (!hasChildren) {
    meta = null;
  } else if (indeterminate) {
    meta = (
      <span style={metaStyle(t, t.accentDeep)}>
        {selCount} of {total} selected
      </span>
    );
  } else {
    // "N scenes · M footswitches" — only the parts that exist.
    const parts: string[] = [];
    if (hasFs)
      parts.push(
        `${String(scenesArr.length)} scene${scenesArr.length === 1 ? "" : "s"}`,
      );
    if (hasFootswitches)
      parts.push(
        `${String(fswArr.length)} footswitch${fswArr.length === 1 ? "" : "es"}`,
      );
    meta = <span style={metaStyle(t, t.mutedInk)}>{parts.join(" · ")}</span>;
  }

  // Caret — present when the preset has children (scenes and/or footswitches); inert +
  // faint until `ready`.
  const caretActive = hasChildren && ready;
  const caret = hasChildren ? (
    <div
      onClick={(e) => {
        e.stopPropagation();
        if (caretActive) onToggleExpand(row.slot);
      }}
      title={
        caretActive
          ? expanded
            ? "Hide sounds"
            : "Show Base + scenes"
          : "Loading sounds…"
      }
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        height: "100%",
        cursor: caretActive ? "pointer" : "default",
        opacity: caretActive ? 1 : 0.4,
      }}
    >
      <Icon
        name={expanded ? "chev-down" : "chev-right"}
        size={13}
        stroke={caretActive ? t.mutedInk : t.faint}
        strokeWidth={2}
      />
    </div>
  ) : (
    <div />
  );

  return (
    <>
      <div
        data-active={active ? "1" : undefined}
        onClick={() => {
          if (!empty) onTogglePreset(row.slot);
        }}
        style={{
          position: "relative",
          display: "grid",
          gridTemplateColumns: COLUMNS,
          alignItems: "center",
          height: 44,
          padding: "0 16px 0 14px",
          borderBottom: `0.5px solid ${t.hairline}`,
          background: allOn || indeterminate ? t.rowSel : "transparent",
          borderLeft: active
            ? `2px solid ${t.accent}`
            : "2px solid transparent",
          opacity: empty ? 0.5 : 1,
          cursor: empty ? "default" : "pointer",
        }}
      >
        <div
          onClick={(e) => {
            e.stopPropagation();
            if (!empty) onTogglePreset(row.slot);
          }}
          title={selectTitle}
          style={{
            display: "flex",
            alignItems: "center",
            height: "100%",
            cursor: empty ? "default" : "pointer",
          }}
        >
          {!empty && <Checkbox checked={allOn} indeterminate={indeterminate} />}
        </div>

        {caret}

        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsData,
            color: empty ? t.faint : t.mutedInk,
          }}
        >
          {slotLabel(row.slot)}
        </span>

        <span
          style={{ display: "flex", alignItems: "center", gap: 9, minWidth: 0 }}
        >
          <span
            style={{
              fontFamily: empty ? t.mono : t.serif,
              fontSize: empty ? t.fsUi : t.fsName,
              color: empty ? t.faint : t.ink,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
              flexShrink: 1,
              minWidth: 0,
            }}
          >
            {empty ? "—— empty ——" : row.name}
          </span>
          {active && <Tag tone="good">ACTIVE</Tag>}
          <span style={{ flex: 1 }} />
          {meta}
          {/* Real per-preset CPU bar — base rows only, once the backup graph is ready.
              The 18px gap keeps the scene-summary meta off the bar. */}
          {!empty && ready && cpu != null && (
            <span
              style={{
                display: "inline-flex",
                marginLeft: meta ? 18 : 0,
                flexShrink: 0,
              }}
            >
              <RowCpu value={cpu} />
            </span>
          )}
        </span>
      </div>

      {expanded && caretActive && (
        <>
          <SceneRow
            kind="base"
            tag="BASE"
            name="Base"
            sub="main preset sound"
            selected={sel.has(baseKey(row.slot))}
            onToggle={() => {
              onToggleKey(baseKey(row.slot));
            }}
          />
          {scenesArr.map((sc, i) => (
            <SceneRow
              key={`s${String(i)}`}
              kind="fs"
              tag={sc.fs != null ? `FS${String(sc.fs)}` : "—"}
              name={sc.name}
              sub="footswitch scene"
              selected={sel.has(sceneKeyOf(row.slot, i))}
              onToggle={() => {
                onToggleKey(sceneKeyOf(row.slot, i));
              }}
            />
          ))}
          {fswArr.map((f, i) => (
            <SceneRow
              key={`f${String(i)}`}
              kind="fs"
              tag={`FS${String(f.switch + 1)}`}
              name={footswitchName(f)}
              sub="footswitch"
              selected={sel.has(fswKey(row.slot, i))}
              onToggle={() => {
                onToggleKey(fswKey(row.slot, i));
              }}
            />
          ))}
        </>
      )}
    </>
  );
}

function metaStyle(
  t: ReturnType<typeof useTheme>["t"],
  color: string,
): React.CSSProperties {
  return {
    fontFamily: t.mono,
    fontSize: t.fsMicro,
    letterSpacing: t.lsMeta,
    color,
    whiteSpace: "nowrap",
    flexShrink: 0,
  };
}

export default PresetRow;
