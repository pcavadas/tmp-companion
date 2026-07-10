// src/views/PresetList.tsx — the Presets-view list shell (scene-tree selection).
//
// Filter row → background scene-scan strip → select-all header → scrollable rows.
// Each PRESET row is a parent of its scenes: the caret reveals Base + footswitch
// scenes (each independently selectable); the row checkbox selects/clears the WHOLE
// preset; clicking the row body toggles its selection (NOT a recall — app-driven recall
// was removed; see PresetRow). Selection is a flat
// set of scene KEYS owned by usePresetData.
//
// Two-phase load: rows paint instantly from the snapshot; each preset's scenes (and
// its expand caret) come alive when the background backup read completes (`ready`).
// While the scan runs the strip shows the determinate transfer %, carets are inert,
// and whole-preset ticks are still allowed.

import { useMemo } from "react";

import { slotLabel } from "../lib/format";
import { useTheme } from "../theme/ThemeContext";
import { Spinner } from "../ui/Spinner";
import { Skel, SkelStatus } from "../ui/Skeleton";
import { ProgressBar } from "../ui/ProgressBar";
import { Checkbox, SearchInput } from "../ui/primitives";
import { PresetRow as PresetRowComponent } from "./level/PresetRow";
import { childKeys } from "./level/leveling";
import { presetCpu } from "../models/cpu";
import type { ScanState } from "./level/usePresetData";
import type { ActiveGraph, FootswitchInfo, SceneInfo } from "../lib/types";

export interface PresetRow {
  slot: number;
  name: string;
  empty: boolean;
}

export interface PresetListProps {
  rows: PresetRow[];
  /** Selected scene keys (Base = `p${slot}`, FS = `s${slot}:${i}`). */
  sel: Set<string>;
  /** Presets ticked whole while their scenes were still unknown (mid-load). */
  pendingWhole: Set<number>;
  /** Expanded preset slots (drawer open). */
  expanded: Set<number>;
  /** Background scene load settled — releases the caret + the per-row meta. */
  ready: boolean;
  activeSlot: number | null; // the amp's currently-active slot (may be null/unknown)
  filter: string;
  /** The preset list is still arriving — ghost the rows in place. */
  loading?: boolean;
  /** Background scene-detail scan state (drives the strip). */
  scan: ScanState;
  /** Per-preset scenes from the backup read, keyed by 0-based list index. */
  sceneInfo: Map<number, SceneInfo[]>;
  /** Per-preset levelable footswitches from the same backup, keyed by 0-based index. */
  footswitchInfo: Map<number, FootswitchInfo[]>;
  /** Per-preset signal graph from the startup backup, keyed by 0-based list index —
   *  drives each row's real CPU readout (CPU is roster-based, so the backup graph
   *  yields the same value as the live hero graph for the active preset too). */
  graphByIndex: Map<number, ActiveGraph>;
  onFilterChange: (s: string) => void;
  onTogglePreset: (slot: number) => void;
  onToggleExpand: (slot: number) => void;
  onToggleKey: (key: string) => void;
  onToggleAll: () => void;
  /** Checkbox tooltip verb passed to each row (Level "…to level" default, Doctor
   *  "…to check"). */
  selectTitle?: string;
}

const COLUMNS = "34px 26px 52px 1fr";

// Per-row name-bar widths, cycled so the ghost list doesn't read as a uniform
// block (mirrors the real list's varied preset-name lengths).
const PRESET_SKEL_W = [
  128, 156, 102, 174, 116, 138, 92, 162, 124, 110, 148, 96,
];

// Ghost preset rows on the EXACT PresetRow grid so placeholders become real rows.
function PresetRowsSkeleton({ rows = 10 }: { rows?: number }) {
  const { t } = useTheme();
  return (
    <div>
      {Array.from({ length: rows }).map((_, i) => (
        <div
          key={i}
          style={{
            display: "grid",
            gridTemplateColumns: COLUMNS,
            alignItems: "center",
            height: 44,
            padding: "0 16px 0 14px",
            borderBottom: `0.5px solid ${t.hairline}`,
            borderLeft: "2px solid transparent",
          }}
        >
          <div style={{ display: "flex", alignItems: "center" }}>
            <Skel w={14} h={14} r={3} />
          </div>
          <span />
          <Skel w={26} h={9} />
          <Skel w={PRESET_SKEL_W[i % PRESET_SKEL_W.length]} h={11} />
        </div>
      ))}
    </div>
  );
}

