// src/views/settings/NeedsDevicePill.tsx — the disabled "Needs device" pill an
// InstrumentRow shows in place of its calibrate control while the unit is absent.

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";

// Disabled "Needs device" pill — shown in place of the calibrate control when the
// unit is disconnected (calibration listens through the device input).
export function NeedsDevicePill() {
  const { t } = useTheme();
  return (
    <span
      title="Connect the Tone Master Pro to calibrate"
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: t.space3,
        height: 28,
        boxSizing: "border-box",
        fontFamily: t.sans,
        fontSize: t.fsUi,
        fontWeight: 500,
        color: t.faint,
        background: "transparent",
        border: `0.5px solid ${t.hairline}`,
        borderRadius: t.rMd,
        padding: `0 ${String(t.space5)}px`,
        whiteSpace: "nowrap",
        cursor: "not-allowed",
        opacity: 0.75,
      }}
    >
      <Icon name="cable" size={13} stroke={t.faint} />
      Needs device
    </span>
  );
}
