// src/views/doctor/DiagnosisChip.tsx — a toggle-to-expand diagnosis pill. Open
// state is owned by the results page (multiple chips can be open at once); the
// opt-in frequency picture is a local reveal. The expanded panel carries the
// plain-language explanation, the Hz/dB detail, the BandMeter, and one
// PrescriptionCard per fix.

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { microLabel } from "../../theme/tokens";
import { BandMeter } from "./BandMeter";
import { PrescriptionCard } from "./PrescriptionCard";
import { diagSevLabel, sevTone } from "./severity";
import type { DoctorDiag } from "../../lib/types";

export interface DiagnosisChipProps {
  diag: DoctorDiag;
  /** The sound's real six-band balance — drives the opt-in BandMeter. */
  balanceDb: number[];
  listIndex: number;
  presetName: string;
  open: boolean;
  onToggle: () => void;
}

export function DiagnosisChip({
  diag,
  balanceDb,
  listIndex,
  presetName,
  open,
  onToggle,
}: DiagnosisChipProps) {
  const { t } = useTheme();
  const [showBands, setShowBands] = useState(false);
  const tone = sevTone(t, diag.sev);
  const banded = diag.bands.length > 0;

  return (
    <div>
      <button
        type="button"
        aria-expanded={open}
        onClick={onToggle}
        title="Show what this means and how to fix it"
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 6,
          border: `0.5px solid ${tone.fg}`,
          background: open ? t.bg : tone.soft,
          borderRadius: t.rPill,
          padding: "4px 10px",
          cursor: "pointer",
          color: tone.fg,
          fontFamily: t.sans,
          fontSize: t.fsLabel,
        }}
      >
        <Icon name="warn-tri" size={12} stroke={tone.fg} />
        <span>{diag.label}</span>
        <span
          style={{
            display: "inline-flex",
            transform: open ? "rotate(90deg)" : "none",
            transition: "transform 0.12s",
          }}
        >
          <Icon name="chev-right" size={12} stroke={tone.fg} />
        </span>
      </button>
      {open && (
        <div
          style={{
            marginTop: 8,
            border: `0.5px solid ${tone.fg}`,
            background: tone.soft,
            borderRadius: t.rMd,
            padding: 12,
          }}
        >
          <div style={{ ...microLabel(t), color: tone.fg }}>
            {diagSevLabel(diag.sev)}
          </div>
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsBody,
              color: t.ink2,
              marginTop: 6,
              lineHeight: 1.5,
            }}
          >
            {diag.explain}
          </div>
          <div
            style={{
              fontFamily: t.mono,
              fontSize: t.fsData,
              color: t.mutedInk,
              marginTop: 6,
            }}
          >
            {diag.detail}
          </div>
          {banded && (
            <>
              <button
                type="button"
                aria-expanded={showBands}
                onClick={() => {
                  setShowBands((v) => !v);
                }}
                style={{
                  display: "inline-block",
                  marginTop: 10,
                  padding: 0,
                  border: 0,
                  background: "transparent",
                  cursor: "pointer",
                  fontFamily: t.sans,
                  fontSize: t.fsLabel,
                  color: tone.fg,
                  textDecoration: "underline dotted",
                  textUnderlineOffset: 3,
                }}
              >
                {showBands ? "Hide the frequencies" : "Show the frequencies"}
              </button>
              {showBands && (
                <BandMeter
                  balanceDb={balanceDb}
                  bands={diag.bands}
                  sev={diag.sev}
                />
              )}
            </>
          )}
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 10,
              marginTop: 12,
            }}
          >
            {diag.rx.map((rx, i) => (
              <PrescriptionCard
                key={`${rx.kind}-${String(i)}`}
                rx={rx}
                listIndex={listIndex}
                presetName={presetName}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export default DiagnosisChip;
