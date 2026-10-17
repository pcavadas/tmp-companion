// src/views/PresetList.tsx — the preset list shared by Level and Doctor, look B.
//
// Filter bar (44 high) → a scrolling Group of preset Rows. The filter bar holds the
// "Select all N shown" Checkbox, the SearchInput and, at the right, one caption: the
// preset count ("14 presets", or "3 of 14 shown" while filtering), "Reading presets…"
// while the list loads, or the scan's progress ("Reading preset details" + bar + %).
//
// Each PRESET row is a parent of its sounds (main sound, scenes, footswitches): the Caret
// opens it, the Checkbox chooses the whole preset, and the name opens a preset with scenes
// or chooses any other (see PresetRow). Selection is a flat set of sound KEYS owned by
// usePresetData. Select all acts on the presets the filter SHOWS — hidden presets keep
// their choice — and its mixed state is computed over the shown presets too.
//
// Two-phase load: rows paint as soon as the list arrives; each preset's scenes, caret and
// CPU come alive when the background backup read completes (`ready`). Ticks made while it
// reads choose whole presets. A filter that matches nothing shows an EmptyState with
// "Clear filter" in place of the rows. The filter bar and the list keep a 960 px maximum
// width, centred, on a wide window.

import { useMemo, type ReactNode } from "react";

import { slotLabel } from "../lib/format";
import { useTheme } from "../theme/ThemeContext";
import { Spinner } from "../ui/Spinner";
import { Skel } from "../ui/Skeleton";
import { ProgressBar } from "../ui/ProgressBar";
import { Button, Checkbox, SearchInput } from "../ui/primitives";
import { EmptyState } from "../ui/EmptyState";
import { Group, Row } from "../ui/List";
import { PresetRow as PresetRowComponent } from "./level/PresetRow";
import type { PresetRow } from "./level/leveling";
import { combineChoices, presetChoice, presetKeys } from "./level/presetChoice";
import { presetCpu } from "../models/cpu";
import type { ScanState } from "./level/usePresetData";
import { useLibraryScanPercent } from "../device/libraryScan";
import type { ActiveGraph, FootswitchInfo, SceneInfo } from "../lib/types";

export interface PresetListProps {
  rows: PresetRow[];
  /** Selected sound keys (main sound = `p${slot}`, scene = `s${slot}:${i}`,
   *  footswitch = `f${slot}:${i}`). */
  sel: Set<string>;
  /** Presets ticked whole while their scenes were still unknown (mid-load). */
  pendingWhole: Set<number>;
  /** Open preset slots (sub-rows shown). */
  expanded: Set<number>;
  /** Background scene load settled — releases the caret, the per-row meta and CPU. */
  ready: boolean;
  activeSlot: number | null; // the amp's currently-active slot (may be null/unknown)
  filter: string;
  /** The preset list is still arriving — ghost the rows in place. */
  loading?: boolean;
  /** Background scene-detail scan state (drives the filter bar's progress). */
  scan: ScanState;
  /** Per-preset scenes from the backup read, keyed by 0-based list index. */
  sceneInfo: Map<number, SceneInfo[]>;
  /** Per-preset footswitches from the same backup, keyed by 0-based index. */
  footswitchInfo: Map<number, FootswitchInfo[]>;
  /** Per-preset signal graph from the startup backup, keyed by 0-based list index —
   *  drives each row's real CPU readout (CPU is roster-based, so the backup graph
   *  yields the same value as the live hero graph for the active preset too). */
  graphByIndex: Map<number, ActiveGraph>;
  onFilterChange: (s: string) => void;
  onTogglePreset: (slot: number) => void;
  onToggleExpand: (slot: number) => void;
  onToggleKey: (key: string) => void;
  /** Select all: receives the slots of the presets the filter shows. */
  onToggleAll: (slots: readonly number[]) => void;
  /** One line above the list, scrolling with it (Doctor's "what Doctor does"). */
  intro?: ReactNode;
}

/** The filter bar and list's maximum width on a wide window. */
const LIST_MAX_W = 960;

// Per-row name-bar widths, cycled so the ghost list doesn't read as a uniform
// block (mirrors the real list's varied preset-name lengths).
const PRESET_SKEL_W = [
  128, 156, 102, 174, 116, 138, 92, 162, 124, 110, 148, 96,
];

