// src/views/level/PresetRow.tsx — one preset row on the Level (and Doctor) list, look B.
//
// A preset is a PARENT of its sounds: the main sound, each scene and each footswitch. One
// 36 px Row, left to right: the Checkbox ("Choose <name>") that chooses or unchooses the
// whole preset (its state is DERIVED from the children — on when all are, mixed when some
// are); the Caret that opens the sub-rows (only on a preset with scenes or footswitches,
// once the backup scan has read them — an 18 px blank otherwise); the slot; the name; a
// caption ("3 scenes, 1 footswitch", "2 of 6 chosen", "Reading…"); and the CPU meter.
//
// The NAME is a real button. On a preset with scenes it opens or closes the preset, like
// the Caret; on a preset without scenes (footswitches only, or nothing) it chooses it, like
// the Checkbox. While the scan reads (children unknown) it chooses the whole preset. A
// chosen (fully or partly) or open preset has the grey `rowSelected` fill. The preset
// loaded on the unit carries a neutral "Loaded" badge after its name. Empty slots show
// "Empty", with no Checkbox or Caret, and take no click. Clicking a row never recalls the
// preset on the unit — recall is owned by Pro Control and the footswitches.

import { memo } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { plainButton } from "../../theme/tokens";
import { Checkbox } from "../../ui/primitives";
import { Badge } from "../../ui/Badge";
import { Caret, Row } from "../../ui/List";
import { SlotLabel } from "../../ui/SlotLabel";
import {
  baseKey,
  footswitchLevelable,
  footswitchName,
  fswKey,
  sceneKeyOf,
  type PresetRow as PresetRowData,
} from "./leveling";
import { presetChoice, presetKeys } from "./presetChoice";
import { SceneRow } from "./SceneRow";
import { presetRowPropsEqual } from "./presetRowEqual";
import { RowCpu } from "./RowCpu";
import type { FootswitchInfo, SceneInfo } from "../../lib/types";

/** The caret's width; a preset with nothing to open keeps the same blank. */
const CARET_W = 18;
/** The checkbox's width; an empty slot keeps the same blank so its slot lines up. */
const CHECK_W = 14;

export interface PresetRowProps {
  row: PresetRowData;
  /** The unit's currently-loaded preset ("Loaded" badge). */
  active: boolean;
  /** Background scene load has settled — releases the caret, the meta and the CPU. */
  ready: boolean;
  /** This preset's scenes from the backup, or undefined while still unknown. */
  scenes: SceneInfo[] | undefined;
  /** This preset's footswitches from the backup (undefined when none/unknown). Each is a
   *  sub-row after the scenes; one with no level control is shown disabled. */
  footswitches: FootswitchInfo[] | undefined;
  /** Whether this preset's sub-rows are open. */
  expanded: boolean;
  /** The full selection set (scene keys). Only this row's own keys matter to its
   *  memo (`presetRowPropsEqual`, in `presetRowEqual.ts`), so a toggle elsewhere
   *  doesn't re-render it. */
  sel: Set<string>;
  /** Ticked WHOLE during the load (scenes not yet known) — drives the checkbox then. */
  pendingWhole: boolean;
  /** Real per-preset CPU % (from the startup backup graph); null hides the meter. */
  cpu: number | null;
  /** The three toggles must be STABLE (`usePresetData`'s are `useCallback([])`) —
   *  a fresh function per render defeats the row's `memo`. */
  onTogglePreset: (slot: number) => void;
  onToggleExpand: (slot: number) => void;
  onToggleKey: (key: string) => void;
}

