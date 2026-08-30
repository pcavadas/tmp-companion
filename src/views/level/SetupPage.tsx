// src/views/level/SetupPage.tsx — the leveling wizard's "Set up" stage (design
// handoff 1a: full window, one row per preset). Replaces `overlays/SetupBody.tsx` +
// `overlays/LevelSetupPage.tsx`.
//
// A bulk row (Guitar / How loud) writes every sound at once — no ticking; what gets
// leveled was already chosen on the preset list, so a second selection model here
// would be redundant. Each sound also carries its OWN Guitar / Target / Knob pickers
// (and, for footswitches, the D3 scene-context picker), so any single row can be
// overridden on top of the bulk write. The list groups by PRESET (`PresetGroupRow`),
// auto-opening the first preset so the per-sound controls are discoverable
// (`useGroupOpen`) — the user can expand/collapse any other row on top of that.
//
// The leveling HANDLE (D2 — every row levels against one user-chosen block+param
// control) keeps its full any-block/any-param capability via `FlatLevelPick`
// (Base rows default to "Preset level", Scene rows to "Amp output level", Footswitch
// rows always carry a real pick) — this is the one piece of Set up that ISN'T a
// simplification of the pre-redesign `BlockLevelPick`, just a single-dropdown
// re-render of the same candidate data.
//
// History (do not reintroduce): an earlier build put inclusion checkboxes here,
// forcing users to pick sounds twice (list + dialog) — the list is the single place
// you choose WHAT to level, this step only chooses HOW. A later build gave footswitch
// rows a "Verify only" default + scene rows a match/offset target-mode chip — both
// removed (the backend dropped verify-only entirely; the combined handle picker
// replaced the target-mode concept). The gain-budget redistribution / common-target /
// per-row "Restore original" features are gone from the wizard entirely (design 1a) —
// the backup-acknowledgment checkbox below is the one revert path: restore from your
// Pro Control backup.

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { useTheme, useStyles } from "../../theme/ThemeContext";
import { BackupAckLabel } from "../../ui/BackupAckLabel";
import { Icon } from "../../ui/Icon";
import { Button, Toggle } from "../../ui/primitives";
import { LevelPage } from "./LevelPage";
import { PresetGroupList, PresetGroupRow } from "./PresetGroupRow";
import { useGroupOpen } from "./useGroupOpen";
import { ByEarChip } from "../overlays/ByEarChip";
import { Pick, type PickOption } from "../overlays/Pick";
import { FlatLevelPick, type FlatLevelFetch } from "./FlatLevelPick";
import { PickWarnNote } from "../overlays/PickWarnNote";
import type { BlockLevelHandle } from "./blockLevelGroups";
import { useSceneHandles, type HandleFetchState } from "./useSceneHandles";
import { useLevelBlocks, type BaseBlockFetchState } from "./useLevelBlocks";
import {
  useFootswitchSceneContexts,
  type FsContextFetchState,
} from "./useFootswitchSceneContexts";
import {
  bestEnabled,
  blockKeyOf,
  groupByBlock,
  groupHead,
} from "./blockLevelGroups";
import {
  footswitchNameForCandidate,
  instCalState,
  setupRowHookKey,
  targetFromCandidate,
} from "./leveling";
import type {
  SetupOption,
  SetupChoice,
  BaseHandlePick,
  FootswitchTarget,
} from "./leveling";
import type { LevelParamCandidate } from "../../lib/types";

export type { SetupChoice };

/** The "calibrate" word — an inviting next-step cue. Dotted terracotta underline
 *  that solidifies on hover; clicking jumps to Settings → Instruments. Click-only
 *  app, so no keyboard handler — the click IS the affordance. */
function CalibrateCue({
  children,
  onCalibrate,
}: {
  children: ReactNode;
  onCalibrate?: () => void;
}) {
  const { t } = useTheme();
  const [hover, setHover] = useState(false);
  return (
    <span
      title="Calibrate instruments in Settings"
      onClick={onCalibrate}
      onMouseEnter={() => {
        setHover(true);
      }}
      onMouseLeave={() => {
        setHover(false);
      }}
      style={{
        color: t.accentDeep,
        fontWeight: 500,
        cursor: "pointer",
        textDecoration: "underline",
        textDecorationStyle: "dotted",
        textDecorationColor: hover ? t.accentDeep : t.warnBorder,
        textUnderlineOffset: "2.5px",
      }}
    >
      {children}
    </span>
  );
}

