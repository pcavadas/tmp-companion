// src/__tests__/BlockLevelPick.test.tsx — `BlockLevelPick`'s two-dropdown
// leveling-handle picker (D2/Part C: the original single flat block+param list
// split into a BLOCK dropdown, then a CONTROL dropdown for the selected block's
// own params).
//
// The DANGER-rule guard (`Pick`/`BlockPick` trap) now has TWO arms: a stored
// `handle` whose BLOCK isn't in the (resolved) candidate list at all must render
// VERBATIM + a warning on the BLOCK trigger (the control dropdown has nothing to
// show, so it's hidden — never silently dropping the stored param from view); a
// block that IS present but whose stored PARAM is gone must render normally on the
// block trigger and VERBATIM + "(removed)" + a warning on the CONTROL trigger.
// Never silently falls back to the pseudo-option or the first candidate
// (BUG→GATE, item 8a: "not yet fetched" must not be conflated with "fetched, this
// handle is gone").

import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";
import { BlockLevelPick } from "../views/overlays/BlockLevelPick";
import { WithCard } from "./pickCardTestUtils";
import type {
  BlockLevelCandidate,
  BlockLevelHandle,
} from "../views/overlays/BlockLevelPick";

const BLOCK_TITLE = "Choose this sound's leveling block";
const CONTROL_TITLE = "Choose this sound's leveling parameter";

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
    Parameters<typeof BlockLevelPick>[0] & { handle: BlockLevelHandle | null }
  > & { onHandleChange?: (h: BlockLevelHandle | null) => void } = {},
) {
  const onHandleChange = props.onHandleChange ?? vi.fn();
  render(
    <ThemeProvider>
      <WithCard>
        <BlockLevelPick
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

describe("BlockLevelPick — DANGER-rule guard for a stale stored handle", () => {
  it("block missing from the list: block trigger renders it VERBATIM + warning, control dropdown hidden", async () => {
    renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "gone", parameterId: "outputLevel" },
      candidates: { status: "resolved", list: [twinOutputLevel] },
    });
    expect(screen.queryByText("Amp output level")).toBeNull();
    // Verbatim: the raw stored nodeId, plus the param's label so the stored param
    // is never silently dropped from view.
    expect(screen.getByText("gone · Output level")).toBeInTheDocument();
    expect(screen.getByTitle(BLOCK_TITLE)).toBeInTheDocument();
    expect(screen.queryByTitle(CONTROL_TITLE)).toBeNull();
    // The warning isn't just the trigger's icon/color — the menu itself carries an
    // explicit stale note (proves a warning affordance actually exists, not just a
    // verbatim label).
    const user = userEvent.setup();
    await user.click(screen.getByTitle(BLOCK_TITLE));
    expect(
      screen.getByText(/stored pick no longer offered — pick again/),
    ).toBeInTheDocument();
  });

  it("shows a stored handle's plain label while unfetched — no removed/warn state, control dropdown hidden", () => {
    renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: { status: "unfetched" },
    });
    expect(screen.getByText("Output level")).toBeInTheDocument();
    expect(screen.queryByText(/removed/)).toBeNull();
    expect(screen.queryByTitle(CONTROL_TITLE)).toBeNull();
  });

  it('block present but param gone: block trigger renders normally, control trigger renders the param VERBATIM + "(removed)" + warning', async () => {
    renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "goneParam" },
      candidates: { status: "resolved", list: [twinOutputLevel] },
    });
    // Block trigger: the block IS found, so it renders its normal (catalog) label —
    // never the raw nodeId.
    expect(screen.getByText("FENDER '65 TWIN REVERB")).toBeInTheDocument();
    // Control trigger: the stored param verbatim + the removed marker + a warning.
    expect(screen.getByText("GoneParam (removed)")).toBeInTheDocument();
    const controlTrigger = screen.getByTitle(CONTROL_TITLE);
    expect(controlTrigger).toBeInTheDocument();
    // The warning isn't just the trigger's icon/color — the control menu itself
    // carries an explicit stale note (proves a warning affordance actually exists,
    // not just a verbatim label).
    const user = userEvent.setup();
    await user.click(controlTrigger);
    expect(
      screen.getByText(/stored pick no longer offered — pick a control below/),
    ).toBeInTheDocument();
  });

  it("a fully matched handle renders both triggers with no warning", () => {
    renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: { status: "resolved", list: [twinOutputLevel] },
    });
    expect(screen.getByText("FENDER '65 TWIN REVERB")).toBeInTheDocument();
    expect(screen.getByText("Output level")).toBeInTheDocument();
    expect(screen.queryByText(/removed/)).toBeNull();
  });
});

