// src/views/signalChain/EndpointNode.tsx — a strip's source jack (GUITAR / MIC/LINE)
// or sink jack (OUT / OUT 1 / OUT 2): a ringed DS icon over its mono caption.

import { useTheme } from "../../theme/ThemeContext";
import { Icon, type IconName } from "../../ui/Icon";
import { Skel } from "../../ui/Skeleton";
import {
  STRIP_LBL,
  type SinkType,
  type Size,
  type SourceType,
} from "./stripTypes";

// Endpoint glyphs come from the DS icon catalog (`<Icon>`), never hand-rolled SVG:
// the instrument input is the canonical `cable` plug, the mic is `mic`, the output is the
// `wave` meter. Keeps these nodes in the design system (one source of truth) instead of a
// private inline drawing that drifts (the old guitar SVG rendered as a paintbrush).
const ENDPOINT_ICON: Record<SourceType | "out", IconName> = {
  guitar: "cable",
  mic: "mic",
  out: "wave",
};

export interface EndpointNodeProps {
  /** A source jack (guitar / mic) or a sink jack (OUT / OUT 1 / OUT 2). */
  type: SourceType | SinkType;
  ink: string;
  skeleton?: boolean;
  size: Size;
}

/** The jack's glyph, caption and skeleton caption width. */
function endpointFace(type: SourceType | SinkType): {
  icon: SourceType | "out";
  label: string;
  skeletonLabelWidth: number;
} {
  switch (type) {
    case "guitar":
      return { icon: "guitar", label: "GUITAR", skeletonLabelWidth: 28 };
    case "mic":
      return { icon: "mic", label: "MIC/LINE", skeletonLabelWidth: 34 };
    case "out1":
      return { icon: "out", label: "OUT 1", skeletonLabelWidth: 28 };
    case "out2":
      return { icon: "out", label: "OUT 2", skeletonLabelWidth: 28 };
    case "out":
      return { icon: "out", label: "OUT", skeletonLabelWidth: 22 };
  }
}

export function EndpointNode({ type, ink, skeleton, size }: EndpointNodeProps) {
  const { t } = useTheme();
  const { icon, label, skeletonLabelWidth } = endpointFace(type);
  const artH = size === "sm" ? 46 : 58;
  const fs = size === "sm" ? 7 : 8;
  if (skeleton) {
    return (
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          width: 34,
          flexShrink: 0,
        }}
      >
        <div
          style={{
            flex: 1,
            minHeight: artH,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <Skel w={34} h={34} r={999} />
        </div>
        <div
          style={{
            height: STRIP_LBL,
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "center",
            paddingTop: t.space3,
          }}
        >
          <Skel w={skeletonLabelWidth} h={6} r={3} />
        </div>
      </div>
    );
  }
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        width: 34,
        flexShrink: 0,
      }}
    >
      <div
        style={{
          flex: 1,
          minHeight: artH,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div
          style={{
            // border-box so the 1.5px border is INSIDE the 34px box: otherwise the
            // ring's outer width (34 + 2×1.5 = 37) exceeds its 34px flex column and
            // flex-shrink squashes the width back to 34 while height stays 34 — an
            // oval that reads as "circle cropped left & right". border-box → no shrink.
            boxSizing: "border-box",
            width: 34,
            height: 34,
            borderRadius: 999,
            border: `1.5px solid ${ink}`,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: ink,
          }}
        >
          <Icon name={ENDPOINT_ICON[icon]} size={17} stroke={ink} />
        </div>
      </div>
      <div
        style={{
          height: STRIP_LBL,
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "center",
          paddingTop: t.space2,
        }}
      >
        <span
          style={{
            fontFamily: t.mono,
            fontSize: fs,
            letterSpacing: "0.1em",
            color: t.stripInk,
            lineHeight: 1,
            whiteSpace: "nowrap",
          }}
        >
          {label}
        </span>
      </div>
    </div>
  );
}
