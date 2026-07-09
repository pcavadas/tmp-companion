// Live-sync monitor event listeners — assert each wrapper subscribes to the EXACT
// event name the backend emits, with the typed payload threaded through. Mocks
// @tauri-apps/api/event so no real bridge is touched; forces `isTauri()` true so the
// wrappers take the real `listen` path (off-Tauri they no-op by design).

import { describe, it, expect, vi, beforeEach } from "vitest";

const listenMock = vi.fn<
  (name: string, cb: (e: { payload: unknown }) => void) => Promise<() => void>
>(() =>
  Promise.resolve(() => {
    /* no-op unlisten */
  }),
);
vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: [string, (e: { payload: unknown }) => void]) =>
    listenMock(...args),
}));

// Force the in-Tauri path so the wrappers call `listen` (off-Tauri they no-op).
vi.mock("../lib/invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invoke")>();
  return { ...actual, isTauri: () => true };
});

import {
  LIVE_EVENT,
  onLivePreset,
  onLiveScene,
  onSceneList,
  onSignalChain,
  onSync,
  onLevelingLufs,
} from "../lib/liveEvents";
import type {
  LiveLufsEvent,
  LivePresetEvent,
  LiveSceneEvent,
  SceneListEvent,
  SignalChainEvent,
  SyncEvent,
} from "../lib/types";

beforeEach(() => {
  listenMock.mockClear();
});

describe("live-sync event names match the backend EVT_* constants", () => {
  it("exposes the monitor event names verbatim (+ backup-progress)", () => {
    expect(LIVE_EVENT).toEqual({
      livePreset: "tmp://live-preset",
      liveScene: "tmp://live-scene",
      sceneList: "tmp://scene-list",
      signalChain: "tmp://signal-chain",
      sync: "tmp://sync",
      backupProgress: "tmp://backup-progress",
      levelingLufs: "tmp://leveling-lufs",
    });
  });
});

describe("each wrapper subscribes to its event and forwards the typed payload", () => {
  it("onLivePreset → tmp://live-preset", async () => {
    let got: LivePresetEvent | null = null;
    await onLivePreset((e) => {
      got = e;
    });
    const [name, cb] = listenMock.mock.calls[0];
    expect(name).toBe("tmp://live-preset");
    const payload: LivePresetEvent = {
      listIndex: 3,
      name: "Lead",
      isDirty: false,
      isFavorite: true,
    };
    cb({ payload });
    expect(got).toEqual(payload);
  });

  it('onLiveScene → tmp://live-scene (key may be "base" or a number)', async () => {
    let got: LiveSceneEvent | null = null;
    await onLiveScene((e) => {
      got = e;
    });
    const [name, cb] = listenMock.mock.calls[0];
    expect(name).toBe("tmp://live-scene");
    const payload: LiveSceneEvent = { key: "base", name: "Base" };
    cb({ payload });
    expect(got).toEqual(payload);
  });

  it("onSceneList → tmp://scene-list (fs null degrades to the em-dash)", async () => {
    let got: SceneListEvent | null = null;
    await onSceneList((e) => {
      got = e;
    });
    const [name, cb] = listenMock.mock.calls[0];
    expect(name).toBe("tmp://scene-list");
    const payload: SceneListEvent = { scenes: [{ name: "Clean", fs: null }] };
    cb({ payload });
    expect(got).toEqual(payload);
  });

  it("onSignalChain → tmp://signal-chain (ActiveGraph shape)", async () => {
    let got: SignalChainEvent | null = null;
    await onSignalChain((e) => {
      got = e;
    });
    const [name, cb] = listenMock.mock.calls[0];
    expect(name).toBe("tmp://signal-chain");
    const payload: SignalChainEvent = {
      name: "Lead",
      slot: 3,
      template: "gtrSeries",
      split_mix: null,
      nodes: [],
      stages: [],
    };
    cb({ payload });
    expect(got).toEqual(payload);
  });

  it("onSync → tmp://sync", async () => {
    let got: SyncEvent | null = null;
    await onSync((e) => {
      got = e;
    });
    const [name, cb] = listenMock.mock.calls[0];
    expect(name).toBe("tmp://sync");
    cb({ payload: { syncing: true } satisfies SyncEvent });
    expect(got).toEqual({ syncing: true });
  });

  it("onLevelingLufs → tmp://leveling-lufs (forwards the latest reading)", async () => {
    let got: LiveLufsEvent | null = null;
    await onLevelingLufs((e) => {
      got = e;
    });
    const [name, cb] = listenMock.mock.calls[0];
    expect(name).toBe("tmp://leveling-lufs");
    cb({ payload: { lufs: -23.4, momentary: -30.0 } satisfies LiveLufsEvent });
    expect(got).toEqual({ lufs: -23.4, momentary: -30.0 });
  });
});
