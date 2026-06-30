// src/views/songs/shared.tsx — small shared bits for the Songs view split.
// PRIVATE: not re-exported from index.ts; must not import any sub-component.
import { useTheme, useStyles } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";

/** Inline-edit draft of a song (the create/edit form's working values). */
export interface SongDraft {
  name: string;
  notes: string;
  bpm: number | null;
}

export const SONG_COLS = "34px 1fr 72px 72px";
export const LIST_COLS = "26px 34px 1fr 70px 30px";

// (songBpm / bpmStr / plainInput pure helpers moved to ./songUtil.)

// Square hairline icon button (the prototype's SIconBtn).
export interface IconBtnProps {
  icon: Parameters<typeof Icon>[0]["name"];
  title: string;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
  size?: number;
}

export function IconBtn({
  icon,
  title,
  onClick,
  danger,
  disabled,
  size = 26,
}: IconBtnProps) {
  const { t } = useTheme();
  const s = useStyles();
  return (
    <span
      onClick={disabled ? undefined : onClick}
      title={title}
      style={{
        ...s.iconBtnBox({ box: size, radius: t.rMd, danger }),
        opacity: disabled ? 0.4 : 1,
        cursor: disabled ? "default" : "pointer",
      }}
    >
      <Icon name={icon} size={14} stroke={danger ? t.warn : t.ink2} />
    </span>
  );
}
