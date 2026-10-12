// useDeviceLoad — #311: overlapping loads (the `connected` flag flipping twice
// during a multi-second live read) must let only the NEWEST load commit, whatever
// order they settle in. REAL timers (repo gotcha).

import { describe, it, expect } from "vitest";
import { act, renderHook } from "@testing-library/react";

import { useDeviceLoad } from "../lib/useDeviceLoad";

/** A load body whose fetch resolves/rejects when the test says so. */
function deferredLoad(commit: (v: string) => void) {
  let resolve: (v: string) => void = () => undefined;
  let reject: (e: Error) => void = () => undefined;
  const fetched = new Promise<string>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  const body = async (isCurrent: () => boolean) => {
    const v = await fetched;
    if (!isCurrent()) return;
    commit(v);
  };
  return { body, resolve, reject };
}

describe("useDeviceLoad — only the newest load commits (#311)", () => {
  it("an older load resolving LAST doesn't overwrite the newer result", async () => {
    const committed: string[] = [];
    const { result } = renderHook(() => useDeviceLoad());
    const older = deferredLoad((v) => committed.push(v));
    const newer = deferredLoad((v) => committed.push(v));

    let p1: Promise<void> = Promise.resolve();
    let p2: Promise<void> = Promise.resolve();
    act(() => {
      p1 = result.current.runLoad(older.body);
      p2 = result.current.runLoad(newer.body);
    });
    await act(async () => {
      newer.resolve("fresh");
      await p2;
      older.resolve("stale");
      await p1;
    });

    expect(committed).toEqual(["fresh"]);
    expect(result.current.phase).toEqual({ kind: "ready" });
  });

  it("an older load REJECTING after a newer success doesn't flip the phase to error", async () => {
    const { result } = renderHook(() => useDeviceLoad());
    const older = deferredLoad(() => undefined);
    const newer = deferredLoad(() => undefined);

    let p1: Promise<void> = Promise.resolve();
    let p2: Promise<void> = Promise.resolve();
    act(() => {
      p1 = result.current.runLoad(older.body);
      p2 = result.current.runLoad(newer.body);
    });
    await act(async () => {
      newer.resolve("ok");
      await p2;
      older.reject(new Error("device went away"));
      await p1;
    });

    expect(result.current.phase).toEqual({ kind: "ready" });
  });

  it("a single load that fails lands in error", async () => {
    const { result } = renderHook(() => useDeviceLoad());
    const only = deferredLoad(() => undefined);
    let p: Promise<void> = Promise.resolve();
    act(() => {
      p = result.current.runLoad(only.body);
    });
    await act(async () => {
      only.reject(new Error("boom"));
      await p;
    });
    expect(result.current.phase).toEqual({ kind: "error", message: "boom" });
  });
});
