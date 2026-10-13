// src/views/signalChain/Wire.tsx — the horizontal wire between two strip columns.

import { STRIP_LBL } from "./stripTypes";

export interface WireProps {
  ink: string;
  w?: number;
  // A fork/join lane's own row is narrower than its longer sibling — grow
  // instead of a fixed width so the trailing endpoint node (SinkNode/
  // SourceNode) still lines up across lanes, with the drawn line stretching
  // to meet it rather than leaving a dangling gap.
  grow?: boolean;
}

export function Wire({ ink, w = 18, grow }: WireProps) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        ...(grow ? { flex: 1, minWidth: w } : { width: w, flexShrink: 0 }),
      }}
    >
      <div style={{ flex: 1, display: "flex", alignItems: "center" }}>
        <div style={{ width: "100%", height: 1.5, background: ink }} />
      </div>
      <div style={{ height: STRIP_LBL }} />
    </div>
  );
}
