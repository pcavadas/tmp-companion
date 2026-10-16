// src/views/level/setupCandidates.ts — pure adapters from the Set up page's fetch states
// to one candidate list (`CandidateFetch`) the Block and Knob dropdowns are built from,
// plus the Boost preselect.

import {
  bestEnabled,
  blockKeyOf,
  groupByBlock,
  groupHead,
  type BlockLevelCandidate,
  type BlockLevelHandle,
} from "./blockLevelGroups";
import type { HandleFetchState } from "./useSceneHandles";
import type { BaseBlockFetchState } from "./useLevelBlocks";
import type { LevelParamCandidate } from "../../lib/types";

/** A sound's leveling candidates as the Set up row reads them: still being read, the
 *  read failed, or the list (possibly empty). */
export type CandidateFetch =
  | { status: "unfetched" | "loading" | "error" }
  | { status: "resolved"; list: BlockLevelCandidate[] };

/** `useLevelBlocks` (main sound rows) → the row's candidate list. */
export function baseCandidatesFetch(
  state: BaseBlockFetchState,
): CandidateFetch {
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

/** Why a scene can't use a knob (the backend refuses that write), as the Knob list says
 *  it. `undefined` for a knob the scene can use. */
function sceneDisabledTitle(
  scope: "isolated" | "shared_with_base" | "unknown",
): string | undefined {
  if (scope === "shared_with_base") return "Shared with the main sound";
  if (scope === "unknown") return "Couldn't confirm it for this scene";
  return undefined;
}

function sceneDisabledFields(
  scope: "isolated" | "shared_with_base" | "unknown",
): { disabled: true; disabledTitle: string } | { disabled?: false } {
  const title = sceneDisabledTitle(scope);
  return title != null ? { disabled: true, disabledTitle: title } : {};
}

/** `list_scene_level_handles`'s `allCandidates` (scene rows) → the row's candidate
 *  list — the full class-annotated superset, level-class first. */
export function sceneCandidatesFetch(state: HandleFetchState): CandidateFetch {
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

/** A footswitch's own level knobs (already in hand from the backup scan) → the row's
 *  candidate list. No knobs at all (a re-run row whose preset the scan never read) is a
 *  read that failed, not an empty preset. */
export function footswitchCandidatesFetch(
  params: readonly LevelParamCandidate[],
): CandidateFetch {
  if (params.length === 0) return { status: "error" };
  return {
    status: "resolved",
    list: params.map((c) => ({
      groupId: c.group_id,
      nodeId: c.node_id,
      fenderId: c.fender_id,
      parameterId: c.parameter_id,
      paramClass: c.class,
    })),
  };
}

/** Issue 5 (Boost preselect): a scene whose overlay un-bypasses a block the base
 *  graph keeps bypassed should default its leveling handle to THAT block's own
 *  control, not the amp's output level. */
export function enablingBlockHandle(
  st: HandleFetchState,
  fetch: CandidateFetch,
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
