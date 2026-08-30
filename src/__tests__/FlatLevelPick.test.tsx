// src/__tests__/FlatLevelPick.test.tsx — `FlatLevelPick`'s single-dropdown
// leveling-handle picker (D2, design-1a redesign): one flattened list of
// candidates, grouped by block with a header only when a preset carries more
// than one leveling-relevant block. Replaces `BlockLevelPick.test.tsx`'s
// two-dropdown (block, then control) coverage.
//
// The DANGER-rule guard now has ONE arm (there is only one trigger): a stored
// `handle` whose exact groupId/nodeId/parameterId isn't in the (resolved)
// candidate list renders VERBATIM (`nodeId · param label`) + a warning, never a
// silent fallback to the pseudo-option or the first candidate (BUG→GATE, item
// 8a: "not yet fetched" must not be conflated with "fetched, this handle is
// gone").

import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";
import { FlatLevelPick } from "../views/level/FlatLevelPick";
import { WithCard } from "./pickCardTestUtils";
import type {
  BlockLevelCandidate,
  BlockLevelHandle,
} from "../views/level/FlatLevelPick";

const TRIGGER_TITLE = "Choose this sound's leveling control";

// Two distinct blocks, each resolving to a catalog full name via `blockArtTile`
// (the ALL-CAPS Pro-Control-style name, e.g. "BOOST" — see `blockArt.ts`'s
// `BlockArtFields.fullName` doc).
const twinOutputLevel: BlockLevelCandidate = {
  groupId: "G1",
  nodeId: "amp",
  fenderId: "ACD_TwinReverb65NoFx", // → "FENDER '65 TWIN REVERB"
  parameterId: "outputLevel",
  paramClass: "level_linear",
};
const boostToneOther: BlockLevelCandidate = {
  groupId: "G1",
  nodeId: "boost1",
  fenderId: "ACD_Boost", // → "BOOST"
  parameterId: "toneKnob",
  // No class → ranks LAST within its block (after any level/wet_mix candidate).
};
const boostGain: BlockLevelCandidate = {
  groupId: "G1",
  nodeId: "boost1",
  fenderId: "ACD_Boost",
  parameterId: "gain",
  paramClass: "level_db",
};

function renderPick(
  props: Partial<
    Parameters<typeof FlatLevelPick>[0] & { handle: BlockLevelHandle | null }
  > & { onHandleChange?: (h: BlockLevelHandle | null) => void } = {},
) {
  const onHandleChange = props.onHandleChange ?? vi.fn();
  render(
    <ThemeProvider>
      <WithCard>
        <FlatLevelPick
          pseudoLabel={props.pseudoLabel}
          handle={props.handle ?? null}
          onHandleChange={onHandleChange}
          candidates={props.candidates ?? { status: "unfetched" }}
          onOpen={props.onOpen ?? (() => undefined)}
        />
      </WithCard>
    </ThemeProvider>,
  );
  return onHandleChange;
}

describe("FlatLevelPick — DANGER-rule guard for a stale stored handle", () => {
  it("handle missing from the list: trigger renders it VERBATIM + warning, menu carries a stale note", async () => {
    renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "gone", parameterId: "outputLevel" },
      candidates: { status: "resolved", list: [twinOutputLevel] },
    });
    expect(screen.queryByText("Amp output level")).toBeNull();
    expect(screen.getByText("gone · Output level")).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByTitle(TRIGGER_TITLE));
    expect(
      screen.getByText(/stored pick no longer offered — pick again/),
    ).toBeInTheDocument();
  });

  it("shows a stored handle's plain label while unfetched — no warning", () => {
    renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: { status: "unfetched" },
    });
    expect(screen.getByText("Output level")).toBeInTheDocument();
    expect(screen.queryByText(/no longer offered/)).toBeNull();
  });

  it("a fully matched handle renders the combined block + param label with no warning", () => {
    renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: { status: "resolved", list: [twinOutputLevel] },
    });
    expect(
      screen.getByText("FENDER '65 TWIN REVERB — Output level"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/no longer offered/)).toBeNull();
  });
});

