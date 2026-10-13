// src/__tests__/setup.ts — Vitest global setup (referenced by vitest.config.ts).
//
// 1. jest-dom matchers for RTL.
// 2. A default mock of @tauri-apps/api/core so screen smoke tests don't hit Tauri.
//    `invoke(command, args)` resolves the command's typed EMPTY default
//    (`DEFAULT_HANDLERS` in ./commandMocks) — enough for a screen to mount + render
//    its empty state without throwing. Individual tests override commands with
//    `mockCommands({ … })` from ./commandMocks.

import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

import { resolveCommand } from "./commandMocks";

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
