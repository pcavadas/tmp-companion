// src/main.tsx — React entry point.
//
// Mounts the single-window App inside React.StrictMode + the shared ThemeProvider
// (the one authoritative provider — App no longer wraps its own).

import React from "react";
import ReactDOM from "react-dom/client";

import { ThemeProvider } from "./theme/ThemeProvider";
import { ErrorBoundary } from "./ui/ErrorBoundary";
import { installGlobalErrorLogging } from "./lib/log";
import App from "./App";

// Forward uncaught errors + unhandled promise rejections to the Tauri log file.
installGlobalErrorLogging();

const rootElement = document.getElementById("root");
if (!rootElement) throw new Error('Root element "#root" not found');

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </ThemeProvider>
  </React.StrictMode>,
);