describe("FlatLevelPick — single-list navigation", () => {
  it("picking a candidate submits its handle and closes the menu", async () => {
    const user = userEvent.setup();
    const onHandleChange = renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: {
        status: "resolved",
        list: [twinOutputLevel, boostToneOther, boostGain],
      },
    });
    await user.click(screen.getByTitle(TRIGGER_TITLE));
    const row = document.querySelector(
      '[data-block-param-pick="G1:boost1:gain"]',
    );
    if (!row) throw new Error("Boost's gain row did not render");
    await user.click(row);
    expect(onHandleChange).toHaveBeenCalledExactlyOnceWith({
      groupId: "G1",
      nodeId: "boost1",
      parameterId: "gain",
    });
    expect(screen.queryByTitle(/stored pick/)).toBeNull();
  });

  it("a single-block list has no standalone block-name header", async () => {
    const user = userEvent.setup();
    renderPick({
      pseudoLabel: "Amp output level",
      handle: null,
      candidates: { status: "resolved", list: [twinOutputLevel] },
    });
    await user.click(screen.getByTitle(TRIGGER_TITLE));
    // Combined-label row only ("BLOCK — PARAM") — an exact match for the block
    // name alone would only find a separate header, which doesn't render here.
    expect(screen.queryByText("FENDER '65 TWIN REVERB")).toBeNull();
  });

  it("a multi-block list gets a mono header per block", async () => {
    const user = userEvent.setup();
    renderPick({
      pseudoLabel: "Amp output level",
      handle: null,
      candidates: { status: "resolved", list: [twinOutputLevel, boostGain] },
    });
    await user.click(screen.getByTitle(TRIGGER_TITLE));
    expect(screen.getByText("BOOST")).toBeInTheDocument();
    expect(screen.getByText("FENDER '65 TWIN REVERB")).toBeInTheDocument();
    expect(screen.getByText("BOOST — Gain")).toBeInTheDocument();
  });

  it("the pseudo-option submits a null handle", async () => {
    const user = userEvent.setup();
    const onHandleChange = renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: { status: "resolved", list: [twinOutputLevel] },
    });
    await user.click(screen.getByTitle(TRIGGER_TITLE));
    await user.click(await screen.findByText("Amp output level (default)"));
    expect(onHandleChange).toHaveBeenCalledExactlyOnceWith(null);
  });

  it("footswitch rows (no pseudoLabel) never render the pseudo row", async () => {
    const user = userEvent.setup();
    renderPick({
      // pseudoLabel omitted — the FS shape.
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: { status: "resolved", list: [twinOutputLevel] },
    });
    await user.click(screen.getByTitle(TRIGGER_TITLE));
    expect(screen.queryByText(/\(default\)/)).toBeNull();
  });
});

describe("FlatLevelPick — disabled reasons and per-candidate notes", () => {
  const sharedReason =
    "shared with the base preset — changes every scene sharing it";
  const disabledBoostGain: BlockLevelCandidate = {
    ...boostGain,
    disabled: true,
    disabledTitle: sharedReason,
  };

  it("a disabled candidate shows its reason and is unclickable", async () => {
    const user = userEvent.setup();
    const onHandleChange = renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: {
        status: "resolved",
        list: [twinOutputLevel, disabledBoostGain],
      },
    });
    await user.click(screen.getByTitle(TRIGGER_TITLE));
    expect(screen.getByTitle(sharedReason)).toBeInTheDocument();
    const row = document.querySelector(
      '[data-block-param-pick="G1:boost1:gain"]',
    );
    if (!row) throw new Error("Boost's disabled gain row did not render");
    await user.click(row);
    expect(onHandleChange).not.toHaveBeenCalled();
  });

  it('a wet_mix candidate is flagged "may change the tone"', async () => {
    const user = userEvent.setup();
    const wetMix: BlockLevelCandidate = {
      groupId: "G1",
      nodeId: "boost1",
      fenderId: "ACD_Boost",
      parameterId: "mix",
      paramClass: "wet_mix",
    };
    renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "boost1", parameterId: "gain" },
      candidates: {
        status: "resolved",
        list: [twinOutputLevel, boostGain, wetMix],
      },
    });
    await user.click(screen.getByTitle(TRIGGER_TITLE));
    expect(screen.getByText("may change the tone")).toBeInTheDocument();
  });

  it('a lowers-only candidate is flagged "can only lower"', async () => {
    const user = userEvent.setup();
    const lowersOnly: BlockLevelCandidate = {
      ...twinOutputLevel,
      lowersOnly: true,
    };
    // A SECOND block so the Twin isn't the lone (and thus auto-Recommended) block —
    // the "can only lower" note only shows on a non-recommended candidate.
    renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: { status: "resolved", list: [boostGain, lowersOnly] },
    });
    await user.click(screen.getByTitle(TRIGGER_TITLE));
    expect(screen.getByText("can only lower")).toBeInTheDocument();
  });

  it('tags the overall-best candidate "Recommended"', async () => {
    const user = userEvent.setup();
    renderPick({
      pseudoLabel: "Amp output level",
      handle: null,
      candidates: {
        status: "resolved",
        // Twin's `outputLevel` (level_linear) outranks Boost's `toneKnob` (no class).
        list: [boostToneOther, twinOutputLevel],
      },
    });
    await user.click(screen.getByTitle(TRIGGER_TITLE));
    const twinRow = document.querySelector(
      '[data-block-param-pick="G1:amp:outputLevel"]',
    );
    if (!twinRow) throw new Error("Twin's row did not render");
    expect(
      within(twinRow as HTMLElement).getByText(/Recommended/),
    ).toBeInTheDocument();
  });
});
