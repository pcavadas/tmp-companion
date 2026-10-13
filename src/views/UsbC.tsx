// src/views/UsbC.tsx — the inline mono "USB-C" token used inside EmptyState bodies.

import { useTheme } from "../theme/ThemeContext";

/** Inline mono USB-C token, for use inside EmptyState bodies. */
export function UsbC() {
  const { t } = useTheme();
  return (
    <span style={{ fontFamily: t.mono, fontSize: t.fsControl, color: t.ink2 }}>
      USB-C
    </span>
  );
}
