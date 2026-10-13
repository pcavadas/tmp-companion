// src/views/level/setupCandidates.ts — pure adapters from the Set up page's fetch
// states to `FlatLevelPick`'s candidate list, plus the Boost preselect.

import type { FlatLevelFetch } from "./FlatLevelPick";
import {
  bestEnabled,
  blockKeyOf,
  groupByBlock,
  groupHead,
  type BlockLevelHandle,
} from "./blockLevelGroups";
import type { HandleFetchState } from "./useSceneHandles";
import type { BaseBlockFetchState } from "./useLevelBlocks";

/** `useLevelBlocks` (Base rows) → the picker's candidate list. */
export function baseCandidatesFetch(
  state: BaseBlockFetchState,
): FlatLevelFetch {
  if (state.status !== "resolved") return { status: state.status };
  return {
    status: "resolved",
    list: state.blocks.map((b) => ({
      groupId: b.group_id,
      nodeId: b.node_id,
      fenderId: b.model_id,
      parameterId: b.parameter_id,
      paramClass: b.paramClass,
      lowersOnly: b.headroom === "lowers_only",
    })),
  };
}

function sceneDisabledTitle(
  scope: "isolated" | "shared_with_base" | "unknown",
): string | undefined {
  if (scope === "shared_with_base")
    return "shared with the base preset — changes every scene sharing it";
  if (scope === "unknown") return "could not be confirmed for this scene";
  return undefined;
}

function sceneDisabledFields(
  scope: "isolated" | "shared_with_base" | "unknown",
): { disabled: true; disabledTitle: string } | { disabled?: false } {
  const title = sceneDisabledTitle(scope);
  return title != null ? { disabled: true, disabledTitle: title } : {};
}

/** `list_scene_level_handles`'s `allCandidates` (Scene rows) → the picker's
 *  candidate list — the full class-annotated superset, level-class first. */
export function sceneCandidatesFetch(state: HandleFetchState): FlatLevelFetch {
  if (state.status !== "resolved") return { status: state.status };
  return {
    status: "resolved",
    list: state.allCandidates.map((c) => ({
      groupId: c.groupId,
      nodeId: c.nodeId,
      fenderId: c.fenderId,
      parameterId: c.parameterId,
      paramClass: c.class,
      lowersOnly: c.headroom === "lowers_only",
      ...sceneDisabledFields(c.scope),
    })),
  };
}

/** Issue 5 (Boost preselect): a scene whose overlay un-bypasses a block the base
 *  graph keeps bypassed should default its leveling handle to THAT block's own
 *  control, not "Amp output level". */
export function enablingBlockHandle(
  st: HandleFetchState,
  fetch: FlatLevelFetch,
): BlockLevelHandle | undefined {
  if (st.status !== "resolved") return undefined;
  const enablingSource = st.allCandidates.find((c) => c.enablesBlock === true);
  if (!enablingSource) return undefined;
  const key = blockKeyOf(enablingSource);
  if (fetch.status !== "resolved") return undefined;
  const group = groupByBlock(fetch.list).find((g) => {
    const first = groupHead(g);
    return first != null && blockKeyOf(first) === key;
  });
  const best = group ? bestEnabled(group) : undefined;
  return best
    ? {
        groupId: best.groupId,
        nodeId: best.nodeId,
        parameterId: best.parameterId,
      }
    : undefined;
}
