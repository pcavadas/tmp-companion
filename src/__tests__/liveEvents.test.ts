// Live-sync monitor event listeners — assert `onLive(key, cb)` subscribes to the EXACT
// event name the backend emits, with the typed payload threaded through. Mocks
// @tauri-apps/api/event so no real bridge is touched; forces `isTauri()` true so the
// wrappers take the real `listen` path (off-Tauri they no-op by design).

import { describe, it, expect, expectTypeOf, vi, beforeEach } from "vitest";

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

import { LIVE_EVENT, onLive, type LiveEventMap } from "../lib/liveEvents";
import type {
  BackupProgress,
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

describe("onLive subscribes to each key's event and forwards the typed payload", () => {
  it("livePreset → tmp://live-preset", async () => {
    let got: LivePresetEvent | null = null;
    await onLive("livePreset", (e) => {
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

  it('liveScene → tmp://live-scene (key may be "base" or a number)', async () => {
    let got: LiveSceneEvent | null = null;
    await onLive("liveScene", (e) => {
      got = e;
    });
    const [name, cb] = listenMock.mock.calls[0];
    expect(name).toBe("tmp://live-scene");
    const payload: LiveSceneEvent = { key: "base", name: "Base" };
    cb({ payload });
    expect(got).toEqual(payload);
  });

  it("sceneList → tmp://scene-list (fs null degrades to the em-dash)", async () => {
    let got: SceneListEvent | null = null;
    await onLive("sceneList", (e) => {
      got = e;
    });
    const [name, cb] = listenMock.mock.calls[0];
    expect(name).toBe("tmp://scene-list");
    const payload: SceneListEvent = { scenes: [{ name: "Clean", fs: null }] };
    cb({ payload });
    expect(got).toEqual(payload);
  });

  it("signalChain → tmp://signal-chain (ActiveGraph shape)", async () => {
    let got: SignalChainEvent | null = null;
    await onLive("signalChain", (e) => {
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

  it("sync → tmp://sync", async () => {
    let got: SyncEvent | null = null;
    await onLive("sync", (e) => {
      got = e;
    });
    const [name, cb] = listenMock.mock.calls[0];
    expect(name).toBe("tmp://sync");
    cb({ payload: { syncing: true } satisfies SyncEvent });
    expect(got).toEqual({ syncing: true });
  });

  it("levelingLufs → tmp://leveling-lufs (forwards the latest reading)", async () => {
    let got: LiveLufsEvent | null = null;
    await onLive("levelingLufs", (e) => {
      got = e;
    });
    const [name, cb] = listenMock.mock.calls[0];
    expect(name).toBe("tmp://leveling-lufs");
    cb({ payload: { lufs: -23.4, momentary: -30.0 } satisfies LiveLufsEvent });
    expect(got).toEqual({ lufs: -23.4, momentary: -30.0 });
  });

  it("backupProgress → tmp://backup-progress", async () => {
    let got: BackupProgress | null = null;
    await onLive("backupProgress", (e) => {
      got = e;
    });
    const [name, cb] = listenMock.mock.calls[0];
    expect(name).toBe("tmp://backup-progress");
    const payload: BackupProgress = {
      phase: "streaming",
      received: 3,
      total: 10,
      bytes: 300,
      total_bytes: 1000,
      percent: 30,
      build_size: 0,
      build_ticks: 0,
    };
    cb({ payload });
    expect(got).toEqual(payload);
  });
});

describe("onLive covers the whole registry", () => {
  it("every LIVE_EVENT key subscribes to exactly its own name", async () => {
    const keys = Object.keys(LIVE_EVENT) as (keyof LiveEventMap)[];
    for (const k of keys) {
      await onLive(k, () => undefined);
    }
    expect(listenMock.mock.calls.map(([name]) => name)).toEqual(
      keys.map((k) => LIVE_EVENT[k]),
    );
  });

  it("the compiler pairs each key with its own payload type", () => {
    // Type-level: a callback typed for another event's payload must not compile.
    // `expectTypeOf` fails `tsc` (and so the gates) if the pairing ever drifts.
    expectTypeOf<Parameters<typeof onLive<"sync">>[1]>().toEqualTypeOf<
      (payload: SyncEvent) => void
    >();
    expectTypeOf<
      Parameters<typeof onLive<"backupProgress">>[1]
    >().toEqualTypeOf<(payload: BackupProgress) => void>();
    expectTypeOf<keyof LiveEventMap>().toEqualTypeOf<keyof typeof LIVE_EVENT>();
  });
});
