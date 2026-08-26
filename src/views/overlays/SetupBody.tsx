// src/views/overlays/SetupBody.tsx — wizard step 2, "Set up".
//
// Everything chosen in the LIST (the scene tree) WILL be leveled — this step never
// re-gates inclusion. Its single job is to set each sound's INSTRUMENT + TARGET +
// LEVELING HANDLE (D2 — every row levels against ONE user-chosen block+param control,
// picked from `BlockLevelPick`'s two dropdowns — block, then control; Base rows
// default to the "Preset level" pseudo-option, Scene rows to "Amp output level",
// Footswitch rows always carry a real pick):
//   • A top "Apply to" bar is a brush that writes instrument + target to every row at
//     once — or, when the user ticks a few rows, to just those. Ticking is a bulk-edit
//     convenience only.
//   • Each row also carries its OWN instrument + target pickers, plus its own handle
//     (and, for footswitches, scene-context — D3) pickers.
// On "Level N sounds" it hands the flow one SetupChoice per option. The footer's
// "I've backed up with Pro Control" checkbox gates the button (an inline backup
// acknowledgment — there is no separate Back-up step). Re-level skips the ack (the
// user already acknowledged when the initial run started).
//
// History (do not reintroduce): an earlier build put inclusion checkboxes here,
// forcing users to pick sounds twice (list + dialog). The list is the single place
// you choose WHAT to level; this step only chooses HOW. A later build gave footswitch
// rows a "Verify only" default + "Make level-neutral" opt-in and scene rows a
// match/offset target-mode chip — both REMOVED: every row levels now (the backend
// dropped the verify-only footswitch mode entirely), and the combined handle picker
// replaced the target-mode concept.

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { useTheme, useStyles } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button, Toggle } from "../../ui/primitives";
import { BackupAckLabel } from "../../ui/BackupAckLabel";
import { SetupGroupHeader } from "../../ui/SetupGroupHeader";
import { PresetOptionRow } from "../../ui/PresetOptionRow";
import { ApplyToBar } from "../../ui/ApplyToBar";
import { usePickedRows } from "../../lib/usePickedRows";
import { WizardFooter, WizTitle } from "./WizardShell";
import { ByEarChip } from "./ByEarChip";
import { Pick, type PickOption } from "./Pick";
import {
  BlockLevelPick,
  type BlockLevelHandle,
  type BlockLevelFetch,
} from "./BlockLevelPick";
import { PickWarnNote } from "./PickWarnNote";
import {
  useSceneHandles,
  type HandleFetchState,
} from "../level/useSceneHandles";
import {
  useLevelBlocks,
  type BaseBlockFetchState,
} from "../level/useLevelBlocks";
import {
  useFootswitchSceneContexts,
  type FsContextFetchState,
} from "../level/useFootswitchSceneContexts";
import {
  footswitchNameForCandidate,
  instCalState,
  setupRowHookKey,
  targetFromCandidate,
} from "../level/leveling";
import type {
  SetupOption,
  SetupChoice,
  BaseHandlePick,
  FootswitchTarget,
} from "../level/leveling";
import type { LevelParamCandidate } from "../../lib/types";

export type { SetupChoice };

/** The "calibrate" word — an inviting next-step cue. Dotted terracotta underline
 *  that solidifies on hover; clicking jumps to Settings → Instruments (`onCalibrate`,
 *  threaded from App down through LevelView/LevelingWizard). Click-only app, so no
 *  keyboard handler — the click IS the affordance. Intentional detour: it unmounts
 *  LevelView and discards any in-progress wizard setup. */
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

/** Quiet good → better → best caption beneath the apply-to-all instrument picker.
 *  `cal` removes the element entirely (no reserved height) so the list below reclaims
 *  the space. Not a warning — muted body with a single accent cue on "calibrate". */
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
        marginTop: t.space4,
        fontFamily: t.sans,
        fontSize: 12,
        lineHeight: 1.45,
        color: t.mutedInk,
      }}
    >
      {state === "none" ? (
        <span>
          Set an instrument for better results —{" "}
          <CalibrateCue onCalibrate={onCalibrate}>calibrate</CalibrateCue> it
          for the best.
        </span>
      ) : (
        <span>
          <CalibrateCue onCalibrate={onCalibrate}>Calibrate</CalibrateCue> this
          instrument for the best results.
        </span>
      )}
    </div>
  );
}

