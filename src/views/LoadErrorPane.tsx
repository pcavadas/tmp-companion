// src/views/LoadErrorPane.tsx — the shared device-view load-error page: a danger
// PageState "Couldn't read your presets (or songs) from your Tone Master Pro" with one
// "Try again". Used by Level, Doctor, Copy and Songs. The failure's text is never drawn;
// it is in the log (the invoke wrapper, or `logUnlessCommandError`), so in a bug report.

import { Button } from "../ui/primitives";
import { PageState } from "../ui/PageState";

export interface LoadErrorPaneProps {
  /** What couldn't be read. */
  what: "presets" | "songs";
  onRetry: () => void;
}

export function LoadErrorPane({ what, onRetry }: LoadErrorPaneProps) {
  return (
    <PageState
      tone="danger"
      icon="alert"
      title={`Couldn't read your ${what} from your Tone Master Pro`}
      body="The details are saved for a bug report."
      actions={
        <Button variant="secondary" onClick={onRetry}>
          Try again
        </Button>
      }
    />
  );
}
