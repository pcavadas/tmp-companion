// src/views/ConnectionHelpSheet.tsx — the "Connection help" explainer behind the no-device
// page's one button (DeviceState). A calm, read-only Dialog (look B, 460 wide) with the
// four checks that actually get the unit talking to the app: a data cable and power, Pro
// Control quit (it holds the unit's one exclusive connection), a short wait while the unit
// starts up, then another port or cable. The app connects by itself, so there is no
// Connect button to point at. No inputs, no persistence, no fetch. The backdrop closes it:
// it guards no write.

import { useTheme } from "../theme/ThemeContext";
import { Dialog, DialogHeader, DialogBody, DialogFooter } from "../ui/Dialog";
import { Button } from "../ui/primitives";

const STEPS: readonly { title: string; body: string }[] = [
  {
    title: "Plug it in and switch it on",
    body: "Use a USB-C cable that carries data. Some cables only charge.",
  },
  {
    title: "Quit Pro Control",
    body: "Only one app can talk to the Tone Master Pro at a time.",
  },
  {
    title: "Give it a few seconds",
    body: "It connects by itself once it has started up.",
  },
  {
    title: "Still nothing?",
    body: "Try another USB port or another cable.",
  },
];

const TITLE = "Connecting your Tone Master Pro";

export interface ConnectionHelpSheetProps {
  onClose: () => void;
}

export function ConnectionHelpSheet({ onClose }: ConnectionHelpSheetProps) {
  const { t } = useTheme();
  return (
    <Dialog size="sm" onClose={onClose} label={TITLE}>
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
        <ol style={{ margin: 0, padding: 0, listStyle: "none" }}>
          {STEPS.map((step, i) => (
            <li
              key={step.title}
              style={{
                display: "grid",
                gridTemplateColumns: "24px minmax(0, 1fr)",
                gap: t.sp8 + t.sp2,
                padding: `${String(t.sp8 + t.sp2)}px 0`,
                borderTop: `1px solid ${t.separator}`,
              }}
            >
              <span
                style={{
                  fontWeight: t.weightSemibold,
                  color: t.textSecondary,
                  fontVariantNumeric: "tabular-nums",
                }}
              >
                {i + 1}
              </span>
              <span>
                <span
                  style={{ display: "block", fontWeight: t.weightSemibold }}
                >
                  {step.title}
                </span>
                <span style={{ display: "block", color: t.textSecondary }}>
                  {step.body}
                </span>
              </span>
            </li>
          ))}
        </ol>
      </DialogBody>
      <DialogFooter>
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </DialogFooter>
    </Dialog>
  );
}