/** Onboarding nudge toward Tier-2 calibration (capture-as-stimulus) — a small
 *  dismissable banner shown once per wizard open, only while the chosen instrument
 *  is a real, uncalibrated profile (an unset/"None" instrument or an already-
 *  calibrated one shows nothing). Local `dismissed` state, so re-entering the Set
 *  up step (a fresh SetupBody mount) shows it again — cheap enough not to thread
 *  through the flow. No navigation coupling: plain text points at Settings. */
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
        margin: `${String(t.space6)}px ${String(t.space10)}px 0`,
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
        Level with your own guitar — a 2-minute calibration makes leveling match
        your instrument. Settings → Instruments → Calibrate.
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

/** A footswitch row's leveling controls (D2 + D3): `BlockLevelPick`'s two-dropdown
 *  block+param picker — NO pseudo-option (the backend removed the verify-only "no
 *  handle" row entirely, so every FS row must carry a real handle) — stacked over
 *  the scene-context picker
 *  (which scene, if any, this switch's sound is measured and solved in). Picking a
 *  scene that doesn't actually turn the switch on is ALLOWED — flagged, never blocked
 *  (D3). `sceneContext` is the EFFECTIVE value to display, resolved by the caller
 *  (the row's own pick, else the lazily-fetched `suggested`, else the carried-forward
 *  default) so this stays a pure renderer. */
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
  const { t } = useTheme();
  const candidates: BlockLevelFetch = {
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
        display: "flex",
        flexDirection: "column",
        gap: t.space2,
        minWidth: 0,
      }}
    >
      <BlockLevelPick
        handle={handle}
        onHandleChange={(h) => {
          onHandleChange(h);
        }}
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

/** `useLevelBlocks` (Base rows) → the combined picker's candidate list. TWO sources ride
 *  the SAME `BaseCandidate` shape now: the backup-derived arm carries a real `paramClass`
 *  (`base_handles`, classified backend-side, same table the scene picker uses), while the
 *  device-fallback arm (`list_level_blocks` → `session::LevelBlock`) carries none — that
 *  wire shape was never annotated, and this is a deliberate divergence, not a gap to
 *  "fix" by classifying frontend-side (there is no local classifier mirror; see
 *  `LevelParamCandidate`'s doc in `types.ts`). `BlockLevelPick`'s `rank()` already sorts
 *  an `undefined` class after every classified one, so the fallback arm's candidates
 *  simply fall in last, exactly as they always have. */
