// src/views/LoadErrorPane.tsx — the shared device-view load-error pane.
//
// A warn banner + a "Try again" button. Used by LevelView / SongsView / CopyView,
// which previously each carried a byte-identical copy of this markup.

import { useTheme } from "../theme/ThemeContext";
import { AlertBanner, Button } from "../ui/primitives";

export interface LoadErrorPaneProps {
  message: string;
  onRetry: () => void;
}

export function LoadErrorPane({ message, onRetry }: LoadErrorPaneProps) {
  const { t } = useTheme();
  return (
    <div style={{ padding: t.space11 }}>
      <AlertBanner style={{ marginBottom: t.space7 }}>{message}</AlertBanner>
      <Button variant="primary" onClick={onRetry}>
        Try again
      </Button>
    </div>
  );
}

export default LoadErrorPane;
