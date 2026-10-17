// src/views/level/SetupSoundRow.tsx — one sound on the Set up page (look B): its name
// (a footswitch adds "footswitch 3"), then the Target, Block and Knob dropdowns and, on a
// footswitch of a preset with scenes, "Level it in scene". A row that needs a word shows
// it under the line (the scene couldn't be read, no amp found, a knob gone, a scene that
// doesn't turn the footswitch's effect on).
//
// Every dropdown shows the value the run will use (`useSetupRows` / `setupControls`).

import { useTheme } from "../../theme/ThemeContext";
import { Row } from "../../ui/List";
import { Select } from "../../ui/Select";
import type { SelectOption } from "../../ui/selectValue";
import type { BlockLevelHandle } from "./blockLevelGroups";
import { SETUP_COLUMN_GAP, SETUP_ROW_PAD_X, setupColumns } from "./setupLayout";
import type { SetupRowModel } from "./useSetupRows";

export interface SetupSoundRowProps {
  row: SetupRowModel;
  targetOptions: readonly SelectOption[];
  /** The scene column is on (some chosen footswitch needs it). */
  sceneColumn: boolean;
  /** A new non-zero value opens this row's Block or scene dropdown (the footer link). */
  openBlock: number;
  openScene: number;
  onTarget: (key: string, name: string) => void;
  onHandle: (key: string, handle: BlockLevelHandle | null) => void;
  onSceneContext: (key: string, value: number | null) => void;
  /** The row's candidates are read lazily for a preset the backup scan missed. */
  onOpenControls: (row: SetupRowModel) => void;
}

export function SetupSoundRow({
  row,
  targetOptions,
  sceneColumn,
  openBlock,
  openScene,
  onTarget,
  onHandle,
  onSceneContext,
  onOpenControls,
}: SetupSoundRowProps) {
  const { t } = useTheme();
  const o = row.option;
  const who = `${o.presetName}, ${o.sceneName}`;
  const { block, knob } = row.control;
  const key = o.key;
  const pick = (h: BlockLevelHandle | null | undefined) => {
    if (h !== undefined) onHandle(key, h);
  };

  return (
    <Row
      size="controls"
      padX={SETUP_ROW_PAD_X}
      data-setup-row={row.hookKey}
      note={
        row.notes.length > 0 ? (
          <>
            {row.notes.map((n) => (
              <div key={n}>{n}</div>
            ))}
          </>
        ) : undefined
      }
      style={{
        display: "grid",
        gridTemplateColumns: setupColumns(sceneColumn),
        gap: SETUP_COLUMN_GAP,
      }}
    >
      <span
        style={{
          minWidth: 0,
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
          fontSize: t.fontBody,
        }}
      >
        {o.sceneName}
        {row.caption != null && (
          <span
            style={{
              marginLeft: t.sp4,
              fontSize: t.fontCaption,
              color: t.textSecondary,
            }}
          >
            {row.caption}
          </span>
        )}
      </span>
      <Select
        size="sm"
        width="100%"
        label={`Target for ${who}`}
        value={row.target}
        options={targetOptions}
        onChange={(v) => {
          onTarget(key, v);
        }}
        tid={`target:${row.hookKey}`}
      />
      <Select
        size="sm"
        width="100%"
        label={`Block for ${who}`}
        value={block.value}
        options={block.options}
        staleLabel={block.staleLabel}
        placeholder="Choose a block"
        disabled={block.disabled}
        loading={block.loading}
        loadingText="Reading knobs…"
        listNote={block.listNote}
        emptyText="No blocks to choose"
        openRequest={openBlock}
        onOpen={() => {
          onOpenControls(row);
        }}
        onChange={(v) => {
          pick(row.control.pickBlock(v));
        }}
        tid={`block:${row.hookKey}`}
      />
      <Select
        size="sm"
        width="100%"
        menuWidth={280}
        label={`Knob for ${who}`}
        value={knob.value}
        options={knob.options}
        staleLabel={knob.staleLabel}
        placeholder=""
        attention={false}
        disabled={knob.disabled}
        loading={knob.loading}
        loadingText="Reading knobs…"
        listNote={knob.listNote}
        onOpen={() => {
          onOpenControls(row);
        }}
        onChange={(v) => {
          pick(row.control.pickKnob(v));
        }}
        tid={`knob:${row.hookKey}`}
      />
      {row.scene != null ? (
        <Select
          size="sm"
          width="100%"
          label={`Scene to level footswitch ${String((o.footswitch?.switchIndex ?? 0) + 1)} of ${o.presetName} in`}
          value={row.scene.value}
          options={row.scene.options}
          placeholder="Choose a scene"
          busy={row.scene.busy}
          openRequest={openScene}
          onChange={(v) => {
            onSceneContext(key, v === "base" ? null : Number(v));
          }}
          tid={`scene:${row.hookKey}`}
        />
      ) : (
        sceneColumn && <span />
      )}
    </Row>
  );
}
