// src/views/catalog/ModelWall.tsx — the Catalog's model wall (look B): one section per
// group (a sentence-case label, then a grid of tiles, 10 apart), or the no-match state.
// The grid fits as many 200 px columns as the width allows, up to four: three at the
// default 900 window, two at 800, four in a wide window.

import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import { EmptyState } from "../../ui/EmptyState";
import { Group } from "../../ui/List";
import { Button } from "../../ui/primitives";
import type { ModelRecord } from "../../models/catalog";
import { ModelCard } from "./ModelCard";
import type { Group as WallGroup } from "./useCatalogFilter";

/** The tile grid: columns at least 200 wide, never more than four. */
const GRID_COLUMNS =
  "repeat(auto-fill, minmax(max(200px, calc((100% - 30px) / 4)), 1fr))";

export interface ModelWallProps {
  groups: WallGroup[];
  pick: ModelRecord | null;
  onPick: (r: ModelRecord) => void;
  /** The wall is sorted by CPU: tiles show their CPU cost. */
  showCpu: boolean;
  /** The search the wall is filtered by ("" for none). */
  query: string;
  onClearSearch: () => void;
}

export function ModelWall({
  groups,
  pick,
  onPick,
  showCpu,
  query,
  onClearSearch,
}: ModelWallProps) {
  const { t } = useTheme();
  if (groups.every((g) => g.blocks.length === 0)) {
    return (
      <Group>
        {query ? (
          <EmptyState
            icon="search"
            title={`No models match "${query}"`}
            body="Check the spelling, or search by the real amp or pedal it models."
            cta={
              <Button variant="secondary" onClick={onClearSearch}>
                Clear search
              </Button>
            }
          />
        ) : (
          <EmptyState icon="search" title="No models match these filters" />
        )}
      </Group>
    );
  }
  return (
    <>
      {groups.map((g) => (
        <section
          key={g.key}
          aria-label={g.label}
          style={{ display: "flex", flexDirection: "column", gap: t.sp8 }}
        >
          <h2 style={{ ...sectionLabel(t), margin: 0 }}>{g.label}</h2>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: GRID_COLUMNS,
              gap: t.sp8 + 2,
            }}
          >
            {g.blocks.map((r) => (
              <ModelCard
                key={r.cid}
                r={r}
                t={t}
                selected={pick?.cid === r.cid}
                showCpu={showCpu}
                onPick={onPick}
              />
            ))}
          </div>
        </section>
      ))}
    </>
  );
}
