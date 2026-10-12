// src/views/ConnectionHelpSheet.tsx — the "Connection help" explainer behind the
// no-device screen's secondary button (EmptyState). A calm, read-only sheet with the
// checks that actually get the unit talking to the app: power + a data cable, Pro
// Control closed (it holds the unit's one exclusive connection), a short wait while
// the unit boots, then another port or cable. The app connects on its own, so there
// is no Connect button to point at. No inputs, no persistence, no fetch.

import { useTheme } from "../theme/ThemeContext";
import { Dialog, DialogHeader, DialogBody, DialogFooter } from "../ui/Dialog";
import { Button } from "../ui/primitives";
import { Icon } from "../ui/Icon";

const STEPS: readonly { title: string; body: string }[] = [
  {
    title: "Plug it in and power it on",
    body: "Connect the Tone Master Pro to this computer with a USB-C cable, then switch the unit on. Some cables only charge; use one that carries data.",
  },
  {
    title: "Close Pro Control",
    body: "Only one app can talk to the unit at a time. Quit Fender Tone Master Pro Control while you use this app.",
  },
  {
    title: "Give it a few seconds",
    body: "The unit takes a moment to start up. This app connects on its own as soon as it is ready; there is nothing to click.",
  },
  {
    title: "Still nothing?",
    body: "Try another USB port or another cable. The app finds the unit on its own once it is plugged in.",
  },
];

export interface ConnectionHelpSheetProps {
  onClose: () => void;
}

export function ConnectionHelpSheet({ onClose }: ConnectionHelpSheetProps) {
  const { t } = useTheme();
  return (
    <Dialog size="md" onClose={onClose} label="Connection help">
      <DialogHeader>
        <span
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: t.space4,
          }}
        >
          <Icon name="cable" size={16} stroke={t.accentDeep} />
          <span style={{ fontFamily: t.serif, fontSize: 18, color: t.ink }}>
            Connecting your Tone Master Pro
          </span>
        </span>
      </DialogHeader>
      <DialogBody>
        <ol
          style={{
            margin: 0,
            padding: 0,
            listStyle: "none",
            display: "flex",
            flexDirection: "column",
            gap: t.space7,
          }}
        >
          {STEPS.map((step, i) => (
            <li key={step.title} style={{ display: "flex", gap: t.space6 }}>
              <span
                style={{
                  fontFamily: t.mono,
                  fontSize: t.fsData,
                  color: t.accentDeep,
                  minWidth: 14,
                }}
              >
                {i + 1}
              </span>
              <span>
                <span
                  style={{
                    display: "block",
                    fontFamily: t.sans,
                    fontSize: t.fsBody,
                    fontWeight: 500,
                    color: t.ink,
                  }}
                >
                  {step.title}
                </span>
                <span
                  style={{
                    display: "block",
                    marginTop: t.space2,
                    fontFamily: t.sans,
                    fontSize: t.fsUi,
                    lineHeight: 1.5,
                    color: t.ink2,
                  }}
                >
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