export function PresetList(props: PresetListProps) {
  const {
    rows,
    sel,
    pendingWhole,
    expanded,
    ready,
    activeSlot,
    filter,
    loading = false,
    scan,
    sceneInfo,
    footswitchInfo,
    graphByIndex,
    onFilterChange,
    onTogglePreset,
    onToggleExpand,
    onToggleKey,
    onToggleAll,
    selectTitle,
  } = props;
  const { t } = useTheme();

  // Memoized: with ~500 presets this would otherwise re-filter the whole list on
  // every render, including every keystroke into the filter input.
  const filteredRows = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return needle
      ? rows.filter(
          (r) =>
            r.name.toLowerCase().includes(needle) ||
            slotLabel(r.slot).includes(needle),
        )
      : rows;
  }, [rows, filter]);

  const nonEmpty = useMemo(() => rows.filter((r) => !r.empty), [rows]);

  // Select-all header state — over every scene key of every non-empty preset (an
  // unknown preset contributes just its Base key). `someSel` also counts mid-load
  // whole ticks so the header reflects them before scenes land.
  const { allSel, someSel } = useMemo(() => {
    const everyKey = nonEmpty.flatMap((r) =>
      childKeys(
        r.slot,
        sceneInfo.get(r.slot) ?? [],
        footswitchInfo.get(r.slot) ?? [],
      ),
    );
    return {
      allSel: everyKey.length > 0 && everyKey.every((k) => sel.has(k)),
      someSel: sel.size > 0 || pendingWhole.size > 0,
    };
  }, [nonEmpty, sceneInfo, footswitchInfo, sel, pendingWhole]);

  const shownCount = useMemo(
    () => filteredRows.filter((r) => !r.empty).length,
    [filteredRows],
  );

  // Real per-preset CPU from the startup backup graph, keyed by 0-based slot. Computed
  // ONCE per backup (not per render) so a selection/expand/filter change — which rebuild
  // the row list below — doesn't re-sum every preset's graph. CPU is roster-based, so the
  // active preset's backup graph gives the same value the hero strip shows from its live
  // graph; `presetCpu` returns null when a graph is absent (backup not yet settled).
  const cpuByIndex = useMemo(() => {
    const m = new Map<number, number | null>();
    for (const r of rows) {
      if (!r.empty) m.set(r.slot, presetCpu(graphByIndex.get(r.slot) ?? null));
    }
    return m;
  }, [rows, graphByIndex]);

  // The rendered rows, memoized WITHOUT `scan.percent` — the backup scan emits ~59
  // progress ticks, and rebuilding ~500 rows on each is wasted work. Only a real
  // change (filter / selection / expansion / active row / scenes landing / ready)
  // recomputes the list; a percent tick just repaints the strip.
  const renderedRows = useMemo(
    () =>
      filteredRows.map((r) => {
        const active = activeSlot !== null && r.slot === activeSlot;
        const cpu = r.empty ? null : (cpuByIndex.get(r.slot) ?? null);
        return (
          <PresetRowComponent
            key={r.slot}
            row={r}
            active={active}
            ready={ready}
            scenes={sceneInfo.get(r.slot)}
            footswitches={footswitchInfo.get(r.slot)}
            expanded={expanded.has(r.slot)}
            sel={sel}
            pendingWhole={pendingWhole.has(r.slot)}
            cpu={cpu}
            onTogglePreset={onTogglePreset}
            onToggleExpand={onToggleExpand}
            onToggleKey={onToggleKey}
            selectTitle={selectTitle}
          />
        );
      }),
    [
      filteredRows,
      activeSlot,
      ready,
      sceneInfo,
      footswitchInfo,
      cpuByIndex,
      expanded,
      sel,
      pendingWhole,
      onTogglePreset,
      onToggleExpand,
      onToggleKey,
      selectTitle,
    ],
  );

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        minHeight: 0,
        background: t.bg,
      }}
    >
      {/* Filter row */}
      <div
        style={{
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "7px 14px",
          borderBottom: `0.5px solid ${t.hairline}`,
        }}
      >
        <SearchInput
          value={filter}
          onChange={onFilterChange}
          placeholder="Filter by name or slot…"
          disabled={loading}
          style={{ flex: 1, opacity: loading ? 0.55 : 1 }}
        />
        {loading ? (
          <SkelStatus label="Reading presets…" />
        ) : (
          <span
            style={{
              font: `${String(t.fsData2)}px ${t.mono}`,
              color: t.faint,
              letterSpacing: "0.04em",
            }}
          >
            {shownCount} {shownCount === 1 ? "preset" : "presets"}
          </span>
        )}
      </div>

      {/* Background scene-detail scan strip — only while the backup read runs */}
      {scan.scanning && (
        <div
          style={{
            flexShrink: 0,
            padding: "8px 14px 9px",
            borderBottom: `0.5px solid ${t.hairline}`,
            background: t.accentSoft,
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              marginBottom: 6,
            }}
          >
            <span
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 7,
                fontFamily: t.mono,
                fontSize: t.fsMeta,
                letterSpacing: t.lsMeta,
                color: t.accentDeep,
              }}
            >
              <Spinner size={12} stroke={t.accentDeep} />
              Reading preset details…
            </span>
            <span
              style={{
                fontFamily: t.mono,
                fontSize: t.fsMeta,
                color: t.mutedInk,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {Math.round(scan.percent)}%
            </span>
          </div>
          <ProgressBar percent={scan.percent} height={3} />
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsMeta,
              color: t.mutedInk,
              marginTop: 6,
            }}
          >
            Go ahead and tick the presets you want to level — you can start as
            soon as this finishes.
          </div>
        </div>
      )}

      {/* List header — select-all checkbox + an empty caret column */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: COLUMNS,
          alignItems: "center",
          height: 30,
          padding: "0 16px 0 14px",
          borderBottom: `0.5px solid ${t.hairline}`,
          borderLeft: "2px solid transparent",
          font: `${String(t.fsMicro)}px ${t.mono}`,
          letterSpacing: t.lsLabel,
          textTransform: "uppercase",
          color: t.faint,
          flexShrink: 0,
        }}
      >
        <div
          onClick={onToggleAll}
          title={allSel ? "Clear selection" : "Select all"}
          style={{
            display: "flex",
            alignItems: "center",
            height: "100%",
            cursor: "pointer",
          }}
        >
          <Checkbox checked={allSel} indeterminate={!allSel && someSel} />
        </div>
        <span />
        <span>slot</span>
        <span>preset</span>
      </div>

      {/* Scroll body */}
      <div style={{ flex: 1, minHeight: 0, position: "relative" }}>
        <div style={{ position: "absolute", inset: 0, overflowY: "auto" }}>
          {loading ? <PresetRowsSkeleton rows={10} /> : renderedRows}
        </div>
      </div>
    </div>
  );
}

export default PresetList;
