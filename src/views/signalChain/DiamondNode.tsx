// src/views/signalChain/DiamondNode.tsx — the SPLIT / MIX / JOIN diamond of a split
// or join point, captioned below.

import { useTheme } from "../../theme/ThemeContext";
import { STRIP_LBL, STROKE_W_PX } from "./stripTypes";

export interface DiamondNodeProps {
  kind: "split" | "mix" | "join";
  ink: string;
  skeleton?: boolean;
}

export function DiamondNode({ kind, ink, skeleton }: DiamondNodeProps) {
  const { t } = useTheme();
  const label = kind === "split" ? "SPLIT" : kind === "mix" ? "MIX" : "JOIN";
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        width: 14,
        flexShrink: 0,
      }}
    >
      <div
        style={{
          flex: 1,
          minHeight: 14,
          position: "relative",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div
          style={{
            // border-box + no shrink so the 1.5px border stays INSIDE the 12×12
            // box and the flex cell can't squash the width — otherwise the
            // square renders a touch wider than tall (a lopsided diamond).
            boxSizing: "border-box",
            flexShrink: 0,
            width: 12,
            height: 12,
            transform: "rotate(45deg)",
            border: `${STROKE_W_PX} solid ${ink}`,
            borderRadius: 2,
          }}
        />
        {!skeleton && (
          <div
            style={{
              position: "absolute",
              top: "calc(50% + 12px)",
              left: -16,
              right: -16,
              textAlign: "center",
              fontFamily: t.mono,
              fontSize: 7.5,
              letterSpacing: "0.1em",
              color: t.stripInk,
              lineHeight: 1,
              whiteSpace: "nowrap",
            }}
          >
            {label}
          </div>
        )}
      </div>
      <div style={{ height: STRIP_LBL }} />
    </div>
  );
}
