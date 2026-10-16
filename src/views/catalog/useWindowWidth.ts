// src/views/catalog/useWindowWidth.ts — the window's inner width, live. The Catalog
// changes layout with the window: at 800 wide the filter bar wraps under the search,
// and from 1280 the chosen model's details stand beside the wall instead of under it.

import { useSyncExternalStore } from "react";

function subscribe(onChange: () => void): () => void {
  window.addEventListener("resize", onChange);
  return () => {
    window.removeEventListener("resize", onChange);
  };
}

const width = () => window.innerWidth;

export function useWindowWidth(): number {
  return useSyncExternalStore(subscribe, width);
}
