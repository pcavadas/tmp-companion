// src/views/level/SummaryRow.tsx — one Summary row (look B): a status mark, then the
// preset (600) and the sound (secondary) with at most one Badge, and under them the row's
// sentences (13/18). A footswitch whose effect is off in its scene adds "Level it in
// scene" with a small dropdown (the normal look: it doesn't block Done). Leveled and Not
// reached rows end in a 12 px word at the right; Need you rows have none.

import { useTheme } from "../../theme/ThemeContext";
import { Badge } from "../../ui/Badge";
import { Select } from "../../ui/Select";
import { StatusMark } from "./StatusMark";
import type { SummaryRowModel } from "./summaryModel";

export interface SummaryRowProps {
  row: SummaryRowModel;
  /** A footswitch's scene picked here (`value` is "base" or a scene index). */
  onScene: (key: string, value: string) => void;
}

export function SummaryRow({ row, onScene }: SummaryRowProps) {
  const { t } = useTheme();
  const detailed = row.messages.length > 0 || row.scene != null;
  return (
    <div
      data-summary-row={row.hookKey ?? undefined}
      data-base-boost={row.baseBoost ?? undefined}
      style={{
        borderTop: `1px solid ${t.separator}`,
        minHeight: t.rowControls,
        boxSizing: "border-box",
        display: "flex",
        alignItems: detailed ? "flex-start" : "center",
        gap: t.sp8 + t.sp2,
        padding: detailed
          ? `${String(t.sp8 + 1)}px ${String(t.sp12)}px`
          : `0 ${String(t.sp12)}px`,
        fontSize: t.fontBody,
        lineHeight: `${String(t.lineBody)}px`,
        color: t.text,
      }}
    >
      <span
        style={{ display: "flex", flexShrink: 0, paddingTop: detailed ? 1 : 0 }}
      >
        <StatusMark kind={row.mark} />
      </span>
      <div
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          flexDirection: "column",
          gap: t.sp2,
        }}
      >
        <span
          style={{
            display: "flex",
            alignItems: "center",
            gap: t.sp8,
            minWidth: 0,
          }}
        >
          <span style={{ fontWeight: t.weightSemibold, whiteSpace: "nowrap" }}>
            {row.preset}
          </span>
          <span
            style={{
              color: t.textSecondary,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
              minWidth: 0,
            }}
          >
            {row.sound}
          </span>
          {row.badge && <Badge tone={row.badge.tone}>{row.badge.text}</Badge>}
        </span>
        {row.messages.map((m) => (
          <span key={m}>{m}</span>
        ))}
        {row.scene && (
          <span
            style={{
              display: "flex",
              alignItems: "center",
              gap: t.sp8,
              marginTop: t.sp4,
              fontSize: t.fontCaption,
              color: t.textSecondary,
            }}
          >
            Level it in scene
            <Select
              size="sm"
              width={120}
              label={row.scene.label}
              value={row.scene.value}
              options={row.scene.options}
              onChange={(value) => {
                if (row.scene) onScene(row.scene.key, value);
              }}
              tid={row.scene.tid}
            />
          </span>
        )}
      </div>
      {row.word && (
        <span
          style={{
            flexShrink: 0,
            fontSize: t.fontCaption,
            whiteSpace: "nowrap",
            color: row.word === "On target" ? t.good : t.textSecondary,
          }}
        >
          {row.word}
        </span>
      )}
    </div>
  );
}
