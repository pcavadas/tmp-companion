// src/views/doctor/DoctorSelect.tsx — the Doctor "select" stage body.
//
// The tab's default body: a "what Doctor does" info row, the shared preset→scene
// tree (PresetList), and a selection-aware footer. Stays MOUNTED under the run
// modal, so the check reads as an overlay. The list is READ-ONLY (clicking a row
// selects it, never recalls it on the unit); it shows a scene summary / "N of M
// selected" on the right, never CPU — an empty graph map suppresses the readout
// with no PresetList change.

import { useTheme } from "../../theme/ThemeContext";
import { ActionBar } from "../../ui/ActionBar";
import { Button } from "../../ui/primitives";
import { Icon } from "../../ui/Icon";
import { ReadingPill } from "../../ui/ReadingPill";
import { PresetList } from "../PresetList";
import type { ScanState } from "../level/usePresetData";
import type { PresetRow } from "../PresetList";
import type { ActiveGraph, FootswitchInfo, SceneInfo } from "../../lib/types";

// CPU-free: PresetList derives each row's CPU from this map; empty → null → no
// readout, so the right side shows only the scene summary / selection count.
const NO_CPU = new Map<number, ActiveGraph>();

export interface DoctorSelectProps {
  rows: PresetRow[];
  sel: Set<string>;
  pendingWhole: Set<number>;
  expanded: Set<number>;
  ready: boolean;
  filter: string;
  loading: boolean;
  scan: ScanState;
  sceneInfo: Map<number, SceneInfo[]>;
  footswitchInfo: Map<number, FootswitchInfo[]>;
  /** Distinct presets with any selected sound. */
  presetCount: number;
  /** Total selected sounds (Base counts as a sound). */
  sceneCount: number;
  onFilterChange: (s: string) => void;
  onTogglePreset: (slot: number) => void;
  onToggleExpand: (slot: number) => void;
  onToggleKey: (key: string) => void;
  onToggleAll: () => void;
  /** Select → Set up. */
  onCheck: () => void;
}

export function DoctorSelect(props: DoctorSelectProps) {
  const { t } = useTheme();
  const {
    rows,
    sel,
    pendingWhole,
    expanded,
    ready,
    filter,
    loading,
    scan,
    sceneInfo,
    footswitchInfo,
    presetCount,
    sceneCount,
    onFilterChange,
    onTogglePreset,
    onToggleExpand,
    onToggleKey,
    onToggleAll,
    onCheck,
  } = props;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        flex: 1,
        minHeight: 0,
        position: "relative",
      }}
    >
      {/* what Doctor does */}
      <div
        style={{
          flexShrink: 0,
          display: "flex",
          alignItems: "flex-start",
          gap: 10,
          padding: "11px 16px",
          borderBottom: `0.5px solid ${t.hairline}`,
        }}
      >
        <span style={{ display: "inline-flex", paddingTop: 1, flexShrink: 0 }}>
          <Icon name="wave" size={15} stroke={t.accentDeep} />
        </span>
        <span
          style={{
            fontFamily: t.sans,
            fontSize: t.fsBody2,
            lineHeight: 1.5,
            color: t.mutedInk,
          }}
        >
          Doctor listens to each sound and points out tone problems — muddy,
          harsh, lost in the mix — with a one-tap fix for most.
        </span>
      </div>

      <div style={{ flex: 1, minHeight: 0 }}>
        <PresetList
          rows={rows}
          sel={sel}
          pendingWhole={pendingWhole}
          expanded={expanded}
          ready={ready}
          activeSlot={null}
          filter={filter}
          loading={loading}
          scan={scan}
          sceneInfo={sceneInfo}
          footswitchInfo={footswitchInfo}
          graphByIndex={NO_CPU}
          onFilterChange={onFilterChange}
          onTogglePreset={onTogglePreset}
          onToggleExpand={onToggleExpand}
          onToggleKey={onToggleKey}
          onToggleAll={onToggleAll}
          selectTitle="Select preset to check"
        />
      </div>

      <DoctorSelectFooter
        presetCount={presetCount}
        sceneCount={sceneCount}
        ready={ready}
        onCheck={onCheck}
      />
    </div>
  );
}

interface DoctorSelectFooterProps {
  presetCount: number;
  sceneCount: number;
  ready: boolean;
  onCheck: () => void;
}

function DoctorSelectFooter({
  presetCount,
  sceneCount,
  ready,
  onCheck,
}: DoctorSelectFooterProps) {
  const { t } = useTheme();

  if (presetCount === 0) {
    return (
      <ActionBar
        left={
          <span
            style={{ fontFamily: t.mono, fontSize: t.fsMeta, color: t.faint }}
          >
            Tick a preset or scene to check it
          </span>
        }
        right={
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMeta,
              color: t.mutedInk,
              display: "inline-flex",
              gap: 8,
              alignItems: "center",
            }}
          >
            <Icon name="wave" size={13} stroke={t.accentDeep} />
            Finds muddy · harsh · lost in the mix · and more
          </span>
        }
      />
    );
  }

  const noun = presetCount === 1 ? "preset" : "presets";
  const m = sceneCount;
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
          <strong style={{ color: t.ink }}>{presetCount}</strong> {noun}
          <span style={{ color: t.mutedInk }}>
            {" · "}
            <strong style={{ color: t.ink }}>{m}</strong> sound
            {m === 1 ? "" : "s"} selected
          </span>
        </span>
      }
      right={
        ready ? (
          <Button variant="primary" small icon="wave" onClick={onCheck}>
            {`Check ${String(m)} sound${m === 1 ? "" : "s"}…`}
          </Button>
        ) : (
          <ReadingPill />
        )
      }
    />
  );
}

export default DoctorSelect;
