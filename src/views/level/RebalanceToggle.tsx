// src/views/level/RebalanceToggle.tsx — the Set up page's opt-in run option: equalize
// a path-MERGE scene's two parallel-amp lanes before leveling. A real switch button.

import { useTheme } from "../../theme/ThemeContext";
import { Toggle } from "../../ui/primitives";
import { ByEarChip } from "../overlays/ByEarChip";

export interface RebalanceToggleProps {
  on: boolean;
  onToggle: () => void;
}

export function RebalanceToggle({ on, onToggle }: RebalanceToggleProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flexShrink: 0,
        padding: `${String(t.space5)}px ${String(t.space7)}px`,
        background: t.bgAlt,
        borderTop: `0.5px solid ${t.hairline}`,
        borderRadius: t.rCard,
      }}
    >
      <button
        type="button"
        role="switch"
        aria-checked={on}
        onClick={onToggle}
        style={{
          display: "flex",
          alignItems: "flex-start",
          gap: t.space6,
          cursor: "pointer",
          userSelect: "none",
          width: "100%",
          textAlign: "left",
          background: "none",
          border: "none",
          padding: 0,
          font: "inherit",
          color: "inherit",
        }}
      >
        <span aria-hidden style={{ paddingTop: t.space1, flexShrink: 0 }}>
          <Toggle on={on} />
        </span>
        <div style={{ minWidth: 0 }}>
          <div
            style={{
              fontFamily: t.sans,
              fontSize: 13,
              fontWeight: 500,
              color: t.ink,
            }}
          >
            Even out parallel amps
          </div>
          <div
            style={{
              fontFamily: t.sans,
              fontSize: 11,
              lineHeight: 1.5,
              color: t.mutedInk,
              marginTop: t.space1,
              textWrap: "pretty",
            }}
          >
            When a sound blends two amps into one, match their levels before
            leveling. No effect on single-amp sounds.
          </div>
          {on && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: t.space4,
                marginTop: t.space4,
              }}
            >
              <ByEarChip />
              <span
                style={{
                  fontFamily: t.sans,
                  fontSize: 11,
                  color: t.mutedInk,
                }}
              >
                Rebalanced sounds come back flagged for a listen.
              </span>
            </div>
          )}
        </div>
      </button>
    </div>
  );
}
