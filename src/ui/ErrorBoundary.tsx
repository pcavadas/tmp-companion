// TMP Companion — top-level ErrorBoundary.
//
// Defense-in-depth for render crashes. A thrown error in any descendant (e.g. a
// Rules-of-Hooks violation) would otherwise unmount the entire React root and
// leave a blank window with no on-disk trace. This boundary instead:
//   • logs the error + component stack to the Tauri log file (src/lib/log.ts), and
//   • renders the "Something went wrong" PageState (danger) with Try again and Reload,
//     rather than a blank tree. The caught message is NOT drawn (it is in the log, and so
//     in a bug report): the page says only that the details were saved.
// Two mounts: one per tab in App.tsx (keyed on the tab, so the toolbar survives) and the
// top-level one in main.tsx (the same page filling the window). Mounted inside
// ThemeProvider (see main.tsx) so the fallback can use the theme.

import { Component, type ErrorInfo, type ReactNode } from "react";

import { logError } from "../lib/log";
import { errMsg } from "../lib/format";
import { PageState } from "./PageState";
import { Button } from "./primitives";

interface Props {
  children: ReactNode;
}

// JS can throw ANY value (a bare string from a rejected command, an object), so the
// caught value is kept as `unknown` and logged through `errMsg` — typing it `Error` once
// logged a non-Error throw as nothing.
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
    if (this.state.caught) return <Fallback onReset={this.reset} />;
    return this.props.children;
  }
}

function Fallback({ onReset }: { onReset: () => void }) {
  return (
    <div
      role="alert"
      style={{ flex: 1, minHeight: 0, height: "100%", display: "flex" }}
    >
      <PageState
        tone="danger"
        icon="alert"
        title="Something went wrong"
        body="The details are saved for a bug report."
        actions={
          <>
            <Button variant="primary" onClick={onReset}>
              Try again
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                window.location.reload();
              }}
            >
              Reload
            </Button>
          </>
        }
      />
    </div>
  );
}
