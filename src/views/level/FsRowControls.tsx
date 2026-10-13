// src/views/level/FsRowControls.tsx — a footswitch Set up row's leveling controls: the
// handle picker over the scene-context picker, with their warn notes.

import { Pick, type PickOption } from "../overlays/Pick";
import { PickWarnNote } from "../overlays/PickWarnNote";
import { FlatLevelPick, type FlatLevelFetch } from "./FlatLevelPick";
import type { BlockLevelHandle } from "./blockLevelGroups";
import type { FsContextFetchState } from "./useFootswitchSceneContexts";
import type { LevelParamCandidate } from "../../lib/types";

export interface FsRowControlsProps {
  switchIndex: number;
  levelParams: LevelParamCandidate[];
  fsSceneNames: string[];
  handle: BlockLevelHandle | null;
  onHandleChange: (h: BlockLevelHandle | null) => void;
  sceneContext: number | null;
  ctxState: FsContextFetchState;
  /** The suggested scene could not be read and none was chosen: Start waits on a pick. */
  needsChoice: boolean;
  onOpenSceneContext: () => void;
  onSceneContextChange: (v: number | null) => void;
}

/** A footswitch row's leveling controls (D2 + D3): `FlatLevelPick` (no pseudo-option
 *  — every FS row must carry a real handle) over the scene-context picker (which
 *  scene, if any, this switch's sound is measured and solved in). Picking a scene
 *  that doesn't actually turn the switch on is ALLOWED — flagged, never blocked. */
export function FsRowControls({
  switchIndex,
  levelParams,
  fsSceneNames,
  handle,
  onHandleChange,
  sceneContext,
  ctxState,
  needsChoice,
  onOpenSceneContext,
  onSceneContextChange,
}: FsRowControlsProps) {
  const candidates: FlatLevelFetch = {
    status: "resolved",
    list: levelParams.map((c) => ({
      groupId: c.group_id,
      nodeId: c.node_id,
      fenderId: c.fender_id,
      parameterId: c.parameter_id,
      paramClass: c.class,
    })),
  };
  const sceneOptions: PickOption[] = [
    { id: "base", label: "Base" },
    ...fsSceneNames.map((name, i) => ({
      id: String(i),
      label: name || `Scene ${String(i + 1)}`,
    })),
  ];
  const enabling =
    ctxState.status === "resolved"
      ? (ctxState.row?.enablingScenes ?? null)
      : null;
  const nonEnabling =
    sceneContext != null &&
    enabling != null &&
    !enabling.includes(sceneContext);
  return (
    <div
      style={{
        width: 200,
        flexShrink: 0,
        display: "flex",
        flexDirection: "column",
        gap: 6,
      }}
    >
      <FlatLevelPick
        handle={handle}
        onHandleChange={onHandleChange}
        candidates={candidates}
        onOpen={() => undefined}
      />
      <Pick
        grow
        value={sceneContext == null ? "base" : String(sceneContext)}
        options={sceneOptions}
        onChange={(id) => {
          onSceneContextChange(id === "base" ? null : Number(id));
        }}
        onOpen={onOpenSceneContext}
        muted={sceneContext == null}
        tid={`fsctx:${String(switchIndex)}`}
      />
      {needsChoice && (
        <PickWarnNote>
          {`Couldn’t read FS${String(switchIndex + 1)}’s scenes — choose one`}
        </PickWarnNote>
      )}
      {nonEnabling && (
        <PickWarnNote>
          FS{String(switchIndex + 1)} doesn’t turn on in that scene
        </PickWarnNote>
      )}
    </div>
  );
}
