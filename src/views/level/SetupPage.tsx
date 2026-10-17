// src/views/level/SetupPage.tsx — Level's Set up, a page under the toolbar (look B).
//
// Top to bottom: the title ("Level 6 sounds") with the flow's Steps, the "for all"
// choices ("Instrument for all" with 2+ presets, "Target for all"), the column heads, one
// Group (a header row per preset with its one instrument, then a row per chosen sound
// with its Target, Block, Knob and, for a footswitch of a preset with scenes, "Level it in
// scene"), and the Footer: Cancel on the left; on the right the one thing Start waits for
// (a link to it: "Add a target in Settings" while no target is configured, else the first
// row's missing choice), the backup tick and "Start leveling".
//
// What to level was chosen on the list, once; this page only says how. Every dropdown is
// pre-set to the best guess and always shows exactly what the run will use. Every chosen
// sound shows: no preset collapses. The backup tick is the only safety line (the run
// saves each sound as soon as it matches); the Summary's Try again runs again without
// coming back here, so it never asks twice.
//
// State and derivation: `useSetupRows` (over `setupControls` and `buildSetupChoices`).

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { BackupAckLabel } from "../../ui/BackupAckLabel";
import { Footer } from "../../ui/Footer";
import { Group } from "../../ui/List";
import { Select } from "../../ui/Select";
import type { SelectOption } from "../../ui/selectValue";
import { Button } from "../../ui/primitives";
import { FlowPage } from "./FlowPage";
import { SetupPresetHeader } from "./SetupPresetHeader";
import { SetupSoundRow } from "./SetupSoundRow";
import { SETUP_COLUMN_GAP, SETUP_ROW_PAD_X, setupColumns } from "./setupLayout";
import {
  TARGET_BY_NAME,
  targetForAllValue,
  useSetupRows,
  type SetupRowModel,
} from "./useSetupRows";
import type { SetupChoice, SetupOption } from "./leveling";

export type { SetupChoice };

export interface SetupPageProps {
  options: SetupOption[];
  /** "None" + each instrument (`value` = profile id). */
  instrumentOptions: SelectOption[];
  /** The instruments with a recording; any other choice shows the record link. */
  recordedInstruments: ReadonlySet<string>;
  /** The configured targets' names, in their order. */
  targetNames: string[];
  /** The instrument saved for each preset (0-based list index → id). */
  savedInstruments: Partial<Record<number, string>>;
  /** A preset's instrument when nothing is saved for it. */
  defaultInst: string;
  onCancel: () => void;
  /** The commit: the configured sounds, and the presets whose two amps are balanced
   *  first (0-based list indices). */
  onStart: (choices: SetupChoice[], rebalance: number[]) => void;
  /** Remember a preset's instrument choice (the app's own store, never the unit). */
  onInstrumentChange?: (slot: number, id: string) => void;
  /** "Record yours in Settings": go to Settings › Instruments. */
  onCalibrate?: () => void;
  /** "Add a target in Settings": go to Settings › Loudness targets. */
  onAddTarget?: () => void;
  /** The app's store (targets, saved instruments) is still loading: Start waits, and
   *  nothing says a target is missing yet. */
  storeLoading?: boolean;
}

