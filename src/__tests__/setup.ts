// src/__tests__/setup.ts — Vitest global setup (referenced by vitest.config.ts).
//
// 1. jest-dom matchers for RTL.
// 2. A default mock of @tauri-apps/api/core so screen smoke tests don't hit Tauri.
//    `invoke(command, args)` resolves the command's typed EMPTY default
//    (`DEFAULT_HANDLERS` in ./commandMocks) — enough for a screen to mount + render
//    its empty state without throwing. Individual tests override commands with
//    `mockCommands({ … })` from ./commandMocks.
// 3. A default mock of @tauri-apps/api/event: `listen` records each callback in
//    `liveListeners` (./commandMocks) and resolves an unlisten. A file that pushes
//    live events takes the registry with `mockLiveEvents()`.
// 4. An `SVGElement.getBBox` stub: jsdom has none, and `HalfStackArt` measures its
//    head and cab with it in a layout effect, so any render of one would throw.
// 5. An `Element.scrollIntoView` no-op: jsdom has none, and Set up's footer link brings
//    the row it points at into view before opening its dropdown.

import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

import {
  liveListeners,
  resolveCommand,
  type LiveListener,
} from "./commandMocks";

// This jsdom config doesn't provide Web Storage; supply a tiny in-memory shim so
// the gate helpers (gates.ts) and any localStorage reads don't crash under test.
if (typeof globalThis.localStorage === "undefined") {
  const store = new Map<string, string>();
  const mem: Storage = {
    get length() {
      return store.size;
    },
    clear: () => {
      store.clear();
    },
    getItem: (k: string) => store.get(k) ?? null,
    key: (i: number) => [...store.keys()][i] ?? null,
    removeItem: (k: string) => void store.delete(k),
    setItem: (k: string, v: string) => void store.set(k, v),
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: mem,
    configurable: true,
  });
}

// Every command resolves through the TYPED registry defaults (`commandMocks.ts`): each
// default's return is checked against `Commands[K]["ret"]`, so a wrong shape is a `tsc`
// error. The factory runs while `./commandMocks` is still importing this module, so it
// declares its own `MockChannel` and reads `resolveCommand` lazily, at call time.
vi.mock("@tauri-apps/api/core", () => {
  class MockChannel {
    onmessage: ((message: unknown) => void) | null = null;
  }
  return {
    invoke: vi.fn((command: string, args?: unknown) =>
      resolveCommand({}, command, args),
    ),
    Channel: MockChannel,
  };
});

// The factory reads `liveListeners` lazily, at call time, for the same reason.
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: LiveListener) => {
    liveListeners.set(name, cb);
    return Promise.resolve(() => liveListeners.delete(name));
  },
}));

// A fixed 72×100 box: enough for HalfStackArt to lay head over cab.
(SVGElement.prototype as unknown as { getBBox: () => DOMRect }).getBBox = () =>
  ({ x: 0, y: 0, width: 72, height: 100 }) as DOMRect;

// jsdom lays nothing out, so there is nothing to scroll.
(
  Element.prototype as unknown as { scrollIntoView: () => void }
).scrollIntoView = () => undefined;
