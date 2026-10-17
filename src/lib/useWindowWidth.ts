// src/lib/useWindowWidth.ts — the window's inner width, live, for the few layouts that
// change shape on a wide window (Doctor's Results put the opened sound beside the list).
//
// A `useSyncExternalStore` over the window's `resize` event: no effect, no state, and every
// reader sees the same width in the same render.

import { useSyncExternalStore } from "react";

function subscribe(onChange: () => void): () => void {
  window.addEventListener("resize", onChange);
  return () => {
    window.removeEventListener("resize", onChange);
  };
}

function snapshot(): number {
  return window.innerWidth;
}

/** The window's inner width in px, re-read on every resize. */
export function useWindowWidth(): number {
  return useSyncExternalStore(subscribe, snapshot);
}