// Ghost preset rows on the EXACT PresetRow line (Row, checkbox, caret blank, slot, name)
// so placeholders become real rows in place.
function PresetRowsSkeleton({ rows = 10 }: { rows?: number }) {
  return (
    <Group>
      {Array.from({ length: rows }).map((_, i) => (
        <Row key={i}>
          <Skel w={14} h={14} r={4} />
          <span aria-hidden style={{ width: 18, flexShrink: 0 }} />
          <Skel w={22} h={10} style={{ marginRight: 8 }} />
          <Skel w={PRESET_SKEL_W[i % PRESET_SKEL_W.length]} h={11} />
        </Row>
      ))}
    </Group>
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
    intro,
  } = props;
  const { t } = useTheme();

  const needle = filter.trim().toLowerCase();
  // Memoized: with ~500 presets this would otherwise re-filter the whole list on
  // every render, including every keystroke into the filter input.
  const filteredRows = useMemo(
    () =>
      needle
        ? rows.filter(
            (r) =>
              r.name.toLowerCase().includes(needle) ||
              slotLabel(r.slot).includes(needle),
          )
        : rows,
    [rows, needle],
  );

  const total = useMemo(() => rows.filter((r) => !r.empty).length, [rows]);
  // The presets the filter shows (empty slots aren't presets): what Select all acts on.
  const shownSlots = useMemo(
    () => filteredRows.filter((r) => !r.empty).map((r) => r.slot),
    [filteredRows],
  );
  const shownCount = shownSlots.length;
  const noMatch = needle !== "" && shownCount === 0 && !loading;

  // "Select all N shown": on when every shown preset is fully chosen, mixed when some
  // sound of a shown preset is — computed over the shown presets only.
  const allChoice = useMemo(
    () =>
      combineChoices(
        shownSlots.map((slot) =>
          presetChoice(
            slot,
            presetKeys(
              slot,
              sceneInfo.get(slot),
              footswitchInfo.get(slot),
              ready,
            ),
            sel,
            pendingWhole.has(slot),
          ),
        ),
      ),
    [shownSlots, sceneInfo, footswitchInfo, ready, sel, pendingWhole],
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

  // The rendered rows, memoized — only a real change (filter / selection / expansion /
  // active row / scenes landing / ready) recomputes the list. Every per-row prop is
  // either a row-scoped primitive (`active`, `expanded`, `pendingWhole`, `cpu`), a
  // per-slot value that keeps its identity (`row`, `scenes`, `footswitches`) or a
  // stable callback; the list-wide `sel` Set is compared by `PresetRow`'s memo on the
  // row's OWN keys only. So a tick rebuilds this array of cheap elements, but React
  // re-renders just the one row whose selection changed — not all ~500. The backup's
  // ~59 progress ticks never reach this component at all: only the filter bar's
  // ScanProgress reads the percent store.
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
    ],
  );

  const caption = needle
    ? `${String(shownCount)} of ${String(total)} shown`
    : `${String(total)} ${total === 1 ? "preset" : "presets"}`;

  const captionStyle = {
    fontSize: t.fontCaption,
    color: t.textSecondary,
    whiteSpace: "nowrap",
  } as const;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        minHeight: 0,
        fontFamily: t.fontSans,
        color: t.text,
      }}
    >
      {/* Filter bar */}
      <div
        style={{
          flexShrink: 0,
          width: "100%",
          maxWidth: LIST_MAX_W,
          margin: "0 auto",
          boxSizing: "border-box",
          height: 44,
          display: "flex",
          alignItems: "center",
          gap: t.sp8 + t.sp2,
          padding: `0 ${String(t.sp16)}px 0 ${String(t.sp24 + t.sp4)}px`,
        }}
      >
        <Checkbox
          checked={allChoice === "on"}
          indeterminate={allChoice === "some"}
          disabled={loading || shownCount === 0}
          onChange={() => {
            onToggleAll(shownSlots);
          }}
          label={`Select all ${String(shownCount)} shown`}
          labelHidden
        />
        <SearchInput
          value={filter}
          onChange={onFilterChange}
          placeholder="Filter by name or slot"
          disabled={loading}
          style={{ width: 260, flexShrink: 1, minWidth: 120 }}
        />
        <span style={{ flex: 1 }} />
        {loading ? (
          <span
            style={{
              ...captionStyle,
              display: "inline-flex",
              alignItems: "center",
              gap: t.sp8,
            }}
          >
            <Spinner size={12} stroke={t.textSecondary} />
            Reading presets…
          </span>
        ) : scan.scanning ? (
          <ScanProgress />
        ) : (
          <span style={captionStyle}>{caption}</span>
        )}
      </div>

      {/* Scroll body — full width, so the scrollbar sits at the window's edge */}
      <div style={{ flex: 1, minHeight: 0, position: "relative" }}>
        <div
          data-preset-list
          style={{ position: "absolute", inset: 0, overflowY: "auto" }}
        >
          <div
            style={{
              maxWidth: LIST_MAX_W,
              margin: "0 auto",
              boxSizing: "border-box",
              padding: `0 ${String(t.sp16)}px ${String(t.sp16)}px`,
            }}
          >
            {intro != null && (
              <p
                style={{
                  margin: `0 0 ${String(t.sp8 + t.sp2)}px`,
                  fontSize: t.fontBody,
                  lineHeight: `${String(t.lineBody)}px`,
                  color: t.textSecondary,
                }}
              >
                {intro}
              </p>
            )}
            {loading ? (
              <PresetRowsSkeleton rows={10} />
            ) : noMatch ? (
              <Group>
                <EmptyState
                  icon="search"
                  title={`No presets match "${filter.trim()}"`}
                  body="Search by preset name or slot number."
                  cta={
                    <Button
                      variant="secondary"
                      onClick={() => {
                        onFilterChange("");
                      }}
                    >
                      Clear filter
                    </Button>
                  }
                />
              </Group>
            ) : (
              renderedRows.length > 0 && <Group>{renderedRows}</Group>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

// The scan's progress in the filter bar: Spinner + "Reading preset details" + an 80 px
// bar + the percent. Only this subscribes to the progress store, so a tick repaints it
// and nothing else.
function ScanProgress() {
  const { t } = useTheme();
  const percent = useLibraryScanPercent();
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: t.sp8,
        fontSize: t.fontCaption,
        color: t.textSecondary,
        whiteSpace: "nowrap",
      }}
    >
      <Spinner size={12} stroke={t.textSecondary} />
      Reading preset details
      <span style={{ width: 80, marginLeft: t.sp2 }}>
        <ProgressBar percent={percent} />
      </span>
      <span style={{ fontVariantNumeric: "tabular-nums" }}>
        {Math.round(percent)}%
      </span>
    </span>
  );
}
