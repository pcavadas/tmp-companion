// src/views/level/SetupPresetHeader.tsx — a preset's header row on the Set up page
// (look B, `groupHeader` fill): its name and slot, then "Instrument" and the preset's one
// instrument dropdown (every sound of the preset uses it). With one preset in the run the
// "Record yours in Settings" link sits here, after the dropdown. At the far right, only for
// a preset whose two amps the run can balance, "Balance its two amps first".

import { useTheme } from "../../theme/ThemeContext";
import { Row } from "../../ui/List";
import { Select } from "../../ui/Select";
import type { SelectOption } from "../../ui/selectValue";
import { SlotLabel } from "../../ui/SlotLabel";
import { Button, Checkbox } from "../../ui/primitives";
import type { SetupGroupModel } from "./useSetupRows";
import { SETUP_ROW_PAD_X } from "./setupLayout";

export interface SetupPresetHeaderProps {
  group: SetupGroupModel;
  instrumentOptions: readonly SelectOption[];
  onInstrument: (slot: number, id: string) => void;
  /** Shown after the instrument dropdown: the link to record an instrument. */
  onRecord?: () => void;
  rebalance: boolean;
  onToggleRebalance: (slot: number) => void;
}

export function SetupPresetHeader({
  group,
  instrumentOptions,
  onInstrument,
  onRecord,
  rebalance,
  onToggleRebalance,
}: SetupPresetHeaderProps) {
  const { t } = useTheme();
  return (
    <Row header padX={SETUP_ROW_PAD_X} data-setup-preset={group.slot}>
      <span
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: t.sp8,
          minWidth: 0,
          flexShrink: 1,
        }}
      >
        <span
          style={{
            fontWeight: t.weightSemibold,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
            minWidth: 0,
          }}
        >
          {group.name}
        </span>
        <SlotLabel index={group.slot} style={{ flexShrink: 0 }} />
      </span>
      <span
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.sp8,
          marginLeft: t.sp8,
          flexShrink: 0,
        }}
      >
        <span
          style={{
            fontSize: t.fontCaption,
            color: t.textSecondary,
            whiteSpace: "nowrap",
          }}
        >
          Instrument
        </span>
        <Select
          size="sm"
          width={120}
          label={`Instrument for ${group.name}`}
          value={group.inst}
          options={instrumentOptions}
          onChange={(v) => {
            onInstrument(group.slot, v);
          }}
          tid={`inst:${String(group.slot)}`}
        />
        {onRecord && (
          <Button variant="link" size="sm" onClick={onRecord}>
            Record yours in Settings
          </Button>
        )}
      </span>
      <span style={{ flex: 1 }} />
      {group.canRebalance && (
        <Checkbox
          checked={rebalance}
          onChange={() => {
            onToggleRebalance(group.slot);
          }}
          label="Balance its two amps first"
          style={{ whiteSpace: "nowrap", flexShrink: 0 }}
        />
      )}
    </Row>
  );
}