describe("BlockLevelPick — two-dropdown navigation", () => {
  it("picking a DIFFERENT block auto-picks its best ENABLED candidate (fewest clicks)", async () => {
    const user = userEvent.setup();
    const onHandleChange = renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: {
        status: "resolved",
        // Boost's candidates are listed OTHER-first, level_db second — the auto-pick
        // must still land on `gain` (rank order, not array order).
        list: [twinOutputLevel, boostToneOther, boostGain],
      },
    });
    await user.click(screen.getByTitle(BLOCK_TITLE));
    await user.click(await screen.findByText("BOOST"));
    expect(onHandleChange).toHaveBeenCalledExactlyOnceWith({
      groupId: "G1",
      nodeId: "boost1",
      parameterId: "gain",
    });
  });

  it("re-selecting the block that already holds the stored handle keeps the stored param (no call)", async () => {
    const user = userEvent.setup();
    const onHandleChange = renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "boost1", parameterId: "toneKnob" },
      candidates: {
        status: "resolved",
        list: [twinOutputLevel, boostToneOther, boostGain],
      },
    });
    await user.click(screen.getByTitle(BLOCK_TITLE));
    // The trigger ALREADY reads "BOOST" (the current handle's block) — scope the
    // click to the freshly-opened MENU's own row (`data-block-pick`), not the
    // trigger's identical text, which `getByText` would otherwise find twice.
    const menuRow = document.querySelector('[data-block-pick="G1:boost1"]');
    if (!menuRow) throw new Error("Boost's block row did not render");
    await user.click(menuRow);
    // A click that LOOKS like navigation must never silently change a stored
    // handle — even though `toneKnob` isn't Boost's best-ranked candidate.
    expect(onHandleChange).not.toHaveBeenCalled();
  });

  it("the pseudo-option hides the control dropdown (the pseudo path has no param)", () => {
    renderPick({
      pseudoLabel: "Amp output level",
      handle: null,
      candidates: { status: "resolved", list: [twinOutputLevel] },
    });
    expect(screen.getByText("Amp output level")).toBeInTheDocument();
    expect(screen.queryByTitle(CONTROL_TITLE)).toBeNull();
  });

  it("picking the pseudo-option submits a null handle", async () => {
    const user = userEvent.setup();
    const onHandleChange = renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: { status: "resolved", list: [twinOutputLevel] },
    });
    await user.click(screen.getByTitle(BLOCK_TITLE));
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
    await user.click(screen.getByTitle(BLOCK_TITLE));
    expect(screen.queryByText(/\(default\)/)).toBeNull();
  });

  it("opening the control dropdown lists the selected block's own params, selecting one closes it and submits the new handle", async () => {
    const user = userEvent.setup();
    const onHandleChange = renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "boost1", parameterId: "gain" },
      candidates: {
        status: "resolved",
        list: [twinOutputLevel, boostToneOther, boostGain],
      },
    });
    await user.click(screen.getByTitle(CONTROL_TITLE));
    // Only Boost's own two params are offered, not the Twin's `outputLevel`. The
    // trigger ALREADY reads "Gain" (the current handle's param), so scope both the
    // presence check and the click to the freshly-opened MENU's own rows
    // (`data-block-param-pick`), not the trigger's identical text.
    expect(
      document.querySelector('[data-block-param-pick="boost1:gain"]'),
    ).not.toBeNull();
    const toneRow = document.querySelector(
      '[data-block-param-pick="boost1:toneKnob"]',
    );
    if (!toneRow)
      throw new Error("Boost's ToneKnob control row did not render");
    expect(toneRow).toHaveTextContent("ToneKnob");
    expect(screen.queryByText("Output level")).toBeNull();
    await user.click(toneRow);
    expect(onHandleChange).toHaveBeenCalledExactlyOnceWith({
      groupId: "G1",
      nodeId: "boost1",
      parameterId: "toneKnob",
    });
  });
});