/** Quiet good → better → best caption beneath the bulk instrument picker. `cal`
 *  removes the element entirely so the list below reclaims the space. */
function InstrumentNudge({
  state,
  onCalibrate,
}: {
  state: "none" | "uncal" | "cal";
  onCalibrate?: () => void;
}) {
  const { t } = useTheme();
  if (state === "cal") return null;
  return (
    <div
      aria-live="polite"
      style={{
        marginTop: t.space3,
        fontFamily: t.sans,
        fontSize: 11.5,
        lineHeight: 1.45,
        color: t.mutedInk,
        textWrap: "pretty",
      }}
    >
      {state === "none" ? (
        <span>
          Set an instrument for better results.{" "}
          <CalibrateCue onCalibrate={onCalibrate}>Calibrate</CalibrateCue> it to
          get more accurate levels.
        </span>
      ) : (
        <span>
          <CalibrateCue onCalibrate={onCalibrate}>Calibrate</CalibrateCue> this
          instrument for more accurate levels.
        </span>
      )}
    </div>
  );
}

/** Onboarding nudge toward Tier-2 calibration — shown once per wizard open, only
 *  while the bulk instrument is a real, uncalibrated profile. */
function CalibrationOnboardingBanner({ show }: { show: boolean }) {
  const { t } = useTheme();
  const [dismissed, setDismissed] = useState(false);
  if (!show || dismissed) return null;
  return (
    <div
      role="status"
      style={{
        flexShrink: 0,
        display: "flex",
        alignItems: "flex-start",
        gap: t.space4,
        padding: `${String(t.space4)}px ${String(t.space5)}px`,
        borderRadius: t.rCard,
        border: `0.5px solid ${t.hairlineStrong}`,
        background: t.bgAlt,
      }}
    >
      <span style={{ display: "flex", flexShrink: 0, marginTop: t.space1 }}>
        <Icon name="info" size={14} stroke={t.accentDeep} strokeWidth={1.5} />
      </span>
      <span
        style={{
          flex: 1,
          fontFamily: t.sans,
          fontSize: 12,
          lineHeight: 1.45,
          color: t.ink2,
        }}
      >
        Level with your own guitar. A 2-minute calibration makes leveling match
        your instrument. Find it in Settings → Instruments → Calibrate.
      </span>
      <button
        type="button"
        aria-label="Dismiss"
        title="Dismiss"
        onClick={() => {
          setDismissed(true);
        }}
        style={{
          cursor: "pointer",
          display: "flex",
          flexShrink: 0,
          background: "transparent",
          border: 0,
          padding: 0,
        }}
      >
        <Icon name="x" size={12} stroke={t.mutedInk} />
      </button>
    </div>
  );
}

/** A footswitch row's leveling controls (D2 + D3): `FlatLevelPick` (no pseudo-option
 *  — every FS row must carry a real handle) over the scene-context picker (which
 *  scene, if any, this switch's sound is measured and solved in). Picking a scene
 *  that doesn't actually turn the switch on is ALLOWED — flagged, never blocked. */
function FsRowControls({
  switchIndex,
  levelParams,
  fsSceneNames,
  handle,
  onHandleChange,
  sceneContext,
  ctxState,
  onOpenSceneContext,
  onSceneContextChange,
}: {
  switchIndex: number;
  levelParams: LevelParamCandidate[];
  fsSceneNames: string[];
  handle: BlockLevelHandle | null;
  onHandleChange: (h: BlockLevelHandle | null) => void;
  sceneContext: number | null;
  ctxState: FsContextFetchState;
  onOpenSceneContext: () => void;
  onSceneContextChange: (v: number | null) => void;
}) {
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
      {nonEnabling && (
        <PickWarnNote>
          FS{String(switchIndex + 1)} doesn’t turn on in that scene
        </PickWarnNote>
      )}
    </div>
  );
}

