// src/ui/ConfirmBar.tsx — the inline stop-confirm bar shared by the run wizards.
//
// Replaces the footer with a "are you sure?" prompt + a dismiss (default
// "Continue") and a destructive confirm (default "Stop"). Extracted byte-for-byte
// from the Doctor + Leveling run bodies — only the message + the two callbacks
// varied between them.

import { useTheme } from "../theme/ThemeContext";
import { Button } from "./primitives";

export interface ConfirmBarProps {
  message: string;
  /** The dismiss button (keep running). */
  onCancel: () => void;
  /** The destructive confirm (stop). */
  onConfirm: () => void;
  /** default "Continue". */
  cancelLabel?: string;
  /** default "Stop". */
  confirmLabel?: string;
}

export function ConfirmBar({
  message,
  onCancel,
  onConfirm,
  cancelLabel = "Continue",
  confirmLabel = "Stop",
}: ConfirmBarProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flexShrink: 0,
        borderTop: `0.5px solid ${t.hairline}`,
        padding: "13px 22px",
        background: t.bgAlt,
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 14,
      }}
    >
      <span style={{ fontFamily: t.sans, fontSize: 12.5, color: t.ink2 }}>
        {message}
      </span>
      <div style={{ display: "flex", gap: 9 }}>
        <Button
          variant="ghost"
          small
          onClick={onCancel}
          style={{ height: 30, padding: "0 13px" }}
        >
          {cancelLabel}
        </Button>
        <Button
          variant="warn"
          small
          onClick={onConfirm}
          style={{ height: 30, padding: "0 14px" }}
        >
          {confirmLabel}
        </Button>
      </div>
    </div>
  );
}
