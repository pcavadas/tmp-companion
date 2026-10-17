// src/views/CatalogView.tsx — the CATALOG tab (look B).
//
// A read-only, filterable catalog of every model the Tone Master Pro ships. Layout
// (under the App toolbar): a SplitView with the category sidebar (190 px) beside the
// content — the filter bar (search, Sound, Sort by, count; a second line for
// Convolution and Effect type where they apply), the scrolling model wall, and the
// chosen model's details, pinned under the wall (or, in a wide window, beside it).
// Filters compose: e.g. Effects → Stompboxes → Fuzz, or Effects + Stereo. Tiles show a
// model's CPU cost only while the wall is sorted by CPU.
//
// Renders per-model artwork through the shared BlockArt SVG engine (identical to the
// signal-chain strip). Split: the filter state + grouping in ./catalog/useCatalogFilter;
// the sidebar, bar, wall, tile and detail in ./catalog/.

import { useLayoutEffect, useRef } from "react";

import { useTheme } from "../theme/ThemeContext";
import { SplitView } from "../ui/SplitView";
import { CatalogBar } from "./catalog/CatalogBar";
import { CatalogRail } from "./catalog/CatalogRail";
import { DetailBar } from "./catalog/DetailBar";
import { ModelWall } from "./catalog/ModelWall";
import { useCatalogFilter } from "./catalog/useCatalogFilter";
import { useWindowWidth } from "../lib/useWindowWidth";

/** Below this window width the filter bar puts the search on a line of its own. */
const NARROW_BELOW = 880;
/** From this window width the details stand beside the wall. */
const WIDE_FROM = 1280;

const modelCount = (n: number) => (n === 1 ? "1 model" : `${String(n)} models`);

export function CatalogView() {
  const { t } = useTheme();
  const f = useCatalogFilter();
  const width = useWindowWidth();
  const wide = width >= WIDE_FROM;
  const wallRef = useRef<HTMLDivElement>(null);

  // Opening the details shortens the wall; keep the chosen tile in view.
  const pickedCid = f.pick?.cid;
  useLayoutEffect(() => {
    if (pickedCid === undefined) return;
    // The chosen tile is the wall's one pressed button.
    const tile = wallRef.current?.querySelector<HTMLElement>(
      'button[aria-pressed="true"]',
    );
    // jsdom has no scrollIntoView.
    if (typeof tile?.scrollIntoView === "function")
      tile.scrollIntoView({ block: "nearest" });
  }, [pickedCid, wide]);

  const query = f.deferredQ.trim();
  const count = f.filtered
    ? `${String(f.shown.length)} of ${String(f.scopeTotal)}`
    : modelCount(f.shown.length);
  const detail = f.pick && (
    <DetailBar
      r={f.pick}
      layout={wide ? "column" : "bar"}
      onClose={() => {
        f.setPick(null);
      }}
    />
  );

  return (
    <SplitView
      sidebarLabel="Categories"
      sidebarWidth={190}
      sidebar={<CatalogRail sel={f.sel} choose={f.choose} />}
    >
      <CatalogBar
        q={f.q}
        setQ={f.setQ}
        routing={f.routing}
        setRouting={f.setRouting}
        hasStereo={f.hasStereo}
        sort={f.sort}
        setSort={f.setSort}
        cpuDir={f.cpuDir}
        setCpuDir={f.setCpuDir}
        count={count}
        showConv={f.showConv}
        convOnly={f.convOnly}
        setConvOnly={f.setConvOnly}
        sub={f.sel.sub}
        etypes={f.etypes}
        showEt={f.showEt}
        et={f.et}
        setEt={f.setEt}
        narrow={width < NARROW_BELOW}
      />
      <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
        <div
          ref={wallRef}
          style={{
            flex: 1,
            minWidth: 0,
            overflowY: "auto",
            display: "flex",
            flexDirection: "column",
            gap: t.sp12,
            padding: `0 ${String(t.sp24)}px ${String(t.sp16 + 4)}px`,
          }}
        >
          <ModelWall
            groups={f.groups}
            pick={f.pick}
            onPick={f.setPick}
            showCpu={f.sort === "cpu"}
            query={query}
            onClearSearch={() => {
              f.setQ("");
            }}
          />
        </div>
        {wide && detail}
      </div>
      {!wide && detail}
    </SplitView>
  );
}
