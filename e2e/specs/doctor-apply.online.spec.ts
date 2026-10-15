import type { Page } from "@playwright/test";
import { test, expect } from "../fixtures/test";
import {
  SCENARIO,
  clearScenario,
  ensureScenario,
  invoke,
  isOnline,
} from "../fixtures/scenario";

// This spec covers apply, save and discard only. The `BypassOnly` prescription refusal
// (COVERAGE row 30) is proven offline: the policy units in `commands/doctor_tests.rs`
// plus `doctor-apply-refusal.spec.ts`.
// ONLINE-ONLY suite member: exercises the doctor_apply → doctor_save and
// doctor_apply → doctor_discard command paths end-to-end on the REAL device
// with a deterministic hand-built job (an EQ-10 cut inserted into each preset's
// G1 chain), independent of which verdicts the diagnosis happens to fire —
// UI-driven Apply can't be asserted deterministically (prescription content is
// sound-dependent). Skipped offline (see below).
//
// Every step is checked against the DEVICE, never against the command's own return:
// - the A/B clips decode as WAV, carry real audio and differ;
// - apply mutates the live working copy (`read_active_preset`, the field-3 graph);
// - save is read back from the STORED row (`e2e_read_stored_preset`, the device's saved
//   row for that one slot): the EQ is there with the cut, and nothing else in the chain moved;
// - discard puts the working copy back to the pre-apply stored chain, and the stored row
//   never changed.
// A no-op save or a no-op discard therefore fails here.
//
// In the default online set, LAST (after copy): its save is a structural write over a
// fixture slot, which clears the server's SCENARIO_VERIFIED flag, so nothing after it in
// the run pays a re-verify. Doctor-persistence code (`commands/doctor.rs`) requires it
// through scripts/gate-map.tsv's `doctor-persist` class.
//
// Device time: 3 one-slot stored reads, 3 live graph reads and 2 applies (each one before
// + one after capture). No page load and no whole-library backup: every call is a raw
// invoke, and a full backup streams at ~27 KiB/s (85 s for a 2.2 MiB bank).

/** The fields of `session::GraphNode` this spec reads (snake_case on the wire). */
interface GraphNode {
  group_id: string;
  node_id: string;
  model: string;
  bypassed: boolean;
  params: Record<string, number>;
}

/** One stored preset row (`e2e_read_stored_preset`): the same `graph` and `footswitches`
 *  derivations as `backup_read::BackupPresetRow`. */
interface StoredRow {
  name: string;
  graph: { nodes: GraphNode[] };
  footswitches: unknown[];
}

const EQ = "ACD_TenBandEQStereo";
const CUT_DB = -3;
const OPS = [
  {
    kind: "insert_node",
    groupId: "G1",
    beforeFenderId: null,
    fenderId: EQ,
    params: [["gain250hz", CUT_DB]],
  },
];
// Stored reads and captures run far past Playwright's 30 s request default.
const T = 180_000;

/** The stored row of list index `slot`, read from the device (read-only), checked to be
 *  `name`'s. */
async function storedRow(
  page: Page,
  slot: number,
  name: string,
): Promise<StoredRow> {
  const row = (await invoke(
    page,
    "e2e_read_stored_preset",
    { slot },
    T,
  )) as StoredRow;
  expect(row.name, `stored row ${String(slot + 1)} is ${name}`).toBe(name);
  return row;
}

/** The live working copy's chain (field-3), checked to be `name`'s. */
async function liveNodes(page: Page, name: string): Promise<GraphNode[]> {
  const g = (await invoke(page, "read_active_preset", {}, T)) as {
    name: string | null;
    nodes: GraphNode[];
  };
  expect(g.name, "the working copy is the preset under test").toBe(name);
  return g.nodes;
}

/** Block identities, in chain order — what a structural edit changes. */
const ids = (nodes: GraphNode[]): string[] =>
  nodes.map((n) => `${n.group_id}/${n.node_id}/${n.model}`);
const eqs = (nodes: GraphNode[]): GraphNode[] =>
  nodes.filter((n) => n.model === EQ);

/** Bytes 2..16 of every KSDATAFORMAT_SUBTYPE_* GUID (WAVE_FORMAT_EXTENSIBLE). */
const KSDATAFORMAT_TAIL = "000000001000800000aa00389b71";

