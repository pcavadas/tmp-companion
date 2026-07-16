// src/views/doctor/CutThroughCard.tsx — the "does this cut through the mix?"
// ESTIMATE card (`DoctorSoundResult.cutThrough`): a presence-contrast reading
// against the measured factory-bank distribution. Render-only — it's not a
// diagnosis (no rx, no apply, never gates anything).

import { useTheme } from "../../theme/ThemeContext";
import { doctorCard } from "./severity";
import { signedDb } from "../../lib/format";
import type { CutThrough } from "../../lib/types";

export interface CutThroughCardProps {
  cutThrough: CutThrough;
}

export function CutThroughCard({ cutThrough }: CutThroughCardProps) {
  const { t } = useTheme();
  const { contrastDb, factoryPercentile, advisory } = cutThrough;

  return (
    <div
      style={doctorCard(
        t,
        advisory ? { border: t.warnBorder, background: t.warnSoft } : undefined,
      )}
    >
      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsLabel,
          fontWeight: 600,
          color: advisory ? t.warn : t.ink2,
        }}
      >
        Cut-through (estimated)
      </div>
      <div
        style={{
          fontFamily: t.mono,
          fontSize: 12.5,
          color: advisory ? t.warn : t.mutedInk,
          marginTop: t.space2,
        }}
      >
        {signedDb(contrastDb)} dB
        {factoryPercentile != null &&
          ` · louder presence than ${String(Math.round(factoryPercentile))}% of the factory bank`}
      </div>
      {advisory && (
        <div
          style={{
            fontFamily: t.sans,
            fontSize: t.fsLabel,
            color: t.warn,
            marginTop: t.space3,
            lineHeight: 1.5,
          }}
        >
          May struggle to cut through a dense mix.
        </div>
      )}
    </div>
  );
}

export default CutThroughCard;
