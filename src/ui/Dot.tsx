// src/ui/Dot.tsx — a status dot (the round colored pip), solid or hollow.

export interface DotProps {
  /** default 7. */
  size?: number;
  color: string;
  /** Outline-only pip — the "was queued, never ran" state (a stopped run's tail). */
  hollow?: boolean;
}

export function Dot({ size = 7, color, hollow }: DotProps) {
  return (
    <span
      style={{
        width: size,
        height: size,
        borderRadius: 999,
        background: hollow ? "transparent" : color,
        border: hollow ? `1px solid ${color}` : undefined,
        boxSizing: "border-box",
        flexShrink: 0,
        display: "inline-block",
      }}
    />
  );
}