/** Decode a `data:audio/wav;base64,` clip and check it is real audio: a RIFF/WAVE
 *  header, a `fmt ` and a `data` chunk, at least 1 s of samples, and a signal above
 *  -60 dBFS RMS (a failed inject captures the device's floor). Returns the samples'
 *  bytes for the A/B comparison. `wav_bytes` writes 32-bit float, which hound
 *  tags WAVE_FORMAT_EXTENSIBLE (0xFFFE) with the float subformat GUID, as it
 *  does for every sample size above 16 bits; a plain 3 (IEEE float) is accepted
 *  too. */
function decodeClip(label: string, url: string): Buffer {
  const prefix = "data:audio/wav;base64,";
  expect(url.startsWith(prefix), `${label} is a WAV data URL`).toBe(true);
  const b = Buffer.from(url.slice(prefix.length), "base64");
  expect(b.toString("ascii", 0, 4), `${label} RIFF`).toBe("RIFF");
  expect(b.toString("ascii", 8, 12), `${label} WAVE`).toBe("WAVE");
  let fmt: {
    format: number;
    channels: number;
    rate: number;
    bits: number;
  } | null = null;
  let data: Buffer | null = null;
  for (let o = 12; o + 8 <= b.length;) {
    const id = b.toString("ascii", o, o + 4);
    const size = b.readUInt32LE(o + 4);
    if (id === "fmt ") {
      const tag = b.readUInt16LE(o + 8);
      // EXTENSIBLE: the real format is the first two bytes of the SubFormat GUID
      // (offset 24 in the chunk body), the rest being the standard KSDATAFORMAT tail.
      const extensible =
        tag === 0xfffe &&
        size >= 40 &&
        b.toString("hex", o + 34, o + 48) === KSDATAFORMAT_TAIL;
      fmt = {
        format: extensible ? b.readUInt16LE(o + 32) : tag,
        channels: b.readUInt16LE(o + 10),
        rate: b.readUInt32LE(o + 12),
        bits: b.readUInt16LE(o + 22),
      };
    } else if (id === "data") {
      data = b.subarray(o + 8, o + 8 + size);
    }
    o += 8 + size + (size % 2);
  }
  expect(fmt, `${label} has a fmt chunk`).not.toBeNull();
  expect(data, `${label} has a data chunk`).not.toBeNull();
  if (!fmt || !data) throw new Error(`${label}: undecodable WAV`);
  expect(
    fmt.format === 3 && fmt.bits === 32,
    `${label} is 32-bit float (got format ${String(fmt.format)}, ${String(fmt.bits)} bits)`,
  ).toBe(true);
  const samples = data.length / 4;
  expect(
    samples / fmt.channels,
    `${label} holds at least 1 s of audio`,
  ).toBeGreaterThanOrEqual(fmt.rate);
  let sum = 0;
  for (let i = 0; i < samples; i++) {
    const v = data.readFloatLE(i * 4);
    sum += v * v;
  }
  const rmsDb = 10 * Math.log10(sum / samples + 1e-30);
  expect(
    rmsDb,
    `${label} carries signal (RMS ${rmsDb.toFixed(1)} dBFS)`,
  ).toBeGreaterThan(-60);
  return data;
}

