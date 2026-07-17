// src/views/doctor/PrescriptionCard.tsx — one prescription with its lifecycle:
// draft → applied → saved. Advisory cards are static (the player turns the knob,
// nothing to apply). The apply path writes LIVE (unsaved) and the save is gated on
// the backup acknowledgment, mirroring the Leveling / Copy save-disclaimer model.

import { useEffect, useId, useRef, useState } from "react";
import type { CSSProperties } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { doctorCard } from "./severity";
import { Icon, type IconName } from "../../ui/Icon";
import { Tag } from "../../ui/Tag";
import { Spinner } from "../../ui/Spinner";
import { Button } from "../../ui/primitives";
import { BackupAckLabel } from "../../ui/BackupAckLabel";
import { doctorApply, doctorDiscard, doctorSave } from "../../lib/invoke";
import { nodeTileArt } from "../../models/blockArt";
import { isComboBid } from "../../models/catalog";
import {
  SignalChainView,
  type StripBlock,
  type StripGraph,
} from "../SignalChainView";
import { ABAudition } from "./ABAudition";
import { useApplyLock } from "./applyLock";
import type {
  DoctorApplyResult,
  DoctorChainPreview,
  DoctorInputArg,
  DoctorRx,
  DoctorRxKind,
  FootswitchInfo,
  GraphNode,
} from "../../lib/types";

const KIND_ICON: Record<DoctorRxKind, IconName> = {
  oneclick: "sliders",
  chain: "link",
  advisory: "settings",
};

const KIND_BADGE: Record<DoctorRxKind, string> = {
  oneclick: "One-click fix",
  chain: "Rebuilds the chain",
  advisory: "You turn the knob",
};

const ADVISORY_LINE =
  "Turn this one on your amp — there's nothing for Doctor to apply.";
// v1 descope: the wire rejects scene trims, so a scene-consistency fix is advised,
// not applied — the player runs it from the Level tab's scene leveling instead.
const SCENE_LINE = "Run scene leveling from the Level tab to apply this one.";

/** Build a one-stage strip graph straight from the chain preview's model ids —
 *  `nodeTileArt` resolves both the block art AND its caption from the model alone,
 *  so no `toStripGraph` lift is needed. Added blocks get the strip's `+` badge. */
function chainGraph(chain: DoctorChainPreview): StripGraph {
  const blocks: StripBlock[] = chain.blocks.map((b) => ({
    ...nodeTileArt(b.model, undefined, isComboBid(b.model)),
    model: b.model,
    change: b.added === true ? "added" : undefined,
  }));
  return { template: chain.template, stages: [{ kind: "series", blocks }] };
}

type Phase = "draft" | "applied" | "saved";

export interface PrescriptionCardProps {
  rx: DoctorRx;
  listIndex: number;
  presetName: string;
  /** Scene-consistency prescriptions can't be applied by the wire (it rejects
   *  scene trims) — render with no Apply button regardless of kind. */
  scene?: boolean;
  /** The diagnosed SOUND's own scene/footswitch (not the `scene` flag above)
   *  + the preset's chain/footswitches — so the A/B captures under the SAME
   *  context `doctor_check` diagnosed, not the as-saved base. Omitted (all
   *  default to "no context") only by scene-consistency cards, which are
   *  never applicable and so never call `doctorApply`/`doctorSave`. */
  soundScene?: number | null;
  soundFootswitch?: number | null;
  nodes?: GraphNode[];
  footswitches?: FootswitchInfo[];
  /** The diagnosed sound's stimulus identity (instrument profile pick at
   *  setup) — the A/B must replay the SAME stimulus the diagnosis used.
   *  Omitted (→ default stimulus) only by scene-consistency cards. */
  stimulus?: DoctorStimulus;
}

/** The stimulus identity a diagnosed sound was measured with — the run's own
 *  `DoctorInputArg` fields, threaded into the apply job so the A/B audition
 *  replays the diagnosis stimulus (topology sample or Tier-2 DI capture). */
export type DoctorStimulus = Pick<
  DoctorInputArg,
  "topologyId" | "calibrationLufs" | "profileId"
>;

const DEFAULT_STIMULUS: DoctorStimulus = {
  topologyId: null,
  calibrationLufs: null,
  profileId: null,
};

