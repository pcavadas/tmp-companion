// src/lib/useWindowWidth.ts — the window's inner width, live, for the few layouts that
// change shape with the window: Doctor's Results put the opened sound beside the list,
// Copy's target cards go to two columns, and the Catalog wraps its filter bar at 800 wide
// and stands the chosen model's details beside the wall from 1280.
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