test.describe("Doctor apply/save/discard — one-off HW validation", () => {
  test.afterEach(async ({ page }) => {
    await clearScenario(page);
  });

  test("apply returns A/B clips; save persists; discard restores", async ({
    page,
  }) => {
    // ONLINE-ONLY: offline the fake re-amp never loads the slot, so the apply's
    // identity guard (confirm_active) correctly refuses ("slot echo None") —
    // a SimDevice fidelity limit, not a product bug.
    //
    // Asks the SERVER, and from inside the test body. The previous
    // `test.skip(!process.env.TMP_E2E_ONLINE)` at describe level was always TRUE:
    // scripts/e2e.sh sets that var only on the server invocation, so the Playwright
    // process never sees it (the same trap documented in level.spec.ts's merged
    // idempotency test).
    test.skip(!(await isOnline(page)), "online-only one-off HW validation");
    test.setTimeout(600_000);
    await ensureScenario(page);

    const t1 = SCENARIO[1]; // E2E Pedalboard — apply, then save
    const t2 = SCENARIO[2]; // E2E Edge — apply, then discard
    const before1 = await storedRow(page, t1.slot, t1.name);
    const before2 = await storedRow(page, t2.slot, t2.name);
    const saved1 = before1.graph.nodes;
    const saved2 = before2.graph.nodes;
    // The premise both checks below rest on: neither stored chain has the EQ yet, so a
    // save that wrote nothing, or a discard that kept the edit, cannot pass.
    expect(eqs(saved1), `${t1.name} starts without ${EQ}`).toHaveLength(0);
    expect(eqs(saved2), `${t2.name} starts without ${EQ}`).toHaveLength(0);

    // The job the Doctor UI would build: the preset's own stored chain and footswitches.
    const job = (slot: number, name: string, row: StoredRow) => ({
      listIndex: slot,
      name,
      ops: OPS,
      topologyId: "guitar-humbucker",
      calibrationLufs: null,
      scene: null,
      footswitch: null,
      nodes: row.graph.nodes,
      footswitches: row.footswitches,
    });
    /** The one EQ the apply inserted, carrying the cut. */
    const expectEqApplied = (nodes: GraphNode[], where: string) => {
      const added = eqs(nodes);
      expect(added, `${where}: exactly one ${EQ}`).toHaveLength(1);
      expect(added[0].group_id, `${where}: the EQ is in G1`).toBe("G1");
      expect(
        added[0].params.gain250hz,
        `${where}: the EQ carries the ${String(CUT_DB)} dB cut at 250 Hz`,
      ).toBeCloseTo(CUT_DB, 3);
    };

    // (1) APPLY on t1 → real, distinct A/B clips, and the working copy holds the EQ.
    const applied = (await invoke(
      page,
      "doctor_apply",
      { job: job(t1.slot, t1.name, before1) },
      T,
    )) as { beforeClip: string; afterClip: string };
    const a = decodeClip("before clip", applied.beforeClip);
    const b = decodeClip("after clip", applied.afterClip);
    expect(a.equals(b), "the before and after clips differ").toBe(false);
    expectEqApplied(
      await liveNodes(page, t1.name),
      "after apply (working copy)",
    );

    // (2) SAVE, then read the STORED row back: the EQ with its cut, and every block the
    // preset already had, in order — the save rebuilt SAVED+ops, nothing else.
    await invoke(
      page,
      "doctor_save",
      { listIndex: t1.slot, expectName: t1.name, scene: null, ops: OPS },
      T,
    );
    const afterSave = (await storedRow(page, t1.slot, t1.name)).graph.nodes;
    expectEqApplied(afterSave, "after save (stored row)");
    expect(
      ids(afterSave.filter((n) => n.model !== EQ)),
      "the save changed nothing but the inserted EQ",
    ).toEqual(ids(saved1));
    // ...and every one of those blocks kept its bypass state and every parameter value
    // (a float tolerance, since the stored row round-trips each value through the device).
    afterSave
      .filter((n) => n.model !== EQ)
      .forEach((n, i) => {
        const was = saved1[i];
        const where = `${n.group_id}/${n.node_id}`;
        expect(n.bypassed, `${where}: bypass unchanged by the save`).toBe(
          was.bypassed,
        );
        expect(
          Object.keys(n.params).sort(),
          `${where}: the same parameters after the save`,
        ).toEqual(Object.keys(was.params).sort());
        for (const [k, v] of Object.entries(was.params)) {
          expect(
            n.params[k],
            `${where}.${k} unchanged by the save`,
          ).toBeCloseTo(v, 4);
        }
      });

    // (3) APPLY on t2, then DISCARD: the working copy goes back to the pre-apply stored
    // chain, and the stored row never changed.
    await invoke(
      page,
      "doctor_apply",
      { job: job(t2.slot, t2.name, before2) },
      T,
    );
    expectEqApplied(
      await liveNodes(page, t2.name),
      "before discard (working copy)",
    );
    await invoke(page, "doctor_discard", { listIndex: t2.slot }, T);
    expect(
      ids(await liveNodes(page, t2.name)),
      "discard restores the pre-apply chain in the working copy",
    ).toEqual(ids(saved2));
    expect(
      ids((await storedRow(page, t2.slot, t2.name)).graph.nodes),
      "apply + discard never touched the stored row",
    ).toEqual(ids(saved2));
  });
});
