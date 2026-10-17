// src/views/doctor/DoctorInstrument.tsx — "Which instrument plays these?", Doctor's first
// step when a chosen preset has no instrument yet (otherwise Doctor skips it).
//
// Top to bottom: the title with Doctor's Steps; one line naming the presets with no
// instrument yet; "Instrument for all" (only with 2 or more presets) and "Record yours in
// Settings"; one Group with a row per chosen preset — its name, slot and one Instrument
// dropdown (one instrument per preset, like Level; a preset with none yet shows "Choose" in
// the attention look); then the Footer: Back on the left; on the right a link to the first
// preset still without one, and "Check N sounds", greyed until every preset has one.
//
// Each choice is remembered for that preset in the app's own store on this computer, never
// on the unit, so the next check starts from it ("None" is a choice too). On a wide window
// the content keeps a 640 px column under the title.

import { useMemo, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Footer } from "../../ui/Footer";
import { Group, Row } from "../../ui/List";
import { Select } from "../../ui/Select";
import type { SelectOption } from "../../ui/selectValue";
import { SlotLabel } from "../../ui/SlotLabel";
import { Button } from "../../ui/primitives";
import { useWindowWidth } from "../../lib/useWindowWidth";
import { FlowPage } from "../level/FlowPage";
import { DOCTOR_STEPS } from "./useDoctorFlow";
import {
  everyInstrumentChosen,
  missingLine,
  missingLink,
  type ChosenPreset,
} from "./instrumentModel";
import { WIDE_WINDOW } from "./layout";

export interface DoctorInstrumentProps {
  /** The presets the chosen sounds span, in slot order. */
  presets: ChosenPreset[];
  /** How many sounds the check will listen to. */
  soundCount: number;
  /** "None" + each instrument. */
  instrumentOptions: SelectOption[];
  /** The instruments with a recording; any other choice shows the record link. */
  recordedInstruments: ReadonlySet<string>;
  /** Each preset's instrument so far ("" = not chosen). */
  initial: ReadonlyMap<number, string>;
  /** Remember a preset's instrument (the app's own store, never the unit). */
  onInstrumentChange: (slot: number, id: string) => void;
  onBack: () => void;
  /** Check, with each preset's instrument. */
  onRun: (instBySlot: ReadonlyMap<number, string>) => void;
  /** "Record yours in Settings": Settings › Instruments. */
  onCalibrate?: () => void;
}

export function DoctorInstrument({
  presets,
  soundCount,
  instrumentOptions,
  recordedInstruments,
  initial,
  onInstrumentChange,
  onBack,
  onRun,
  onCalibrate,
}: DoctorInstrumentProps) {
  const { t } = useTheme();
  const wide = useWindowWidth() >= WIDE_WINDOW;
  const [inst, setInst] = useState<ReadonlyMap<number, string>>(initial);
  // The footer link's request to open one preset's dropdown.
  const [openReq, setOpenReq] = useState<{ slot: number; n: number } | null>(
    null,
  );

  const choose = (slot: number, id: string) => {
    setInst((prev) => new Map(prev).set(slot, id));
    onInstrumentChange(slot, id);
  };
  const chooseAll = (id: string) => {
    setInst(new Map(presets.map((p) => [p.slot, id])));
    for (const p of presets) onInstrumentChange(p.slot, id);
  };

  const missing = presets.filter((p) => (inst.get(p.slot) ?? "") === "");
  const missingNames = missing.map((p) => p.name);
  const line = missingLine(missingNames);
  const ready = everyInstrumentChosen(inst) && presets.length > 0;

  const forAll = presets.length >= 2;
  const values = presets.map((p) => inst.get(p.slot) ?? "");
  const allSame = values.every((v) => v === values[0]);
  const forAllValue = allSame ? (values[0] ?? "") : "";
  const needsRecording =
    onCalibrate != null && values.some((v) => !recordedInstruments.has(v));
  const recordLink = needsRecording ? (
    <Button variant="link" onClick={onCalibrate}>
      Record yours in Settings
    </Button>
  ) : null;

  const firstMissing = missing.length > 0 ? missing[0] : null;
  const goToMissing = () => {
    if (!firstMissing) return;
    document
      .querySelector(`[data-instrument-row="${String(firstMissing.slot)}"]`)
      ?.scrollIntoView({ block: "nearest" });
    setOpenReq((p) => ({ slot: firstMissing.slot, n: (p?.n ?? 0) + 1 }));
  };

  const column = useMemo(
    () => ({
      display: "flex",
      flexDirection: "column" as const,
      gap: t.sp12 + t.sp2,
    }),
    [t],
  );

  return (
    <FlowPage
      title="Which instrument plays these?"
      step={0}
      steps={DOCTOR_STEPS}
      maxWidth={wide ? 640 : undefined}
      footer={
        <Footer
          left={
            <Button variant="secondary" size="lg" onClick={onBack}>
              Back
            </Button>
          }
          right={
            <>
              {firstMissing && (
                <Button variant="link" icon="warn-tri" onClick={goToMissing}>
                  {missingLink(missingNames)}
                </Button>
              )}
              <Button
                variant="primary"
                size="lg"
                disabled={!ready}
                onClick={() => {
                  onRun(inst);
                }}
              >
                {`Check ${String(soundCount)} ${soundCount === 1 ? "sound" : "sounds"}`}
              </Button>
            </>
          }
        />
      }
    >
      <div style={column}>
        {line != null && (
          <p
            style={{
              margin: 0,
              fontSize: t.fontBody,
              lineHeight: `${String(t.lineBody)}px`,
              color: t.textSecondary,
            }}
          >
            {line}
          </p>
        )}
        {forAll && (
          <span
            style={{
              display: "flex",
              alignItems: "center",
              gap: t.sp8,
              fontSize: t.fontBody,
              whiteSpace: "nowrap",
            }}
          >
            Instrument for all
            <Select
              width={130}
              label="Instrument for all presets"
              value={forAllValue}
              mixed={!allSame}
              attention={false}
              options={instrumentOptions}
              onChange={chooseAll}
              tid="inst-all"
            />
            {recordLink}
          </span>
        )}
        <Group label="Presets to check">
          {presets.map((p) => (
            <Row key={p.slot} data-instrument-row={p.slot}>
              <span
                style={{
                  flex: 1,
                  minWidth: 0,
                  display: "flex",
                  alignItems: "baseline",
                  gap: t.sp8,
                  overflow: "hidden",
                }}
              >
                <span
                  style={{
                    fontWeight: t.weightSemibold,
                    whiteSpace: "nowrap",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                  }}
                >
                  {p.name}
                </span>
                <SlotLabel index={p.slot} />
              </span>
              {!forAll && recordLink}
              <Select
                size="sm"
                width={150}
                label={`Instrument for ${p.name}`}
                value={inst.get(p.slot) ?? ""}
                options={instrumentOptions}
                onChange={(v) => {
                  choose(p.slot, v);
                }}
                openRequest={openReq?.slot === p.slot ? openReq.n : 0}
                tid={`inst:${String(p.slot)}`}
              />
            </Row>
          ))}
        </Group>
      </div>
    </FlowPage>
  );
}
