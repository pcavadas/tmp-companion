// src/views/level/SetupPage.tsx — the leveling wizard's "Set up" stage (design
// handoff 1a: full window, one row per preset).
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
// rows always carry a real pick), over the candidate data `blockLevelGroups.ts`
// groups and ranks.
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
//
// Split: the row state + the commit live in ./useSetupRows (over the pure
// `buildSetupChoices` in ./leveling); the candidate adapters in ./setupCandidates; the
// pieces in ./FsRowControls, ./InstrumentNudge, ./CalibrationOnboardingBanner and
// ./RebalanceToggle.

import { useState } from "react";

import { useTheme, useStyles } from "../../theme/ThemeContext";
import { BackupAckLabel } from "../../ui/BackupAckLabel";
import { Button } from "../../ui/primitives";
import { LevelPage } from "./LevelPage";
import { PresetGroupList } from "./PresetGroupList";
import { PresetGroupRow } from "./PresetGroupRow";
import { useGroupOpen } from "./useGroupOpen";
import { Pick, type PickOption } from "../overlays/Pick";
import { FlatLevelPick } from "./FlatLevelPick";
import { CalibrationOnboardingBanner } from "./CalibrationOnboardingBanner";
import { FsRowControls } from "./FsRowControls";
import { InstrumentNudge } from "./InstrumentNudge";
import { RebalanceToggle } from "./RebalanceToggle";
import { baseCandidatesFetch, sceneCandidatesFetch } from "./setupCandidates";
import { useSetupRows, type SetupGroup } from "./useSetupRows";
import { instCalState, setupRowHookKey } from "./leveling";
import type { SetupOption, SetupChoice } from "./leveling";

export type { SetupChoice };

export interface SetupPageProps {
  options: SetupOption[];
  isRelevel: boolean;
  instrumentOptions: PickOption[];
  targetOptions: PickOption[];
  defaultInst: string;
  defaultTarget: string;
  onCancel: () => void;
  /** The commit: the configured rows plus the opt-in rebalance toggle. */
  onStart: (choices: SetupChoice[], rebalance: boolean) => void;
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
  onCalibrate,
}: SetupPageProps) {
  const { t } = useTheme();
  const s = useStyles();
  const requireBackup = !isRelevel;
  const [backedUp, setBackedUp] = useState(false);
  // Opt-in: equalize a path-MERGE scene's two parallel-amp lanes before joint-k. A fresh
  // Set up starts it off; the commit hands it to the run with the choices.
  const [rebalance, setRebalance] = useState(false);
  const toggleRebalance = () => {
    setRebalance(!rebalance);
  };

  const {
    groups,
    rows,
    patchRow,
    bulkInst,
    bulkTarget,
    applyBulkInst,
    applyBulkTarget,
    effectiveHandle,
    fsSuggestedFor,
    fsContextState,
    fsContextPending,
    fsContextMissing,
    candidatesFor,
    blocksFor,
    contextFor,
    fetchBlocksFor,
    fetchHandlesFor,
    fetchFsContextFor,
    buildChoices,
  } = useSetupRows({
    options,
    isRelevel,
    instrumentOptions,
    targetOptions,
    defaultInst,
    defaultTarget,
  });

  const total = options.length;

  const start = () => {
    const choices = buildChoices();
    if (choices.length) onStart(choices, rebalance);
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

  const groupNote = (g: SetupGroup): string => {
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
          size="sm"
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
            size="sm"
            icon="gauge"
            disabled={
              total === 0 ||
              (requireBackup && !backedUp) ||
              fsContextPending ||
              fsContextMissing
            }
            onClick={start}
            style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
          >
            {fsContextPending
              ? "Reading footswitches…"
              : fsContextMissing
                ? "Choose a footswitch scene"
                : `Start — ${String(total)} sound${total === 1 ? "" : "s"}`}
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
                        needsChoice={fsContextState(o) === "error"}
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
      <RebalanceToggle on={rebalance} onToggle={toggleRebalance} />
    </LevelPage>
  );
}