export const PresetRow = memo(function PresetRow({
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
}: PresetRowProps) {
  const { t } = useTheme();

  if (row.empty) {
    return (
      <Row>
        <span aria-hidden style={{ width: CHECK_W, flexShrink: 0 }} />
        <span aria-hidden style={{ width: CARET_W, flexShrink: 0 }} />
        <SlotLabel
          index={row.slot}
          faint
          style={{ width: 30, flexShrink: 0 }}
        />
        <span style={{ color: t.textDisabled }}>Empty</span>
      </Row>
    );
  }

  const scenesArr = scenes ?? [];
  const fswArr = footswitches ?? [];
  const hasScenes = scenesArr.length > 0;
  // Scenes and footswitches both hang under the caret — either one makes the preset a
  // parent with sub-rows to open.
  const hasChildren = hasScenes || fswArr.length > 0;
  const canOpen = hasChildren && ready;

  const keys = presetKeys(row.slot, scenes, footswitches, ready);
  const choice = presetChoice(row.slot, keys, sel, pendingWhole);
  const selCount = keys ? keys.filter((k) => sel.has(k)).length : 0;

  // The caption: "Reading…" while the scan reads, the chosen count on a partly chosen
  // preset, else what it holds ("3 scenes, 1 footswitch"), or nothing.
  let meta: string | null = null;
  if (!ready) {
    meta = "Reading…";
  } else if (choice === "some" && keys) {
    meta = `${String(selCount)} of ${String(keys.length)} chosen`;
  } else if (hasChildren) {
    const parts: string[] = [];
    if (hasScenes)
      parts.push(
        `${String(scenesArr.length)} scene${scenesArr.length === 1 ? "" : "s"}`,
      );
    if (fswArr.length > 0)
      parts.push(
        `${String(fswArr.length)} footswitch${fswArr.length === 1 ? "" : "es"}`,
      );
    meta = parts.join(", ");
  }

  // The name opens a preset with scenes, and chooses any other preset (and every preset
  // while the scan still reads its children).
  const nameOpens = hasScenes && ready;
  const onName = () => {
    if (nameOpens) onToggleExpand(row.slot);
    else onTogglePreset(row.slot);
  };
  const open = expanded && canOpen;

  return (
    <>
      <Row selected={choice !== "off" || open}>
        <Checkbox
          checked={choice === "on"}
          indeterminate={choice === "some"}
          onChange={() => {
            onTogglePreset(row.slot);
          }}
          label={`Choose ${row.name}`}
          labelHidden
        />
        {canOpen ? (
          <Caret
            open={open}
            name={row.name}
            onToggle={() => {
              onToggleExpand(row.slot);
            }}
          />
        ) : (
          <span aria-hidden style={{ width: CARET_W, flexShrink: 0 }} />
        )}
        <SlotLabel index={row.slot} style={{ width: 30, flexShrink: 0 }} />
        <button
          type="button"
          aria-expanded={nameOpens ? open : undefined}
          onClick={onName}
          style={plainButton({
            flex: 1,
            minWidth: 0,
            height: "100%",
            display: "flex",
            alignItems: "center",
            gap: t.sp8,
            textAlign: "left",
            fontFamily: t.fontSans,
            fontSize: t.fontBody,
            fontWeight: t.weightMedium,
            color: t.text,
          })}
        >
          <span
            style={{
              minWidth: 0,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {row.name}
          </span>
          {active && <Badge>Loaded</Badge>}
        </button>
        {meta != null && (
          <span
            style={{
              fontSize: t.fontCaption,
              color: t.textSecondary,
              whiteSpace: "nowrap",
              flexShrink: 0,
            }}
          >
            {meta}
          </span>
        )}
        {ready && cpu != null && <RowCpu value={cpu} />}
      </Row>

      {open && (
        <>
          <SceneRow
            name="Main sound"
            preset={row.name}
            selected={sel.has(baseKey(row.slot))}
            onToggle={() => {
              onToggleKey(baseKey(row.slot));
            }}
          />
          {scenesArr.map((sc, i) => (
            <SceneRow
              key={`s${String(i)}`}
              name={sc.name}
              preset={row.name}
              selected={sel.has(sceneKeyOf(row.slot, i))}
              onToggle={() => {
                onToggleKey(sceneKeyOf(row.slot, i));
              }}
            />
          ))}
          {fswArr.map((f, i) => {
            // BUG 1: a footswitch with no level-class parameter (e.g. "PHASER" →
            // ACD_PhaserP90) still gets a row — disabled, with a reason — never
            // silently omitted. `footswitchLevelable` is the SAME predicate
            // `childKeys` uses to exclude it from the selectable/counted set, so the
            // two can't disagree again.
            const levelable = footswitchLevelable(f);
            return (
              <SceneRow
                key={`f${String(i)}`}
                name={footswitchName(f)}
                preset={row.name}
                caption={
                  levelable
                    ? `footswitch ${String(f.switch + 1)}`
                    : "No volume knob to level"
                }
                selected={sel.has(fswKey(row.slot, i))}
                onToggle={() => {
                  onToggleKey(fswKey(row.slot, i));
                }}
                disabled={!levelable}
              />
            );
          })}
        </>
      )}
    </>
  );
}, presetRowPropsEqual);
