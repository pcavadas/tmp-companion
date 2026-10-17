// src/views/level/HowLevelingSheet.tsx — the "How leveling works" dialog, look B.
//
// A calm, read-only Dialog (500 wide) opened from "How it works" in the Level list's
// Footer: the three steps every run takes, then "Which knob it turns" — a two-column
// table naming the one control leveling turns for each kind of sound — and one closing
// line. "Done" or a click on the scrim closes it (it guards no write). No close (×)
// button, no inputs, no persistence, no fetch.
//
// "Two amps at once" turns "Both amps' volume" (not the preset's level): the rebalance
// adjusts both amps' output, the same control as the scene case, then levels the pair.

import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import {
  Dialog,
  DialogHeader,
  DialogBody,
  DialogFooter,
} from "../../ui/Dialog";
import { Button } from "../../ui/primitives";

const TITLE = "How leveling works";
/** Vertical padding of each "Which knob it turns" row (7 px, between the 6 and 8 steps). */
const KNOB_ROW_PAD_Y = 7;

// The three steps every run takes.
const STEPS: readonly string[] = [
  "A short test tone plays through the sound.",
  "The app listens to how loud it comes out.",
  "It turns one knob until the sound reaches your target.",
];

// Each kind of sound, and the one knob leveling turns for it.
const KNOBS: readonly { sound: string; knob: string }[] = [
  { sound: "Main sound", knob: "The preset's level" },
  {
    sound: "A scene",
    knob: "The amp's output, or the block the scene turns on",
  },
  { sound: "A footswitch", knob: "A knob on the block it turns on" },
  { sound: "Two amps at once", knob: "Both amps' volume" },
];

export interface HowLevelingSheetProps {
  onClose: () => void;
}

export function HowLevelingSheet({ onClose }: HowLevelingSheetProps) {
  const { t } = useTheme();
  return (
    <Dialog size="explainer" onClose={onClose} label={TITLE}>
      <DialogHeader>
        <h2
          style={{
            margin: 0,
            fontSize: t.fontLarge,
            lineHeight: `${String(t.lineLarge)}px`,
            fontWeight: t.weightSemibold,
            color: t.text,
          }}
        >
          {TITLE}
        </h2>
      </DialogHeader>
      <DialogBody>
        <div
          style={{ display: "flex", flexDirection: "column", gap: t.sp12 + 2 }}
        >
          <ol
            style={{
              margin: 0,
              paddingLeft: t.sp16 + t.sp4,
              display: "flex",
              flexDirection: "column",
              gap: t.sp4,
            }}
          >
            {STEPS.map((step) => (
              <li key={step}>{step}</li>
            ))}
          </ol>
          <div>
            <h3
              style={{
                ...sectionLabel(t),
                margin: `0 0 ${String(t.sp4)}px`,
              }}
            >
              Which knob it turns
            </h3>
            {KNOBS.map((k) => (
              <div
                key={k.sound}
                style={{
                  display: "grid",
                  gridTemplateColumns: "150px minmax(0, 1fr)",
                  gap: t.sp12,
                  padding: `${String(KNOB_ROW_PAD_Y)}px 0`,
                  borderTop: `1px solid ${t.separator}`,
                }}
              >
                <span style={{ fontWeight: t.weightMedium }}>{k.sound}</span>
                <span style={{ color: t.textSecondary }}>{k.knob}</span>
              </div>
            ))}
          </div>
          <p style={{ margin: 0 }}>
            You can check and change the block and knob for each sound before
            you start.
          </p>
        </div>
      </DialogBody>
      <DialogFooter>
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </DialogFooter>
    </Dialog>
  );
}