function baseCandidatesFetch(state: BaseBlockFetchState): BlockLevelFetch {
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

/** A non-"isolated" scene-handle candidate's disabled reason — mirrors
 *  `SceneHandleCandidate.scope`'s doc. `"isolated"` candidates aren't disabled at
 *  all, so this is only ever called for the other two. */
function sceneDisabledTitle(
  scope: "isolated" | "shared_with_base" | "unknown",
): string | undefined {
  if (scope === "shared_with_base")
    return "shared with the base preset — changes every scene sharing it";
  if (scope === "unknown") return "could not be confirmed for this scene";
  return undefined;
}

/** `BlockLevelCandidate`'s disabled/disabledTitle pair, paired atomically so a
 *  disabled row can never come out of this without its reason. */
function sceneDisabledFields(
  scope: "isolated" | "shared_with_base" | "unknown",
): { disabled: true; disabledTitle: string } | { disabled?: false } {
  const title = sceneDisabledTitle(scope);
  return title != null ? { disabled: true, disabledTitle: title } : {};
}

/** `list_scene_level_handles`'s `allCandidates` (Scene rows) → the combined picker's
 *  candidate list — the full class-annotated superset, level-class first. */
function sceneCandidatesFetch(state: HandleFetchState): BlockLevelFetch {
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

/** One row's editable choices, keyed by `SetupOption.key`. `handle` is Base/Scene/FS
 *  rows' chosen leveling control (D2) — `null` = the pseudo-option's default path on
 *  a Base/Scene row; always seeded non-null on a footswitch row. `sceneContext` is
 *  footswitch-row-only (D3): `undefined` = not yet explicitly picked by the user —
 *  the effective value falls back to the lazily-fetched `suggested`, then the
 *  carried-forward default. */
interface RowChoice {
  inst: string;
  target: string;
  handle: BlockLevelHandle | null;
  sceneContext: number | null | undefined;
}

export interface SetupBodyProps {
  /** The exact scenes picked in the list — all of them WILL be leveled. */
  options: SetupOption[];
  /** How many presets the flow is leveling (for the sub-line). */
  presetCount: number;
  /** True ⇒ re-leveling a clamped subset (title prefix + backup ack hidden). */
  isRelevel: boolean;
  instrumentOptions: PickOption[];
  targetOptions: PickOption[];
  /** Store-backed defaults (never hard-coded ids). */
  defaultInst: string;
  defaultTarget: string;
  onCancel: () => void;
  onStart: (choices: SetupChoice[]) => void;
  /** Opt-in: equalize a path-MERGE preset's two parallel-amp lanes before leveling.
   * A no-op on series / single-amp / split-output presets. */
  onRebalanceChange?: (on: boolean) => void;
  /** Jump to Settings → Instruments (the "calibrate" cue in the instrument nudge). */
  onCalibrate?: () => void;
}

export function SetupBody({
  options,
  presetCount,
  isRelevel,
  instrumentOptions,
  targetOptions,
  defaultInst,
  defaultTarget,
  onCancel,
  onStart,
  onRebalanceChange,
  onCalibrate,
}: SetupBodyProps) {
  const { t } = useTheme();
  const s = useStyles();
  // Inline backup acknowledgment — gates the primary button (mirrors the Copy save
  // bar). Required only on a fresh run; re-level already acknowledged. Default off.
  const requireBackup = !isRelevel;
  const [backedUp, setBackedUp] = useState(false);
  // Advanced, opt-in run option — applies to the whole run; default off. Toggling it
  // both updates the local pill and notifies the flow (read at run time as `rebalance`).
  const [rebalance, setRebalance] = useState(false);
  const toggleRebalance = () => {
    const next = !rebalance;
    setRebalance(next);
    onRebalanceChange?.(next);
  };
  // The flow holds `rebalance` in a ref that survives this body's unmount/remount, but
  // the pill resets to its default each mount — sync the ref to the VISIBLE state on
  // mount so a stale ON from a prior run (re-level / Back→Continue / a new flow) can't
  // silently rebalance against an OFF-looking pill.
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

  // One per-row choice map — instrument/target + the leveling handle (+ footswitch
  // scene context), all seeded in one pass and patched with one setter.
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
              : (o.sceneHandle ?? null),
        sceneContext: undefined,
      };
    });
    return m;
  });
  const patchRow = (k: string, partial: Partial<RowChoice>) => {
    setRows((p) => {
      const cur = p[k];
      // Every option's key was seeded at mount — an unseeded key is unreachable, but
      // guard rather than assume so the map's real (possibly-undefined) index type
      // holds (this tsconfig has no `noUncheckedIndexedAccess`, so the honest guard
      // has to be written by hand rather than inferred).
      if (!cur) return p;
      return { ...p, [k]: { ...cur, ...partial } };
    });
  };

  // Leveling-handle candidates. Base/Scene resolve INSTANT-FIRST off the startup backup
  // scan (no device I/O — see `useLevelBlocks`/`useSceneHandles`), falling back to a real
  // device read (load+discovery / a field-8 read) only for a row the backup didn't cover.
  // Footswitch candidates need no fetch at all — the switch's own `level_params` are
  // already in hand. The scene-context picker (D3) stays a real, lazy device read.
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

  // Eagerly warm every group's Base/Scene candidates on Set-up step render — SCAN-ONLY:
  // gated on `hasBackupData` so this provably cannot reach the device. A slot the backup
  // scan didn't cover (map key absent — including the "ready but failed" scan shape,
  // where every map is empty) is left unfetched here; its row falls back to a real device
  // read LAZILY, only if the user actually opens that row's own picker (the `onOpen`
  // handlers below, unchanged). Idempotent either way (`useLazySlotCache`'s per-mount
  // `fetchedSlotsRef`), so re-running this on every render (the fetchers are fresh
  // closures, not memoized — matching every other call site in this file) only re-checks
  // already-fetched slots, never re-fetches them.
  useEffect(() => {
    groups.forEach((g) => {
      if (baseHasBackup(g.slot)) fetchBlocksFor(g.slot);
      if (sceneHasBackup(g.slot)) fetchHandlesFor(g.slot);
    });
  }, [groups, fetchBlocksFor, fetchHandlesFor, baseHasBackup, sceneHasBackup]);

  // A footswitch row's D3 default: the lazily-fetched `suggested` scene, when resolved.
  const fsSuggestedFor = (o: SetupOption): number | null => {
    if (o.footswitch == null) return null;
    const st = contextFor(o.slot, o.footswitch.switchIndex);
    return st.status === "resolved" ? (st.row?.suggested ?? null) : null;
  };

  // Bulk-edit selection (which rows the "Apply to" bar writes to). Empty = all.
  const {
    picked,
    togglePick,
    clearPicked,
    somePicked,
    targetsForBulk,
    scopeLabel,
  } = usePickedRows(options);

  // The "Apply to" bar's current value (also the brush applied on change).
  const [bulkInst, setBulkInst] = useState(defaultInst);
  const [bulkTarget, setBulkTarget] = useState(defaultTarget);
  const applyBulkInst = (v: string) => {
    setBulkInst(v);
    setRows((p) => {
      const n = { ...p };
      targetsForBulk().forEach((k) => {
        const cur = n[k];
        if (cur) n[k] = { ...cur, inst: v };
      });
      return n;
    });
  };
  const applyBulkTarget = (v: string) => {
    setBulkTarget(v);
    setRows((p) => {
      const n = { ...p };
      targetsForBulk().forEach((k) => {
        const cur = n[k];
        if (cur) n[k] = { ...cur, target: v };
      });
      return n;
    });
  };

  const total = options.length;

  const start = () => {
    const choices: SetupChoice[] = options.map((o) => {
      const row = rows[o.key];
      let option = o;
      if (o.footswitch != null) {
        const chosenHandle = row?.handle;
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
          // LABEL PROVENANCE. `sceneName` is the Level list's row name for a footswitch
          // (never sent to the backend — the assign gate only ever edits an EXISTING
          // `param` fn or refuses, so there is no on-device `customLabel` write to keep
          // in sync any more). It was chosen back in `chosenFrom`, which only knew the
          // tone-safe DEFAULT candidate — so a user who overrode that pick here would
          // otherwise see the row still named after a block the run never touched.
          // Re-derive it from the candidate actually being leveled. A LABELED switch
          // keeps its own name: that string is the player's, and nothing about picking
          // a different knob makes it wrong.
          ...(o.fsUnlabeled === true && candidate
            ? { sceneName: footswitchNameForCandidate(candidate) }
            : {}),
        };
      } else if (o.isBase) {
        // `chosen` (a `BlockLevelHandle`) already carries exactly the three fields a
        // `BaseHandlePick` needs — no block-list re-lookup required. `block_value`
        // is ALWAYS `null` at dispatch (`buildLevelJob`): the run's own fresh
        // saved-doc read anchors the wet floor instead (`level_preset.rs`'s
        // block-value fallback), for either source, so there is nothing here to
        // resolve from a live read.
        const chosen = row?.handle;
        const baseHandle: BaseHandlePick | null = chosen
          ? {
              groupId: chosen.groupId,
              nodeId: chosen.nodeId,
              parameterId: chosen.parameterId,
            }
          : null; // the "Preset level" pseudo-option (D2 default)
        option = { ...o, baseHandle };
      } else if (o.sceneSlot != null) {
        option = { ...o, sceneHandle: row?.handle ?? null };
      }
      return {
        option,
        instId: row?.inst ?? defaultInst,
        targetName: row?.target ?? defaultTarget,
      };
    });
    if (choices.length) onStart(choices);
  };

  return (
    <>
      <div
        style={{
          flexShrink: 0,
          padding: `${String(t.space8)}px ${String(t.space10)}px ${String(t.space6)}px`,
          borderBottom: `0.5px solid ${t.hairline}`,
        }}
      >
        <WizTitle>
          {isRelevel
            ? "Re-level — set instrument & target"
            : "Set instrument & target"}
        </WizTitle>
        <div
          style={{
            fontFamily: t.mono,
            fontSize: 10.5,
            letterSpacing: "0.04em",
            color: t.mutedInk,
            marginTop: t.space4,
          }}
        >
          {total} sound{total === 1 ? "" : "s"} · {presetCount} preset
          {presetCount === 1 ? "" : "s"}
        </div>
      </div>

      <CalibrationOnboardingBanner
        show={instCalState(bulkInst, instrumentOptions) === "uncal"}
      />

      {/* apply-to bar — writes to all rows, or to the ticked rows */}
      <ApplyToBar
        label={`Apply to ${scopeLabel}`}
        somePicked={somePicked}
        onClear={clearPicked}
      >
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: t.space8,
          }}
        >
          <Pick
            grow
            value={bulkInst}
            options={instrumentOptions}
            onChange={applyBulkInst}
          />
          <Pick
            grow
            value={bulkTarget}
            options={targetOptions}
            onChange={applyBulkTarget}
          />
        </div>
        <InstrumentNudge
          state={instCalState(bulkInst, instrumentOptions)}
          onCalibrate={onCalibrate}
        />
      </ApplyToBar>

      {/* every sound that will be leveled — set any row directly, or tick for bulk */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          padding: `${String(t.space3)}px 0`,
        }}
      >
        {groups.map((g) => (
          <div
            key={g.slot}
            style={{
              padding: `${String(t.space5)}px ${String(t.space10)}px ${String(t.space6)}px`,
            }}
          >
            <SetupGroupHeader slot={g.slot} name={g.name} />
            {g.opts.map((o) => {
              const row = rows[o.key];
              const tag = o.isBase ? (o.hasScenes ? "BASE" : null) : o.tag;
              const nameLabel = o.isBase ? "Whole preset" : o.sceneName;
              const sub = o.isBase
                ? "levels this preset against the others"
                : o.footswitch != null
                  ? "evens this footswitch out to your target"
                  : "levels this scene against the preset’s base";
              return (
                <PresetOptionRow
                  key={o.key}
                  setupRowKey={setupRowHookKey(o)}
                  name={nameLabel}
                  tag={tag ?? undefined}
                  isBase={o.isBase}
                  sub={sub}
                  isPicked={picked.has(o.key)}
                  onTogglePick={() => {
                    togglePick(o.key);
                  }}
                  title="Tick to bulk-edit this row with the bar above"
                  columns="192px 108px 108px"
                >
                  {/* Every row's own leveling handle (D2): Base/Scene get
                      `BlockLevelPick`'s two-dropdown (block, then control) picker
                      with a pseudo-default; footswitch rows also carry the D3
                      scene-context picker. Base rows without scenes still qualify
                      for the Base picker (they're the whole preset). */}
                  {o.footswitch != null &&
                  o.levelParams &&
                  o.levelParams.length > 0 ? (
                    <FsRowControls
                      switchIndex={o.footswitch.switchIndex}
                      levelParams={o.levelParams}
                      fsSceneNames={o.fsSceneNames ?? []}
                      handle={row?.handle ?? null}
                      onHandleChange={(h) => {
                        patchRow(o.key, { handle: h });
                      }}
                      sceneContext={
                        row?.sceneContext !== undefined
                          ? row.sceneContext
                          : (fsSuggestedFor(o) ?? o.footswitch.sceneContext)
                      }
                      ctxState={contextFor(o.slot, o.footswitch.switchIndex)}
                      onOpenSceneContext={() => {
                        fetchFsContextFor(o.slot);
                      }}
                      onSceneContextChange={(v) => {
                        patchRow(o.key, { sceneContext: v });
                      }}
                    />
                  ) : o.isBase ? (
                    <BlockLevelPick
                      pseudoLabel="Preset level"
                      handle={row?.handle ?? null}
                      onHandleChange={(h) => {
                        patchRow(o.key, { handle: h });
                      }}
                      candidates={baseCandidatesFetch(blocksFor(o.slot))}
                      onOpen={() => {
                        fetchBlocksFor(o.slot);
                      }}
                    />
                  ) : o.sceneSlot != null ? (
                    <BlockLevelPick
                      pseudoLabel="Amp output level"
                      handle={row?.handle ?? null}
                      onHandleChange={(h) => {
                        patchRow(o.key, { handle: h });
                      }}
                      candidates={sceneCandidatesFetch(
                        candidatesFor(o.slot, o.sceneSlot),
                      )}
                      onOpen={() => {
                        fetchHandlesFor(o.slot);
                      }}
                    />
                  ) : (
                    <div />
                  )}
                  <Pick
                    grow
                    value={row?.inst ?? defaultInst}
                    options={instrumentOptions}
                    onChange={(v) => {
                      patchRow(o.key, { inst: v });
                    }}
                  />
                  <Pick
                    grow
                    tid={`target:${g.name}`}
                    value={row?.target ?? defaultTarget}
                    options={targetOptions}
                    onChange={(v) => {
                      patchRow(o.key, { target: v });
                    }}
                  />
                </PresetOptionRow>
              );
            })}
          </div>
        ))}
      </div>

      {/* run option — advanced, opt-in, applies to the whole run. Mirrors the apply-to
          bar at the top (same tint + hairline) so the two config zones bookend the list.
          ALWAYS visible: the engine no-ops on non-merged sounds, and setup does no device
          reads (topology is only known once each preset loads at run time). */}
      <div
        style={{
          flexShrink: 0,
          padding: `${String(t.space6)}px ${String(t.space10)}px ${String(t.space6)}px`,
          background: t.bgAlt,
          borderTop: `0.5px solid ${t.hairline}`,
        }}
      >
        <div style={{ ...s.kickerWide(t.faint), marginBottom: t.space4 }}>
          Run option
        </div>
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

      <WizardFooter
        left={
          <Button
            variant="ghost"
            small
            onClick={onCancel}
            style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
          >
            Cancel
          </Button>
        }
        right={
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
              {`Level ${String(total)} sound${total === 1 ? "" : "s"}`}
            </Button>
          </>
        }
      />
    </>
  );
}

export default SetupBody;
