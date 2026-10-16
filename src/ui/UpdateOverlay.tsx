// src/ui/UpdateOverlay.tsx — pure phase → DS Toast/Modal mapping for auto-update (look B).
//
// Renders NOTHING at idle; every other phase maps to a bottom-centre Toast with at most
// one link action and a close button, or (reviewing) the Modal with the release notes.
// All state lives in useUpdater — this component only reads the UpdaterApi it's handed.

import { useTheme } from "../theme/ThemeContext";
import { Modal, Toast } from "./primitives";
import { formatReleaseNotes } from "../lib/useUpdater";
import type { UpdaterApi } from "../lib/useUpdater";

interface UpdateOverlayProps {
  u: UpdaterApi;
}

export function UpdateOverlay({ u }: UpdateOverlayProps) {
  const { t } = useTheme();
  const version = u.version ?? "";

  switch (u.phase) {
    case "idle":
      return null;

    case "available":
      return (
        <Toast
          status="available"
          title={`TMP Companion ${version} is available.`}
          actions={[
            { label: "See what's new", primary: true, onClick: u.review },
          ]}
          onDismiss={u.dismiss}
          dismissLabel="Later"
        />
      );

    case "reviewing": {
      const lines = u.notes != null ? formatReleaseNotes(u.notes) : [];
      const shown = lines.length > 0 ? lines : ["Bug fixes and improvements."];
      return (
        <Modal
          open
          label={`TMP Companion ${version}`}
          headline={`TMP Companion ${version}`}
          body={
            <>
              {u.currentVersion != null && (
                <div
                  style={{
                    fontSize: t.fontCaption,
                    lineHeight: `${String(t.lineCaption)}px`,
                    color: t.textSecondary,
                    marginBottom: t.sp8,
                  }}
                >
                  You have {u.currentVersion}.
                </div>
              )}
              <ul style={{ margin: 0, paddingLeft: t.sp16 }}>
                {shown.map((line, i) => (
                  <li key={i}>{line}</li>
                ))}
              </ul>
            </>
          }
          applyLabel="Update now"
          cancelLabel="Later"
          applyVariant="primary"
          onApply={u.startDownload}
          onCancel={u.cancelReview}
        />
      );
    }

    case "downloading":
      return (
        <Toast
          status="downloading"
          title={`Downloading TMP Companion ${version}…`}
          message="You can keep working."
          percent={u.percent}
        />
      );

    case "ready":
      return (
        <Toast
          status="success"
          title={`TMP Companion ${version} is ready to install.`}
          actions={[
            { label: "Restart now", primary: true, onClick: u.restart },
          ]}
          onDismiss={u.dismiss}
          dismissLabel="Later"
        />
      );

    case "error":
      return (
        <Toast
          status="error"
          title="The update didn't download."
          message="Your presets and settings aren't touched."
          actions={[{ label: "Try again", primary: true, onClick: u.retry }]}
          onDismiss={u.dismiss}
        />
      );
  }
}
