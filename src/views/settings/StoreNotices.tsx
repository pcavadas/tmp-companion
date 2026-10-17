// src/views/settings/StoreNotices.tsx — what a Settings page says about the store
// itself, above its content (Loudness targets, Instruments and Playback; About and
// updates reads nothing from the store): a spinner with "Reading your settings…" while
// it loads, a danger Notice with "Try again" when it couldn't be read, and a danger
// Notice naming what couldn't be saved. The backend's own text goes to the log, never
// to the page. A page with a Notice of its own passes its sentence as `alsoOnSaveError`
// while that Notice is due: a failed save then says both in the one danger Notice, so
// the page never shows two.

import { useTheme } from "../../theme/ThemeContext";
import { Spinner } from "../../ui/Spinner";
import { Button, Notice } from "../../ui/primitives";
import type { SettingsStore } from "./useSettingsStore";

export interface StoreNoticesProps {
  store: Pick<SettingsStore, "load" | "saveError" | "retryLoad">;
  /** The page's own Notice sentence, said after the failed-save one while both are
   *  due (the page leaves its own Notice out then). */
  alsoOnSaveError?: string;
}

export function StoreNotices({ store, alsoOnSaveError }: StoreNoticesProps) {
  const { t } = useTheme();
  const { load, saveError, retryLoad } = store;
  return (
    <>
      {load.kind === "loading" && (
        <div
          role="status"
          style={{
            display: "flex",
            alignItems: "center",
            gap: t.sp6,
            fontSize: t.fontBody,
            lineHeight: `${String(t.lineBody)}px`,
            color: t.textSecondary,
          }}
        >
          <Spinner size={12} strokeWidth={2.2} />
          Reading your settings…
        </div>
      )}
      {load.kind === "error" && (
        <Notice
          tone="danger"
          action={
            <Button variant="secondary" size="sm" onClick={retryLoad}>
              Try again
            </Button>
          }
        >
          Couldn&rsquo;t read your settings.
        </Notice>
      )}
      {saveError != null && (
        <Notice tone="danger">
          Couldn&rsquo;t save your {saveError}. Showing what&rsquo;s saved.
          {alsoOnSaveError != null && ` ${alsoOnSaveError}`}
        </Notice>
      )}
    </>
  );
}
