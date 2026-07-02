// src/views/LoadErrorPane.tsx — the shared device-view load-error pane.
//
// A warn banner + a "Try again" button. Used by LevelView / SongsView / CopyView,
// which previously each carried a byte-identical copy of this markup.

import { AlertBanner, Button } from "../ui/primitives";

export interface LoadErrorPaneProps {
  message: string;
  onRetry: () => void;
}

export function LoadErrorPane({ message, onRetry }: LoadErrorPaneProps) {
  return (
    <div style={{ padding: 28 }}>
      <AlertBanner style={{ marginBottom: 14 }}>{message}</AlertBanner>
      <Button variant="primary" onClick={onRetry}>
        Try again
      </Button>
    </div>
  );
}

export default LoadErrorPane;