/** `useLevelBlocks` (Base rows) → the picker's candidate list. */
function baseCandidatesFetch(state: BaseBlockFetchState): FlatLevelFetch {
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
function sceneCandidatesFetch(state: HandleFetchState): FlatLevelFetch {
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
function enablingBlockHandle(
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

interface RowChoice {
  inst: string;
  target: string;
  handle: BlockLevelHandle | null | undefined;
  sceneContext: number | null | undefined;
}

export interface SetupPageProps {
  options: SetupOption[];
  isRelevel: boolean;
  instrumentOptions: PickOption[];
  targetOptions: PickOption[];
  defaultInst: string;
  defaultTarget: string;
  onCancel: () => void;
  onStart: (choices: SetupChoice[]) => void;
  onRebalanceChange?: (on: boolean) => void;
  onCalibrate?: () => void;
}

export function SetupPage({
  options,
  isRelevel,
  instrumentOptions,
  targetOptions,
  defaultInst,
  defaultTarget,
  onCancel,
  onStart,
  onRebalanceChange,
  onCalibrate,
}: SetupPageProps) {
  const { t } = useTheme();
  const s = useStyles();
  const requireBackup = !isRelevel;
  const [backedUp, setBackedUp] = useState(false);
  const [rebalance, setRebalance] = useState(false);
  const toggleRebalance = () => {
    const next = !rebalance;
    setRebalance(next);
    onRebalanceChange?.(next);
  };
  const didSyncRebalance = useRef(false);
  useEffect(() => {
    if (didSyncRebalance.current) return;
    didSyncRebalance.current = true;
    onRebalanceChange?.(rebalance);
  }, [onRebalanceChange, rebalance]);

  const groups = useMemo(() => {
    const by = new Map<
      number,
      { slot: number; name: string; opts: SetupOption[] }
    >();
    options.forEach((o) => {
      let group = by.get(o.slot);
      if (!group) {
        group = { slot: o.slot, name: o.presetName, opts: [] };
        by.set(o.slot, group);
      }
      group.opts.push(o);
    });
    return [...by.values()].sort((a, b) => a.slot - b.slot);
  }, [options]);

  const [rows, setRows] = useState<Partial<Record<string, RowChoice>>>(() => {
    const m: Partial<Record<string, RowChoice>> = {};
    options.forEach((o) => {
      m[o.key] = {
        inst: defaultInst,
        target: defaultTarget,
        handle:
          o.footswitch != null
            ? {
                groupId: o.footswitch.levGroupId,
                nodeId: o.footswitch.levNodeId,
                parameterId: o.footswitch.levParameterId,
              }
            : o.isBase
              ? (o.baseHandle ?? null)
              : (o.sceneHandle ?? undefined),
        sceneContext: undefined,
      };
    });
    return m;
  });
  const patchRow = (k: string, partial: Partial<RowChoice>) => {
    setRows((p) => {
      const cur = p[k];
      if (!cur) return p;
      return { ...p, [k]: { ...cur, ...partial } };
    });
  };

  const {
    prefetch: fetchHandlesFor,
    candidatesFor,
    hasBackupData: sceneHasBackup,
  } = useSceneHandles();
  const {
    prefetch: fetchBlocksFor,
    blocksFor,
    hasBackupData: baseHasBackup,
  } = useLevelBlocks();
  const { prefetch: fetchFsContextFor, contextFor } =
    useFootswitchSceneContexts();

  useEffect(() => {
    groups.forEach((g) => {
      if (baseHasBackup(g.slot)) fetchBlocksFor(g.slot);
      if (sceneHasBackup(g.slot)) fetchHandlesFor(g.slot);
    });
  }, [groups, fetchBlocksFor, fetchHandlesFor, baseHasBackup, sceneHasBackup]);

  const effectiveHandle = (
    o: SetupOption,
    row: RowChoice | undefined,
    // Pass this when the caller already built the row's `FlatLevelFetch` (e.g. for
    // the `candidates` prop) — skips re-deriving it from raw fetch state.
    fetch?: FlatLevelFetch,
  ): BlockLevelHandle | null => {
    if (row?.handle !== undefined) return row.handle;
    if (o.footswitch == null && o.sceneSlot != null) {
      const st = candidatesFor(o.slot, o.sceneSlot);
      const preselect = enablingBlockHandle(
        st,
        fetch ?? sceneCandidatesFetch(st),
      );
      if (preselect) return preselect;
    }
    return null;
  };

  const fsSuggestedFor = (o: SetupOption): number | null => {
    if (o.footswitch == null) return null;
    const st = contextFor(o.slot, o.footswitch.switchIndex);
    return st.status === "resolved" ? (st.row?.suggested ?? null) : null;
  };

  // The bulk row's current value — always written to EVERY row (no ticking; what
  // gets leveled was already chosen on the preset list).
  const [bulkInst, setBulkInst] = useState(defaultInst);
  const [bulkTarget, setBulkTarget] = useState(defaultTarget);
  const applyBulkField = (field: "inst" | "target", v: string) => {
    setRows((p) => {
      const n = { ...p };
      options.forEach((o) => {
        const cur = n[o.key];
        if (cur) n[o.key] = { ...cur, [field]: v };
      });
      return n;
    });
  };
  const applyBulkInst = (v: string) => {
    setBulkInst(v);
    applyBulkField("inst", v);
  };
  const applyBulkTarget = (v: string) => {
    setBulkTarget(v);
    applyBulkField("target", v);
  };

  const total = options.length;

  const start = () => {
    const choices: SetupChoice[] = options.map((o) => {
      const row = rows[o.key];
      let option = o;
      if (o.footswitch != null) {
        const chosenHandle = effectiveHandle(o, row);
        const candidate = chosenHandle
          ? (o.levelParams ?? []).find(
              (c) =>
                c.group_id === chosenHandle.groupId &&
                c.node_id === chosenHandle.nodeId &&
                c.parameter_id === chosenHandle.parameterId,
            )
          : undefined;
        const suggested = fsSuggestedFor(o);
        const sceneContext =
          row?.sceneContext !== undefined
            ? row.sceneContext
            : (suggested ?? o.footswitch.sceneContext);
        const target: FootswitchTarget = candidate
          ? targetFromCandidate(
              o.footswitch.switchIndex,
              sceneContext,
              candidate,
            )
          : { ...o.footswitch, sceneContext };
        option = {
          ...o,
          footswitch: target,
          ...(o.fsUnlabeled === true && candidate
            ? { sceneName: footswitchNameForCandidate(candidate) }
            : {}),
        };
      } else if (o.isBase) {
        const chosen = effectiveHandle(o, row);
        const baseHandle: BaseHandlePick | null = chosen
          ? {
              groupId: chosen.groupId,
              nodeId: chosen.nodeId,
              parameterId: chosen.parameterId,
            }
          : null;
        option = { ...o, baseHandle };
      } else if (o.sceneSlot != null) {
        option = { ...o, sceneHandle: effectiveHandle(o, row) };
      }
      return {
        option,
        instId: row?.inst ?? defaultInst,
        targetName: row?.target ?? defaultTarget,
      };
    });
    if (choices.length) onStart(choices);
  };

  const [isOpen, toggle] = useGroupOpen(groups.length ? [groups[0].slot] : []);

  const summarizeField = (
    values: Set<string>,
    fieldOptions: PickOption[],
    mixedLabel: string,
  ): string => {
    if (values.size !== 1) return mixedLabel;
    const [only] = values;
    return fieldOptions.find((x) => x.id === only)?.label ?? only;
  };

  const groupNote = (g: { opts: SetupOption[] }): string => {
    const insts = new Set(g.opts.map((o) => rows[o.key]?.inst ?? defaultInst));
    const targets = new Set(
      g.opts.map((o) => rows[o.key]?.target ?? defaultTarget),
    );
    const instLabel = summarizeField(insts, instrumentOptions, "mixed guitars");
    const targetLabel = summarizeField(targets, targetOptions, "mixed targets");
    return `${instLabel} · ${targetLabel}`;
  };

  return (
    <LevelPage
      step={0}
      title={
        isRelevel ? "Level these again" : "Make all your sounds equally loud"
      }
      sub="We play a short test tone through each sound, listen, and turn its volume up or down until they match. Nothing else about your tone changes."
      footerLeft={
        <Button
          variant="ghost"
          small
          onClick={onCancel}
          style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
        >
          Cancel
        </Button>
      }
      footerRight={
        <>
          {requireBackup && (
            <BackupAckLabel
              checked={backedUp}
              onChange={setBackedUp}
              style={{ userSelect: "none", paddingRight: t.space2 }}
            />
          )}
          <Button
            variant="primary"
            small
            icon="gauge"
            disabled={total === 0 || (requireBackup && !backedUp)}
            onClick={start}
            style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
          >
            {`Start — ${String(total)} sound${total === 1 ? "" : "s"}`}
          </Button>
        </>
      }
    >
      {/* bulk row — always writes every sound; per-row pickers override individually */}
      <div
        style={{
          flexShrink: 0,
          display: "flex",
          alignItems: "flex-start",
          gap: t.space10,
          flexWrap: "wrap",
          padding: `${String(t.space6)}px ${String(t.space7)}px`,
          border: `0.5px solid ${t.hairlineStrong}`,
          borderRadius: t.rCard,
          background: t.bgAlt,
        }}
      >
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: t.space3,
            minWidth: 180,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: t.space5 }}>
            <span
              style={{
                fontFamily: t.serif,
                fontSize: 15,
                color: t.ink,
                whiteSpace: "nowrap",
              }}
            >
              Guitar
            </span>
            <div style={{ width: 168 }}>
              <Pick
                value={bulkInst}
                options={instrumentOptions}
                onChange={applyBulkInst}
              />
            </div>
          </div>
          <InstrumentNudge
            state={instCalState(bulkInst, instrumentOptions)}
            onCalibrate={onCalibrate}
          />
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: t.space5 }}>
          <span
            style={{
              fontFamily: t.serif,
              fontSize: 15,
              color: t.ink,
              whiteSpace: "nowrap",
            }}
          >
            How loud
          </span>
          <div style={{ width: 168 }}>
            <Pick
              value={bulkTarget}
              options={targetOptions}
              onChange={applyBulkTarget}
            />
          </div>
        </div>
        <span
          style={{
            flex: 1,
            minWidth: 160,
            fontFamily: t.sans,
            fontSize: 11.5,
            lineHeight: 1.45,
            color: t.faint,
            textWrap: "pretty",
          }}
        >
          Sets all {total} at once.
        </span>
      </div>

      <CalibrationOnboardingBanner
        show={instCalState(bulkInst, instrumentOptions) === "uncal"}
      />

      <PresetGroupList
        label={`${String(groups.length)} preset${groups.length === 1 ? "" : "s"} · ${String(total)} sound${total === 1 ? "" : "s"}`}
      >
        {groups.map((g) => (
          <PresetGroupRow
            key={g.slot}
            slot={g.slot}
            name={g.name}
            open={isOpen(g.slot)}
            onToggle={() => {
              toggle(g.slot);
            }}
            tag={
              <span style={{ ...s.kickerWide(t.faint) }}>
                {g.opts.length} sound{g.opts.length === 1 ? "" : "s"}
              </span>
            }
            note={groupNote(g)}
          >
            {g.opts.map((o) => {
              const row = rows[o.key];
              const fsw = o.footswitch ?? null;
              const params = fsw ? (o.levelParams ?? []) : [];
              const sceneFetch =
                o.sceneSlot != null
                  ? sceneCandidatesFetch(candidatesFor(o.slot, o.sceneSlot))
                  : null;
              return (
                <div
                  key={o.key}
                  data-setup-row={setupRowHookKey(o)}
                  style={{
                    display: "flex",
                    alignItems: "flex-start",
                    gap: t.space5,
                    minHeight: 34,
                    padding: `${String(t.space2)}px 0`,
                  }}
                >
                  <span
                    style={{
                      fontFamily: t.sans,
                      fontSize: 13,
                      color: t.ink2,
                      width: 138,
                      flexShrink: 0,
                      whiteSpace: "nowrap",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      paddingTop: t.space2,
                    }}
                  >
                    {o.sceneName}
                  </span>
                  <div style={{ width: 128, flexShrink: 0 }}>
                    <Pick
                      grow
                      value={row?.inst ?? defaultInst}
                      options={instrumentOptions}
                      onChange={(v) => {
                        patchRow(o.key, { inst: v });
                      }}
                    />
                  </div>
                  <div style={{ width: 118, flexShrink: 0 }}>
                    <Pick
                      grow
                      tid={`target:${setupRowHookKey(o)}`}
                      value={row?.target ?? defaultTarget}
                      options={targetOptions}
                      onChange={(v) => {
                        patchRow(o.key, { target: v });
                      }}
                    />
                  </div>
                  <div style={{ width: 200, flexShrink: 0 }}>
                    {fsw && params.length > 0 ? (
                      <FsRowControls
                        switchIndex={fsw.switchIndex}
                        levelParams={params}
                        fsSceneNames={o.fsSceneNames ?? []}
                        handle={effectiveHandle(o, row)}
                        onHandleChange={(h) => {
                          patchRow(o.key, { handle: h });
                        }}
                        sceneContext={
                          row?.sceneContext !== undefined
                            ? row.sceneContext
                            : (fsSuggestedFor(o) ?? fsw.sceneContext)
                        }
                        ctxState={contextFor(o.slot, fsw.switchIndex)}
                        onOpenSceneContext={() => {
                          fetchFsContextFor(o.slot);
                        }}
                        onSceneContextChange={(v) => {
                          patchRow(o.key, { sceneContext: v });
                        }}
                      />
                    ) : o.isBase ? (
                      <FlatLevelPick
                        pseudoLabel="Preset level"
                        handle={effectiveHandle(o, row)}
                        onHandleChange={(h) => {
                          patchRow(o.key, { handle: h });
                        }}
                        candidates={baseCandidatesFetch(blocksFor(o.slot))}
                        onOpen={() => {
                          fetchBlocksFor(o.slot);
                        }}
                      />
                    ) : sceneFetch != null ? (
                      <FlatLevelPick
                        pseudoLabel="Amp output level"
                        handle={effectiveHandle(o, row, sceneFetch)}
                        onHandleChange={(h) => {
                          patchRow(o.key, { handle: h });
                        }}
                        candidates={sceneFetch}
                        onOpen={() => {
                          fetchHandlesFor(o.slot);
                        }}
                      />
                    ) : null}
                  </div>
                </div>
              );
            })}
          </PresetGroupRow>
        ))}
      </PresetGroupList>

      {/* run option — advanced, opt-in, applies to the whole run */}
      <div
        style={{
          flexShrink: 0,
          padding: `${String(t.space5)}px ${String(t.space7)}px`,
          background: t.bgAlt,
          borderTop: `0.5px solid ${t.hairline}`,
          borderRadius: t.rCard,
        }}
      >
        <button
          type="button"
          role="switch"
          aria-checked={rebalance}
          onClick={toggleRebalance}
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: t.space6,
            cursor: "pointer",
            userSelect: "none",
            width: "100%",
            textAlign: "left",
            background: "none",
            border: "none",
            padding: 0,
            font: "inherit",
            color: "inherit",
          }}
        >
          <span aria-hidden style={{ paddingTop: t.space1, flexShrink: 0 }}>
            <Toggle on={rebalance} />
          </span>
          <div style={{ minWidth: 0 }}>
            <div
              style={{
                fontFamily: t.sans,
                fontSize: 13,
                fontWeight: 500,
                color: t.ink,
              }}
            >
              Even out parallel amps
            </div>
            <div
              style={{
                fontFamily: t.sans,
                fontSize: 11,
                lineHeight: 1.5,
                color: t.mutedInk,
                marginTop: t.space1,
                textWrap: "pretty",
              }}
            >
              When a sound blends two amps into one, match their levels before
              leveling. No effect on single-amp sounds.
            </div>
            {rebalance && (
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: t.space4,
                  marginTop: t.space4,
                }}
              >
                <ByEarChip />
                <span
                  style={{
                    fontFamily: t.sans,
                    fontSize: 11,
                    color: t.mutedInk,
                  }}
                >
                  Rebalanced sounds come back flagged for a listen.
                </span>
              </div>
            )}
          </div>
        </button>
      </div>
    </LevelPage>
  );
}

export default SetupPage;
