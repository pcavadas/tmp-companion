// src/views/songs/SongsPage.tsx — the Songs tab's content column (look B): the shared
// `PageColumn` (title row over a scrolling body), 720 px at most and centred.
// While a song or setlist write is queued or running on the unit (`busy`), the title
// row says so: a write takes a moment, and its form or dialog has already closed.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { PageColumn } from "../../ui/PageColumn";
import { Spinner } from "../../ui/Spinner";

/** The widest the Songs content column grows (Settings uses 640). */
const SONGS_COLUMN_MAX = 720;

/** Shown beside the title while a write to the unit is queued or running. */
export const SAVING_NOTE = "Saving to your Tone Master Pro…";

export interface SongsPageProps {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  /** A song or setlist write is queued or running on the unit. */
  busy?: boolean;
  children: ReactNode;
}

export function SongsPage({ actions, busy = false, ...rest }: SongsPageProps) {
  const { t } = useTheme();
  return (
    <PageColumn
      maxWidth={SONGS_COLUMN_MAX}
      actions={
        <>
          <span
            role="status"
            aria-live="polite"
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: t.sp6,
              fontSize: t.fontCaption,
              lineHeight: `${String(t.lineCaption)}px`,
              color: t.textSecondary,
              whiteSpace: "nowrap",
            }}
          >
            {busy && (
              <>
                <Spinner size={14} stroke={t.textSecondary} />
                {SAVING_NOTE}
              </>
            )}
          </span>
          {actions}
        </>
      }
      {...rest}
    />
  );
}
