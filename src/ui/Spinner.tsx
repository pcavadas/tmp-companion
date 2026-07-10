// src/ui/Spinner.tsx — a spinning Icon.
//
// Wraps any Icon in the app-wide `.tmp-spin` keyframe (defined in index.html)
// so a single sweep animates everywhere. Defaults to the "spinner" glyph; all
// Icon props pass through.

import type { IconName } from "./Icon";
import { Icon } from "./Icon";

export interface SpinnerProps {
  /** default "spinner". */
  name?: IconName;
  size?: number;
  stroke?: string;
  strokeWidth?: number;
}

export function Spinner({
  name = "spinner",
  size,
  stroke,
  strokeWidth,
}: SpinnerProps) {
  return (
    <span className="tmp-spin" style={{ display: "inline-flex" }}>
      <Icon name={name} size={size} stroke={stroke} strokeWidth={strokeWidth} />
    </span>
  );
}