export function SetupPage({
  options,
  instrumentOptions,
  recordedInstruments,
  targetNames,
  savedInstruments,
  defaultInst,
  onCancel,
  onStart,
  onInstrumentChange,
  onCalibrate,
  onAddTarget,
  storeLoading = false,
}: SetupPageProps) {
  const { t } = useTheme();
  const [backedUp, setBackedUp] = useState(false);
  // The footer link's request to open one row's Block or scene dropdown.
  const [openReq, setOpenReq] = useState<{
    key: string;
    which: "block" | "scene";
    n: number;
  } | null>(null);

  const setup = useSetupRows({
    options,
    instrumentOptions,
    targetNames,
    savedInstruments,
    defaultInst,
    onInstrumentChange,
  });
  const { groups, firstNeed, pending } = setup;
  // No target configured: nothing can run until one is added in Settings.
  const noTargets = setup.noTargets && !storeLoading;

  const total = options.length;
  const ready =
    total > 0 &&
    backedUp &&
    !pending &&
    !storeLoading &&
    !setup.noTargets &&
    firstNeed == null;
  const targetOptions: SelectOption[] = targetNames.map((n) => ({
    value: n,
    label: n,
  }));
  const targetAllOptions: SelectOption[] = [
    { value: TARGET_BY_NAME, label: "By name" },
    ...targetNames.map((n) => ({ value: targetForAllValue(n), label: n })),
  ];
  // The run uses "None" or an instrument with no recording: point at Settings once.
  const needsRecording =
    onCalibrate != null && groups.some((g) => !recordedInstruments.has(g.inst));
  const forAllInstrument = groups.length >= 2;

  const start = () => {
    const { choices, rebalance } = setup.buildChoices();
    if (choices.length > 0) onStart(choices, rebalance);
  };

  // "Choose a scene for footswitch 3" / "Choose a block for Lead": bring the row into
  // view and open the dropdown that needs the choice.
  const goToNeed = (row: SetupRowModel) => {
    const need = row.need;
    if (!need) return;
    document
      .querySelector(`[data-setup-row="${row.hookKey}"]`)
      ?.scrollIntoView({ block: "nearest" });
    setOpenReq((p) => ({
      key: row.option.key,
      which: need.which,
      n: (p?.n ?? 0) + 1,
    }));
  };
  const requestFor = (key: string, which: "block" | "scene"): number =>
    openReq?.key === key && openReq.which === which ? openReq.n : 0;

  const onOpenControls = (row: SetupRowModel) => {
    // Lazy read for a preset the backup scan missed (a device read); idempotent.
    if (row.option.isBase) setup.fetchBlocksFor(row.option.slot);
    else if (row.option.sceneSlot != null && row.option.footswitch == null)
      setup.fetchHandlesFor(row.option.slot);
  };

  const caption = {
    fontSize: t.fontCaption,
    color: t.textSecondary,
    whiteSpace: "nowrap",
  } as const;
  const labelled = {
    display: "flex",
    alignItems: "center",
    gap: t.sp8,
    fontSize: t.fontBody,
    whiteSpace: "nowrap",
  } as const;
  const columns = setupColumns(setup.sceneColumn);

  return (
    <FlowPage
      title={`Level ${String(total)} ${total === 1 ? "sound" : "sounds"}`}
      step={0}
      footer={
        <Footer
          left={
            <Button variant="secondary" size="lg" onClick={onCancel}>
              Cancel
            </Button>
          }
          right={
            <>
              {noTargets ? (
                onAddTarget ? (
                  <Button variant="link" icon="warn-tri" onClick={onAddTarget}>
                    Add a target in Settings
                  </Button>
                ) : (
                  <span style={caption}>Add a target in Settings</span>
                )
              ) : pending ? (
                <span style={caption}>Reading footswitch scenes</span>
              ) : (
                firstNeed?.need && (
                  <Button
                    variant="link"
                    icon="warn-tri"
                    onClick={() => {
                      goToNeed(firstNeed);
                    }}
                  >
                    {firstNeed.need.text}
                  </Button>
                )
              )}
              <BackupAckLabel checked={backedUp} onChange={setBackedUp} />
              <Button
                variant="primary"
                size="lg"
                busy={pending || storeLoading}
                disabled={!ready}
                onClick={start}
              >
                Start leveling
              </Button>
            </>
          }
        />
      }
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.sp24,
          flexWrap: "wrap",
        }}
      >
        {forAllInstrument && (
          <span style={labelled}>
            Instrument for all
            <Select
              width={130}
              label="Instrument for all presets"
              value={setup.instAll.value}
              mixed={setup.instAll.mixed}
              options={instrumentOptions}
              onChange={setup.setAllInst}
              tid="inst-all"
            />
            {needsRecording && (
              <Button variant="link" onClick={onCalibrate}>
                Record yours in Settings
              </Button>
            )}
          </span>
        )}
        <span style={labelled}>
          Target for all
          <Select
            width={130}
            label="Target for all sounds"
            value={setup.targetAll.value}
            mixed={setup.targetAll.mixed}
            options={targetAllOptions}
            onChange={setup.setAllTargets}
            tid="target-all"
          />
        </span>
      </div>

      <div>
        <div
          aria-hidden
          style={{
            display: "grid",
            gridTemplateColumns: columns,
            gap: SETUP_COLUMN_GAP,
            padding: `0 ${String(SETUP_ROW_PAD_X + 1)}px ${String(t.sp6)}px`,
            fontSize: t.fontCaption,
            fontWeight: t.weightSemibold,
            color: t.textSecondary,
          }}
        >
          <span>Sound</span>
          <span>Target</span>
          <span>Block</span>
          <span>Knob</span>
          {setup.sceneColumn && <span>Level it in scene</span>}
        </div>
        <Group label="Sounds to level">
          {groups.map((g) => (
            <div key={g.slot}>
              <SetupPresetHeader
                group={g}
                instrumentOptions={instrumentOptions}
                onInstrument={setup.setInst}
                onRecord={
                  !forAllInstrument &&
                  onCalibrate != null &&
                  !recordedInstruments.has(g.inst)
                    ? onCalibrate
                    : undefined
                }
                rebalance={setup.rebalance.has(g.slot)}
                onToggleRebalance={setup.toggleRebalance}
              />
              {g.rows.map((r) => (
                <SetupSoundRow
                  key={r.option.key}
                  row={r}
                  targetOptions={targetOptions}
                  sceneColumn={setup.sceneColumn}
                  openBlock={requestFor(r.option.key, "block")}
                  openScene={requestFor(r.option.key, "scene")}
                  onTarget={setup.setTarget}
                  onHandle={setup.setHandle}
                  onSceneContext={setup.setSceneContext}
                  onOpenControls={onOpenControls}
                />
              ))}
            </div>
          ))}
        </Group>
      </div>
    </FlowPage>
  );
}