export function PrescriptionCard({
  rx,
  listIndex,
  presetName,
  scene = false,
  soundScene = null,
  soundFootswitch = null,
  nodes = [],
  footswitches = [],
  stimulus = DEFAULT_STIMULUS,
}: PrescriptionCardProps) {
  const { t } = useTheme();
  const [phase, setPhase] = useState<Phase>("draft");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [clips, setClips] = useState<DoctorApplyResult | null>(null);
  const [acked, setAcked] = useState(false);
  // Snapshot of the exact ops sent to doctorApply — `rx` is a live prop and
  // can be replaced (e.g. MatchCard recomputing on a new reference pick)
  // while this card sits in "applied"; runSave must persist what was
  // actually applied/auditioned, not whatever `rx.ops` reads at save time.
  const [appliedOps, setAppliedOps] = useState<DoctorRx["ops"] | null>(null);

  // Global guard: every card targets the device's ONE edit buffer, so only one
  // card app-wide may hold an applied-but-unsaved edit at a time (see `applyLock`).
  const cardId = useId();
  const lock = useApplyLock();
  const lockedByOther =
    lock.activeCard !== null && lock.activeCard.id !== cardId;

  // The card itself can unmount while an edit sits applied-but-unsaved on the
  // device (row collapse, or a Match-reference swap that stops rendering this
  // card) — with no mounted UI left to Save/Discard, that would strand the
  // device edit AND the app-wide lock. `discardIfMine` is a stable function
  // (always the same reference, however `lock`'s wrapping object churns from
  // OTHER cards' acquire/release) that no-ops unless THIS card still holds
  // the lock, so calling it unconditionally on unmount is safe and needs no
  // extra phase/ownership bookkeeping here.
  const { discardIfMine } = lock;
  const mountedRef = useRef(true);
  useEffect(() => {
    // Re-arm on every effect run: the cleanup below flips it false, and without
    // this reset a dep change (new cardId/listIndex) would leave it permanently
    // false, silently dropping every later async success/error state update.
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      discardIfMine(cardId, listIndex);
    };
  }, [discardIfMine, cardId, listIndex]);

  // Only oneclick / chain non-scene prescriptions have an Apply path; advisory and
  // scene-consistency cards are static.
  const applicable = !scene && (rx.kind === "oneclick" || rx.kind === "chain");

  const card: CSSProperties = doctorCard(t, {
    border: phase === "saved" ? t.good : undefined,
  });

  // Shared by the advisory / scene / shared-block caption lines below.
  const noteLine: CSSProperties = {
    fontFamily: t.sans,
    fontSize: t.fsLabel,
    color: t.mutedInk,
    marginTop: t.space4,
    lineHeight: 1.5,
  };

  async function runApply() {
    setBusy(true);
    setError(null);
    // Snapshot rx.ops now — rx is a live prop and may be replaced (e.g.
    // MatchCard recomputing on a new reference pick) before Save is clicked.
    const ops = rx.ops;
    // Take the lock BEFORE the await: the device's edit buffer is dirty from the
    // moment the command starts, so a sibling Apply during the in-flight window
    // would clobber it. Released in the catch (a failed apply auto-restores).
    lock.acquire(cardId, listIndex);
    try {
      const res = await doctorApply({
        listIndex,
        name: presetName,
        ops,
        topologyId: stimulus.topologyId,
        calibrationLufs: stimulus.calibrationLufs,
        profileId: stimulus.profileId,
        scene: soundScene,
        footswitch: soundFootswitch,
        nodes,
        footswitches,
      });
      // If we unmounted while this was in flight (row collapsed, or the
      // Match reference swapped away), the unmount cleanup above already
      // fired `discardIfMine` — `lock.acquire` ran before the await, so
      // ownership was already established for it to find and clean up,
      // regardless of whether doctorApply itself had landed yet. Skip the
      // local state updates here; there's no mounted UI left to show them.
      if (mountedRef.current) {
        setClips(res);
        setAppliedOps(ops);
        setPhase("applied");
      }
    } catch (e) {
      lock.release(cardId);
      if (mountedRef.current) {
        setError(e instanceof Error ? e.message : "Couldn't apply this fix.");
      }
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  async function runDiscard() {
    setBusy(true);
    setError(null);
    try {
      await doctorDiscard(listIndex);
      setPhase("draft");
      setClips(null);
      setAppliedOps(null);
      setAcked(false);
      lock.release(cardId);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Couldn't discard.");
    } finally {
      setBusy(false);
    }
  }

  async function runSave() {
    if (appliedOps == null) return;
    setBusy(true);
    setError(null);
    try {
      await doctorSave(listIndex, presetName, appliedOps);
      setPhase("saved");
      lock.release(cardId);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Couldn't save to the preset.");
    } finally {
      setBusy(false);
    }
  }

  const errorBlock =
    error != null ? (
      <div
        style={{
          marginTop: t.space5,
          fontFamily: t.sans,
          fontSize: t.fsLabel,
          color: t.warn,
        }}
      >
        {error}
      </div>
    ) : null;

  if (phase === "saved") {
    return (
      <div style={card}>
        <div style={{ display: "flex", alignItems: "center", gap: t.space4 }}>
          <span
            style={{
              width: 20,
              height: 20,
              borderRadius: 999,
              background: t.goodSoft,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
            }}
          >
            <Icon name="check" size={12} stroke={t.good} />
          </span>
          <span
            style={{ fontFamily: t.serif, fontSize: t.fsName, color: t.ink }}
          >
            Saved to the preset.
          </span>
        </div>
        <div
          style={{
            marginTop: t.space3,
            fontFamily: t.sans,
            fontSize: t.fsLabel,
            color: t.mutedInk,
            lineHeight: 1.5,
          }}
        >
          {
            "This is on the unit now and can't be undone from here — restore your last Pro Control backup to roll it back."
          }
        </div>
      </div>
    );
  }

  if (phase === "applied" && clips) {
    return (
      <div style={card}>
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsData2,
            letterSpacing: t.lsTag,
            textTransform: "uppercase",
            color: t.accentDeep,
            background: t.accentSoft,
            padding: `${String(t.space2)}px ${String(t.space4)}px`,
            borderRadius: t.rPill,
          }}
        >
          Applied to the unit · not saved
        </span>
        <div style={{ marginTop: t.space6 }}>
          <ABAudition
            beforeClip={clips.beforeClip}
            afterClip={clips.afterClip}
          />
        </div>
        <div
          style={{
            marginTop: t.space6,
            paddingTop: t.space6,
            borderTop: `0.5px solid ${t.hairline}`,
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: t.space6,
            flexWrap: "wrap",
          }}
        >
          <BackupAckLabel checked={acked} onChange={setAcked} />
          <div style={{ display: "flex", gap: t.space4, flexShrink: 0 }}>
            <Button
              variant="ghost"
              small
              disabled={busy}
              onClick={() => {
                void runDiscard();
              }}
            >
              Discard
            </Button>
            <Button
              variant="primary"
              small
              icon="save"
              disabled={!acked || busy}
              onClick={() => {
                void runSave();
              }}
            >
              Save to preset
            </Button>
          </div>
        </div>
        {errorBlock}
      </div>
    );
  }

  // draft — also the terminal state for advisory / scene cards (no apply).
  return (
    <div style={card}>
      <div style={{ display: "flex", gap: t.space5 }}>
        <div
          style={{
            width: 28,
            height: 28,
            borderRadius: 7,
            background: t.bgAlt,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
          }}
        >
          <Icon name={KIND_ICON[rx.kind]} size={15} stroke={t.ink2} />
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: t.space4,
              flexWrap: "wrap",
            }}
          >
            <span
              style={{ fontFamily: t.serif, fontSize: t.fsName, color: t.ink }}
            >
              {rx.title}
            </span>
            <Tag tone={rx.kind === "advisory" ? "neutral" : "accent"} uppercase>
              {KIND_BADGE[rx.kind]}
            </Tag>
          </div>
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsBody,
              color: t.ink2,
              marginTop: t.space2,
              lineHeight: 1.5,
            }}
          >
            {rx.detail}
          </div>
          {rx.kind !== "advisory" && (
            <div
              style={{
                fontFamily: t.mono,
                fontSize: t.fsData2,
                color: t.mutedInk,
                marginTop: t.space3,
              }}
            >
              {rx.cpuNote}
            </div>
          )}
          {rx.chain && (
            <div style={{ marginTop: t.space5 }}>
              <SignalChainView size="sm" graph={chainGraph(rx.chain)} />
            </div>
          )}
          {rx.kind === "advisory" && (
            <div style={noteLine}>{ADVISORY_LINE}</div>
          )}
          {scene && <div style={noteLine}>{SCENE_LINE}</div>}
          {errorBlock}
          {applicable && (
            <div style={{ marginTop: t.space8 }}>
              <Button
                variant="primary"
                small
                icon={busy ? undefined : "check"}
                disabled={busy || lockedByOther}
                onClick={() => {
                  void runApply();
                }}
              >
                {busy ? (
                  <span
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: t.space3,
                    }}
                  >
                    <Spinner size={13} stroke={t.onInk} strokeWidth={1.8} />
                    Applying…
                  </span>
                ) : (
                  "Apply to the unit"
                )}
              </Button>
              {lockedByOther && (
                <div
                  style={{
                    marginTop: t.space3,
                    fontFamily: t.sans,
                    fontSize: t.fsLabel,
                    color: t.mutedInk,
                  }}
                >
                  Save or discard the applied fix first.
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export default PrescriptionCard;
