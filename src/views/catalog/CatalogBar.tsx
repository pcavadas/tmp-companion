// src/views/catalog/CatalogBar.tsx — the Catalog's filter bar (look B): the search, the
// Sound choice (All / Mono / Stereo), "Sort by" Type or CPU (and, while sorting by CPU,
// the Highest first / Lowest first button), then the count at the right. A second line
// shows only where it applies: "Convolution only" (Effects and Effects › Reverb) and
// the Effect type dropdown (an Effects subcategory with more than one type).
//
// In a narrow window (800 wide) the search takes the first line on its own and the rest
// moves to a second line.

import { useTheme } from "../../theme/ThemeContext";
import {
  Button,
  Checkbox,
  SearchInput,
  SegmentedControl,
} from "../../ui/primitives";
import { Select } from "../../ui/Select";
import { allOfSub, sentenceCase } from "./catalogLabels";
import type { CpuDir, Routing, Sort } from "./useCatalogFilter";

/** The Effect type dropdown's "All …" choice (no effect type is named this). */
const ALL_TYPES = "*";

/** The search box's width when it shares the line with the other controls, and the
 *  least it gives up to them (the CPU direction button needs the room at 900 wide). */
const SEARCH_W = 260;
const SEARCH_MIN_W = 120;

export interface CatalogBarProps {
  q: string;
  setQ: (q: string) => void;
  routing: Routing;
  setRouting: (r: Routing) => void;
  /** The chosen scope has a stereo model (else Stereo is greyed while it's off). */
  hasStereo: boolean;
  sort: Sort;
  setSort: (s: Sort) => void;
  cpuDir: CpuDir;
  setCpuDir: (d: CpuDir) => void;
  /** "{n} models", or "{n} of {models in the scope}" once a filter is on. */
  count: string;
  showConv: boolean;
  convOnly: boolean;
  setConvOnly: (on: boolean) => void;
  /** The chosen Effects subcategory, when its types get a dropdown. */
  sub: string | null;
  etypes: string[];
  showEt: boolean;
  et: string | null;
  setEt: (et: string | null) => void;
  /** The search takes a line of its own. */
  narrow: boolean;
}

export function CatalogBar({
  q,
  setQ,
  routing,
  setRouting,
  hasStereo,
  sort,
  setSort,
  cpuDir,
  setCpuDir,
  count,
  showConv,
  convOnly,
  setConvOnly,
  sub,
  etypes,
  showEt,
  et,
  setEt,
  narrow,
}: CatalogBarProps) {
  const { t } = useTheme();
  const caption = {
    fontSize: t.fontCaption,
    lineHeight: `${String(t.lineCaption)}px`,
    color: t.textSecondary,
    whiteSpace: "nowrap" as const,
  };

  const search = (
    <SearchInput
      value={q}
      onChange={setQ}
      placeholder="Search a model, the unit it models, or a brand"
      clearable
      style={
        narrow
          ? { flex: 1, minWidth: 0 }
          : { flex: `0 1 ${String(SEARCH_W)}px`, minWidth: SEARCH_MIN_W }
      }
    />
  );

  const controls = (
    <>
      <SegmentedControl<Routing>
        ariaLabel="Sound"
        size="sm"
        value={routing}
        onChange={setRouting}
        options={[
          { value: "all", label: "All" },
          { value: "mono", label: "Mono" },
          // Greyed only while off: a scope with no stereo model can't turn it on,
          // but an active Stereo filter must always be changeable (it survives a
          // category change).
          {
            value: "stereo",
            label: "Stereo",
            disabled: !hasStereo && routing !== "stereo",
          },
        ]}
      />
      <span
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.sp6,
          flexShrink: 0,
        }}
      >
        <span aria-hidden style={caption}>
          Sort by
        </span>
        <SegmentedControl<Sort>
          ariaLabel="Sort by"
          size="sm"
          value={sort}
          onChange={setSort}
          options={[
            { value: "type", label: "Type" },
            { value: "cpu", label: "CPU" },
          ]}
        />
        {sort === "cpu" && (
          <Button
            variant="plain"
            size="sm"
            icon={cpuDir === "desc" ? "arrow-down" : "arrow-up"}
            onClick={() => {
              setCpuDir(cpuDir === "desc" ? "asc" : "desc");
            }}
          >
            {cpuDir === "desc" ? "Highest first" : "Lowest first"}
          </Button>
        )}
      </span>
      <span style={{ flex: 1 }} />
      <span style={caption}>{count}</span>
    </>
  );

  // A line wraps rather than overflow, should its controls ever outgrow it. The one
  // full-width line instead keeps everything on it and lets the search give up room
  // (down to SEARCH_MIN_W), so the bar stays one line at 900 wide.
  const line = {
    display: "flex",
    flexWrap: "wrap" as const,
    alignItems: "center",
    gap: `${String(t.sp8)}px ${String(t.sp12)}px`,
    minWidth: 0,
  };
  const oneLine = { ...line, flexWrap: "nowrap" as const };

  return (
    <div
      style={{
        flexShrink: 0,
        display: "flex",
        flexDirection: "column",
        gap: t.sp8,
        padding: `${String(t.sp12 + 2)}px ${String(t.sp24)}px ${String(t.sp8 + 2)}px`,
      }}
    >
      {narrow ? (
        <>
          <div style={line}>{search}</div>
          <div style={line}>{controls}</div>
        </>
      ) : (
        <div style={oneLine}>
          {search}
          {controls}
        </div>
      )}
      {(showConv || showEt) && (
        <div style={line}>
          {showConv && (
            <Checkbox
              label="Convolution only"
              checked={convOnly}
              onChange={setConvOnly}
            />
          )}
          {showEt && sub !== null && (
            <span style={{ display: "flex", alignItems: "center", gap: t.sp6 }}>
              <span aria-hidden style={caption}>
                Effect type
              </span>
              <Select
                label="Effect type"
                value={et ?? ALL_TYPES}
                options={[
                  { value: ALL_TYPES, label: allOfSub(sub) },
                  ...etypes.map((ty) => ({
                    value: ty,
                    label: sentenceCase(ty),
                  })),
                ]}
                onChange={(v) => {
                  setEt(v === ALL_TYPES ? null : v);
                }}
                tid="effect-type"
              />
            </span>
          )}
        </div>
      )}
    </div>
  );
}
