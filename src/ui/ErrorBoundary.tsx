// TMP Companion — top-level ErrorBoundary.
//
// Defense-in-depth for render crashes. A thrown error in any descendant (e.g. a
// Rules-of-Hooks violation) would otherwise unmount the entire React root and
// leave a blank window with no on-disk trace. This boundary instead:
//   • logs the error + component stack to the Tauri log file (src/lib/log.ts), and
//   • renders a themed fallback with a Reload button rather than a blank tree.
// Mounted inside ThemeProvider (see main.tsx) so the Fallback can use the theme.

import { Component, type ErrorInfo, type ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { logError } from "../lib/log";
import { errMsg } from "../lib/format";

interface Props {
  children: ReactNode;
}

// JS can throw ANY value (a bare string from a rejected command, an object), so
// the caught value is kept as `unknown` and rendered through `errMsg` — typing
// it `Error` once rendered a non-Error throw as an empty box.
interface State {
  caught: { value: unknown } | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { caught: null };

  static getDerivedStateFromError(value: unknown): State {
    return { caught: { value } };
  }

  componentDidCatch(value: unknown, info: ErrorInfo): void {
    const stack = value instanceof Error ? (value.stack ?? "") : "";
    logError(
      `React render crash: ${errMsg(value)}\n${stack}\nComponent stack:${info.componentStack ?? ""}`,
    );
  }

  // Soft recovery: clear the error and re-mount the children in place. Enough for
  // a transient render error, and far cheaper than a full reload (which re-runs
  // the device handshake). The Reload button remains the harder fallback.
  reset = (): void => {
    this.setState({ caught: null });
  };

  render(): ReactNode {
    if (this.state.caught) {
      return (
        <Fallback
          message={errMsg(this.state.caught.value)}
          onReset={this.reset}
        />
      );
    }
    return this.props.children;
  }
}

function Fallback({
  message,
  onReset,
}: {
  message: string;
  onReset: () => void;
}) {
  const { t } = useTheme();
  return (
    <div
      role="alert"
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "flex-start",
        gap: t.space7,
        height: "100%",
        width: "100%",
        boxSizing: "border-box",
        overflow: "auto",
        padding: t.space12,
        background: t.bg,
        color: t.ink,
        fontFamily: t.sans,
      }}
    >
      <div style={{ fontSize: 16, fontWeight: 600 }}>Something went wrong</div>
      <div style={{ fontSize: 13, color: t.mutedInk, maxWidth: 560 }}>
        The interface hit an unexpected error and stopped rendering. The details
        have been written to the log file. Reloading usually recovers.
      </div>
      <pre
        style={{
          margin: 0,
          padding: `${String(t.space5)}px ${String(t.space6)}px`,
          maxWidth: "100%",
          overflowX: "auto",
          border: `0.5px solid ${t.warn}`,
          borderLeft: `2px solid ${t.warn}`,
          borderRadius: t.rMd,
          background: t.accentSoft,
          color: t.warn,
          fontFamily: t.mono,
          fontSize: 12,
        }}
      >
        {message}
      </pre>
      <div style={{ display: "flex", gap: t.space5 }}>
        <button
          type="button"
          onClick={onReset}
          style={{
            padding: `${String(t.space4)}px ${String(t.space8)}px`,
            border: "none",
            borderRadius: t.rMd,
            background: t.ink,
            color: t.bg,
            fontFamily: t.sans,
            fontSize: 13,
            cursor: "pointer",
          }}
        >
          Try again
        </button>
        <button
          type="button"
          onClick={() => {
            window.location.reload();
          }}
          style={{
            padding: `${String(t.space4)}px ${String(t.space8)}px`,
            border: `0.5px solid ${t.hairline}`,
            borderRadius: t.rMd,
            background: t.bgAlt,
            color: t.ink,
            fontFamily: t.sans,
            fontSize: 13,
            cursor: "pointer",
          }}
        >
          Reload
        </button>
      </div>
    </div>
  );
}
