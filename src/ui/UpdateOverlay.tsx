// src/ui/UpdateOverlay.tsx — pure phase → DS Toast/Modal mapping for auto-update.
//
// Renders NOTHING at idle; every other phase maps to the existing Toast (which
// self-positions bottom-right) or the reviewing Modal. All state lives in
// useUpdater — this component only reads the UpdaterApi it's handed.

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
          title={`TMP Companion ${version} is ready`}
          message="A new update is available to download."
          actions={[
            { label: "Review", primary: true, onClick: u.review },
            { label: "Later", onClick: u.dismiss },
          ]}
          onDismiss={u.dismiss}
        />
      );

    case "reviewing": {
      const lines = u.notes != null ? formatReleaseNotes(u.notes) : [];
      const shown = lines.length > 0 ? lines : ["Bug fixes and improvements."];
      return (
        <Modal
          open
          headline={
            <>
              TMP Companion{" "}
              <span style={{ fontStyle: "italic" }}>{version}</span>
            </>
          }
          body={
            <>
              <div
                style={{
                  fontFamily: t.mono,
                  fontSize: t.fsData,
                  color: t.faint,
                  marginBottom: t.space5,
                }}
              >
                Updating from {u.currentVersion ?? ""}
              </div>
              {shown.map((line, i) => (
                <div
                  key={i}
                  style={{
                    fontFamily: t.sans,
                    fontSize: t.fsBody2,
                    lineHeight: 1.75,
                    color: t.ink2,
                  }}
                >
                  {"•  " + line}
                </div>
              ))}
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
          title={`Downloading ${version}…`}
          message="A minute or two left · feel free to keep working"
          percent={u.percent}
        />
      );

    case "ready":
      return (
        <Toast
          status="success"
          title="Ready to install"
          message={`Restart to finish updating to ${version}. Takes a few seconds — your presets and setlists are untouched.`}
          actions={[
            { label: "Restart now", primary: true, onClick: u.restart },
            { label: "Later", onClick: u.dismiss },
          ]}
          onDismiss={u.dismiss}
        />
      );

    case "error":
      return (
        <Toast
          status="error"
          title="Update didn’t finish"
          message={`Couldn’t finish downloading ${version}. Your presets and settings are untouched.`}
          actions={[
            { label: "Try again", primary: true, onClick: u.retry },
            { label: "Dismiss", onClick: u.dismiss },
          ]}
          onDismiss={u.dismiss}
        />
      );
  }
}
