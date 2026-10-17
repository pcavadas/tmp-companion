// src/__tests__/useDoctorFlow.test.ts — the Doctor run's ONE status (#290). A
// `doctor_check` that REJECTS must land `failed` with the backend's text, never
// the `stopped` a user Stop produces; a resolved run is `done` or `stopped` off
// its own `stopped` flag; Stop acknowledges at once and its cancel never leaks an
// unhandled rejection.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

vi.mock("../lib/invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invoke")>();
  return {
    ...actual,
    doctorCheck: vi.fn(),
    cancelDoctorCheck: vi.fn(),
  };
});

import { cancelDoctorCheck, doctorCheck } from "../lib/invoke";
import {
  countFinished,
  runReducer,
  useDoctorFlow,
} from "../views/doctor/useDoctorFlow";
import type { DoctorCheckResult, DoctorInputArg } from "../lib/types";

const item: DoctorInputArg = {
  key: "p0",
  listIndex: 0,
  scene: null,
  footswitch: null,
  label: "Clean",
  soundName: "Main sound",
  tag: null,
  topologyId: null,
  calibrationLufs: null,
  profileId: null,
  nodes: [],
  footswitches: [],
  sceneOverrides: [],
};

function renderFlow() {
  return renderHook(() =>
    useDoctorFlow({
      store: null,
      graphByIndex: new Map(),
      footswitchesByIndex: new Map(),
      sceneOverridesByIndex: new Map(),
    }),
  );
}

describe("useDoctorFlow — run status", () => {
  beforeEach(() => {
    vi.mocked(doctorCheck).mockReset();
    vi.mocked(cancelDoctorCheck).mockReset();
  });

  it("a rejecting check lands FAILED with the backend's text — not stopped", async () => {
    // The real IPC rejects with a bare string (Result<_, String>).
    vi.mocked(doctorCheck).mockRejectedValue("the unit stopped answering");
    const { result } = renderFlow();
    act(() => {
      result.current.startRun([item], null);
    });
    expect(result.current.run.status).toBe("running");
    await waitFor(() => {
      expect(result.current.run.status).toBe("failed");
    });
    expect(result.current.run.error).toBe("the unit stopped answering");
    expect(result.current.run.result).toBeNull();
  });

  it("a resolved check is done, or stopped when the backend says it was", async () => {
    const res: DoctorCheckResult = { presets: [], stopped: true };
    vi.mocked(doctorCheck).mockResolvedValue(res);
    const { result } = renderFlow();
    act(() => {
      result.current.startRun([item], null);
    });
    await waitFor(() => {
      expect(result.current.run.status).toBe("stopped");
    });
    expect(result.current.run.result).toBe(res);
  });

  it("Stop flips to stopping at once and swallows a cancel IPC failure", async () => {
    vi.mocked(doctorCheck).mockReturnValue(new Promise(() => undefined));
    vi.mocked(cancelDoctorCheck).mockRejectedValue("ipc down");
    const { result } = renderFlow();
    act(() => {
      result.current.startRun([item], null);
    });
    act(() => {
      result.current.stopRun();
    });
    expect(result.current.run.status).toBe("stopping");
    await waitFor(() => {
      expect(cancelDoctorCheck).toHaveBeenCalledTimes(1);
    });
  });
});

describe("countFinished", () => {
  it("counts streamed terminal rows while running, every row once resolved", () => {
    const items = [item, { ...item, key: "p1" }];
    const running = runReducer(
      runReducer(
        runReducer(
          {
            attempt: 0,
            status: "idle",
            items: [],
            statusByKey: {},
            result: null,
            error: null,
          },
          { type: "start", items },
        ),
        { type: "progress", key: "p0", status: "done" },
      ),
      { type: "progress", key: "p1", status: "active" },
    );
    expect(
      countFinished(running.status, running.items, running.statusByKey),
    ).toBe(1);
    const done = runReducer(running, {
      type: "resolved",
      result: { presets: [], stopped: false },
    });
    expect(done.status).toBe("done");
    expect(countFinished(done.status, done.items, done.statusByKey)).toBe(2);
  });
});
