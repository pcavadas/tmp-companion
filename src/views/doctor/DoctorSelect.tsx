// src/views/doctor/DoctorSelect.tsx — Doctor's first page: choose the sounds to check.
//
// The shared preset list (PresetList: filter bar, Select all of the presets shown, rows that
// open to their scenes and footswitches, "Loaded" on the preset loaded on the unit) under a
// one-line intro, then the Footer. The list never recalls a preset on the unit, and shows no
// CPU (an empty graph map hides the meter). Footer right: the reason "Check" waits for, or
// "N presets" beside "Check M sounds" — counting sounds, like Level.

import { useTheme } from "../../theme/ThemeContext";
import { Footer } from "../../ui/Footer";
import { Button } from "../../ui/primitives";
import { PresetList } from "../PresetList";
import type { ScanState } from "../level/usePresetData";
import type { PresetRow } from "../level/leveling";
import type { ActiveGraph, FootswitchInfo, SceneInfo } from "../../lib/types";

// CPU-free: PresetList derives each row's CPU from this map; empty → no meter.
const NO_CPU = new Map<number, ActiveGraph>();

/** What Doctor does, above the list. */
const DOCTOR_INTRO =
  "Doctor listens to each sound and tells you what to change on your Tone Master Pro. It changes nothing itself.";

export interface DoctorSelectProps {
  rows: PresetRow[];
  sel: Set<string>;
  pendingWhole: Set<number>;
  expanded: Set<number>;
  ready: boolean;
  /** The preset loaded on the unit ("Loaded"), when known. */
  activeSlot: number | null;
  filter: string;
  loading: boolean;
  scan: ScanState;
  sceneInfo: Map<number, SceneInfo[]>;
  footswitchInfo: Map<number, FootswitchInfo[]>;
  /** Distinct presets with any selected sound. */
  presetCount: number;
  /** Total selected sounds (the main sound counts as a sound). */
  soundCount: number;
  onFilterChange: (s: string) => void;
  onTogglePreset: (slot: number) => void;
  onToggleExpand: (slot: number) => void;
  onToggleKey: (key: string) => void;
  /** Select all: receives the slots of the presets the filter shows. */
  onToggleAll: (slots: readonly number[]) => void;
  /** "Check N sounds". */
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
    activeSlot,
    filter,
    loading,
    scan,
    sceneInfo,
    footswitchInfo,
    presetCount,
    soundCount,
    onFilterChange,
    onTogglePreset,
    onToggleExpand,
    onToggleKey,
    onToggleAll,
    onCheck,
  } = props;

  const caption = {
    fontSize: t.fontCaption,
    color: t.textSecondary,
    whiteSpace: "nowrap",
  } as const;

  let right;
  if (!loading && !ready) {
    right = (
      <>
        <span style={caption}>You can start when reading finishes</span>
        <Button variant="secondary" size="lg" busy>
          Check
        </Button>
      </>
    );
  } else if (loading || soundCount === 0) {
    right = (
      <>
        <span style={caption}>Choose presets or sounds to check</span>
        <Button variant="primary" size="lg" disabled>
          Check
        </Button>
      </>
    );
  } else {
    right = (
      <>
        <span style={caption}>
          {`${String(presetCount)} ${presetCount === 1 ? "preset" : "presets"}`}
        </span>
        <Button variant="primary" size="lg" onClick={onCheck}>
          {`Check ${String(soundCount)} ${soundCount === 1 ? "sound" : "sounds"}`}
        </Button>
      </>
    );
  }

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        flex: 1,
        minHeight: 0,
        background: t.window,
      }}
    >
      <div style={{ flex: 1, minHeight: 0 }}>
        <PresetList
          rows={rows}
          sel={sel}
          pendingWhole={pendingWhole}
          expanded={expanded}
          ready={ready}
          activeSlot={activeSlot}
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
          intro={DOCTOR_INTRO}
        />
      </div>
      <Footer right={right} />
    </div>
  );
}
