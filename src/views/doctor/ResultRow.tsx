// src/views/doctor/ResultRow.tsx — one row of Doctor's Results (look B): a 36 px line with
// the status mark (a danger or caution dot for findings, the good check for a clean sound,
// a caution dot for one that couldn't be checked, a hollow dot for one never reached), the
// preset's name and the sound's, the caption (the findings in order — "Muddy, Dark" — or
// the row's state) and, when it opens, a chevron. No loudness, no band picture, no venue
// icons on a row: those live inside an opened sound.
//
// A row that opens is ONE button (the whole line), so a click anywhere on it opens or
// closes it, keyboard and VoiceOver included; a row that doesn't open is plain text. On a
// wide window the row picks the sound shown beside the list (`aria-current`), instead of
// opening under itself.

import { useTheme } from "../../theme/ThemeContext";
import { plainButton } from "../../theme/tokens";
import { Icon } from "../../ui/Icon";
import { Row } from "../../ui/List";
import { StatusMark } from "../level/StatusMark";
import type { ResultRow as ResultRowModel } from "./resultsModel";

export interface ResultRowProps {
  row: ResultRowModel;
  /** Open under the row, or (wide) shown beside the list. */
  open: boolean;
  /** Wide window: the row picks what the side panel shows. */
  picks?: boolean;
  onToggle: (id: string) => void;
}

export function ResultRow({
  row,
  open,
  picks = false,
  onToggle,
}: ResultRowProps) {
  const { t } = useTheme();
  const name = (
    <span
      style={{
        flex: 1,
        minWidth: 0,
        whiteSpace: "nowrap",
        overflow: "hidden",
        textOverflow: "ellipsis",
        fontWeight: t.weightMedium,
      }}
    >
      {row.presetName}{" "}
      <span style={{ color: t.textSecondary, fontWeight: t.weightRegular }}>
        {row.soundName}
      </span>
    </span>
  );
  const caption = (
    <span
      style={{
        fontSize: t.fontCaption,
        whiteSpace: "nowrap",
        overflow: "hidden",
        textOverflow: "ellipsis",
        minWidth: 0,
        flexShrink: 1,
        // Beside the side panel the column is narrow: keep room for the sound's name.
        maxWidth: picks ? "45%" : "60%",
        color:
          row.captionTone === "good"
            ? t.good
            : row.captionTone === "text"
              ? t.text
              : t.textSecondary,
      }}
    >
      {row.caption}
    </span>
  );

  if (!row.expandable) {
    return (
      <Row data-result-row={row.id}>
        <StatusMark kind={row.mark} />
        {name}
        {caption}
      </Row>
    );
  }

  return (
    <Row data-result-row={row.id} selected={open} style={{ padding: 0 }}>
      <button
        type="button"
        aria-expanded={picks ? undefined : open}
        aria-current={picks && open ? true : undefined}
        onClick={() => {
          onToggle(row.id);
        }}
        style={plainButton({
          flex: 1,
          minWidth: 0,
          height: "100%",
          display: "flex",
          alignItems: "center",
          gap: t.sp8 + 2,
          padding: `0 ${String(t.sp12)}px`,
          textAlign: "left",
          fontFamily: t.fontSans,
          fontSize: t.fontBody,
          color: t.text,
        })}
      >
        <StatusMark kind={row.mark} />
        {name}
        {caption}
        <span
          aria-hidden
          style={{
            width: 18,
            flexShrink: 0,
            display: "flex",
            justifyContent: "center",
          }}
        >
          <Icon
            name={open && !picks ? "chev-down" : "chev-right"}
            size={12}
            stroke={t.textSecondary}
            strokeWidth={2.2}
          />
        </span>
      </button>
    </Row>
  );
}
