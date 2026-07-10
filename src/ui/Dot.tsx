// src/ui/Dot.tsx — a solid status dot (the round colored pip).

export interface DotProps {
  /** default 7. */
  size?: number;
  color: string;
}

export function Dot({ size = 7, color }: DotProps) {
  return (
    <span
      style={{
        width: size,
        height: size,
        borderRadius: 999,
        background: color,
        flexShrink: 0,
        display: "inline-block",
      }}
    />
  );
}