describe("BlockLevelPick — disabled reasons and per-candidate notes", () => {
  const sharedReason =
    "shared with the base preset — changes every scene sharing it";
  const disabledBoostGain: BlockLevelCandidate = {
    ...boostGain,
    disabled: true,
    disabledTitle: sharedReason,
  };
  const disabledBoostTone: BlockLevelCandidate = {
    ...boostToneOther,
    disabled: true,
    disabledTitle: sharedReason,
  };

  it("a block whose every candidate is disabled renders disabled with the shared reason, and is unclickable", async () => {
    const user = userEvent.setup();
    const onHandleChange = renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "amp", parameterId: "outputLevel" },
      candidates: {
        status: "resolved",
        list: [twinOutputLevel, disabledBoostTone, disabledBoostGain],
      },
    });
    await user.click(screen.getByTitle(BLOCK_TITLE));
    expect(screen.getByTitle(sharedReason)).toBeInTheDocument();
    await user.click(screen.getByText("BOOST"));
    expect(onHandleChange).not.toHaveBeenCalled();
  });

  it("a partially-disabled block's control dropdown still shows the per-candidate reason on the disabled row", async () => {
    const user = userEvent.setup();
    renderPick({
      pseudoLabel: "Amp output level",
      handle: { groupId: "G1", nodeId: "boost1", parameterId: "gain" },
      candidates: {
        status: "resolved",
        list: [twinOutputLevel, disabledBoostTone, boostGain],
      },
    });
    await user.click(screen.getByTitle(CONTROL_TITLE));
    expect(screen.getByTitle(sharedReason)).toBeInTheDocument();
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
    await user.click(screen.getByTitle(CONTROL_TITLE));
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
    await user.click(screen.getByTitle(CONTROL_TITLE));
    expect(screen.getByText("can only lower")).toBeInTheDocument();
  });

  it('tags the block holding the overall-best candidate "Recommended" in the block dropdown', async () => {
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
    await user.click(screen.getByTitle(BLOCK_TITLE));
    expect(screen.getByText("Recommended")).toBeInTheDocument();
  });

  it("top block entirely disabled ⇒ the NEXT block carries Recommended, and auto-pick lands on it (BUG, now fixed: the tag and the auto-pick used to disagree — `blocks[0][0]` with no disabled filter vs. auto-pick's `find(!disabled)` — so a fully-disabled top block left NO block tagged Recommended at all)", async () => {
    const user = userEvent.setup();
    const onHandleChange = renderPick({
      pseudoLabel: "Amp output level",
      handle: null,
      candidates: {
        status: "resolved",
        // Boost's `gain` is level_db (rank 0, same as Twin's outputLevel) and sorts
        // first (stable tie-break on insertion order) — but EVERY Boost candidate is
        // disabled. Twin's outputLevel is the next block down that actually has a
        // pickable candidate.
        list: [disabledBoostGain, twinOutputLevel],
      },
    });
    await user.click(screen.getByTitle(BLOCK_TITLE));
    const boostRow = document.querySelector('[data-block-pick="G1:boost1"]');
    if (!boostRow) throw new Error("Boost's block row did not render");
    expect(
      within(boostRow as HTMLElement).queryByText("Recommended"),
    ).toBeNull();
    const twinRow = document.querySelector('[data-block-pick="G1:amp"]');
    if (!twinRow) throw new Error("Twin's block row did not render");
    expect(
      within(twinRow as HTMLElement).getByText("Recommended"),
    ).toBeInTheDocument();
    // Clicking Twin (a different, non-stored block) auto-picks its own best enabled
    // candidate — the same `bestEnabled` the tag itself reads.
    await user.click(twinRow);
    expect(onHandleChange).toHaveBeenCalledExactlyOnceWith({
      groupId: "G1",
      nodeId: "amp",
      parameterId: "outputLevel",
    });
  });
});
