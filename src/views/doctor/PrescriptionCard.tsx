// src/views/doctor/PrescriptionCard.tsx — one prescription with its lifecycle:
// draft → applied → saved. Advisory cards are static (the player turns the knob,
// nothing to apply). The apply path writes LIVE (unsaved) and the save is gated on
// the backup acknowledgment, mirroring the Leveling / Copy save-disclaimer model.

import { useId, useState } from "react";
import type { CSSProperties } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon, type IconName } from "../../ui/Icon";
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
  DoctorRx,
  DoctorRxKind,
} from "../../lib/types";

const KIND_ICON: Record<DoctorRxKind, IconName> = {
  oneclick: "sliders",
  chain: "cable",
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
}

export function PrescriptionCard({
  rx,
  listIndex,
  presetName,
  scene = false,
}: PrescriptionCardProps) {
  const { t } = useTheme();
  const [phase, setPhase] = useState<Phase>("draft");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [clips, setClips] = useState<DoctorApplyResult | null>(null);
  const [acked, setAcked] = useState(false);

  // Global guard: every card targets the device's ONE edit buffer, so only one
  // card app-wide may hold an applied-but-unsaved edit at a time (see `applyLock`).
  const cardId = useId();
  const lock = useApplyLock();
  const lockedByOther =
    lock.activeCard !== null && lock.activeCard.id !== cardId;

  // Only oneclick / chain non-scene prescriptions have an Apply path; advisory and
  // scene-consistency cards are static.
  const applicable = !scene && (rx.kind === "oneclick" || rx.kind === "chain");

  const card: CSSProperties = {
    flexShrink: 0,
    border: `0.5px solid ${phase === "saved" ? t.good : t.hairlineStrong}`,
    borderRadius: 10,
    background: t.bg,
    padding: 12,
  };

  async function runApply() {
    setBusy(true);
    setError(null);
    // Take the lock BEFORE the await: the device's edit buffer is dirty from the
    // moment the command starts, so a sibling Apply during the in-flight window
    // would clobber it. Released in the catch (a failed apply auto-restores).
    lock.acquire(cardId, listIndex);
    try {
      const res = await doctorApply({
        listIndex,
        name: presetName,
        ops: rx.ops,
        topologyId: null,
        calibrationLufs: null,
      });
      setClips(res);
      setPhase("applied");
    } catch (e) {
      lock.release(cardId);
      setError(e instanceof Error ? e.message : "Couldn't apply this fix.");
    } finally {
      setBusy(false);
    }
  }

  async function runDiscard() {
    setBusy(true);
    setError(null);
    try {
      await doctorDiscard(listIndex);
      setPhase("draft");
      setClips(null);
      setAcked(false);
      lock.release(cardId);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Couldn't discard.");
    } finally {
      setBusy(false);
    }
  }

  async function runSave() {
    setBusy(true);
    setError(null);
    try {
      await doctorSave(listIndex, presetName);
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
          marginTop: 10,
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
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
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
            marginTop: 6,
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
            padding: "3px 8px",
            borderRadius: t.rPill,
          }}
        >
          Applied to the unit · not saved
        </span>
        <div style={{ marginTop: 12 }}>
          <ABAudition
            beforeClip={clips.beforeClip}
            afterClip={clips.afterClip}
          />
        </div>
        <div
          style={{
            marginTop: 12,
            paddingTop: 12,
            borderTop: `0.5px solid ${t.hairline}`,
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 12,
            flexWrap: "wrap",
          }}
        >
          <BackupAckLabel checked={acked} onChange={setAcked} />
          <div style={{ display: "flex", gap: 8, flexShrink: 0 }}>
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
      <div style={{ display: "flex", gap: 10 }}>
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
              gap: 8,
              flexWrap: "wrap",
            }}
          >
            <span
              style={{ fontFamily: t.serif, fontSize: t.fsName, color: t.ink }}
            >
              {rx.title}
            </span>
            <span
              style={{
                fontFamily: t.mono,
                fontSize: t.fsTag,
                letterSpacing: t.lsTag,
                textTransform: "uppercase",
                padding: "2px 6px",
                borderRadius: t.rSm,
                color: rx.kind === "advisory" ? t.mutedInk : t.accentDeep,
                background: rx.kind === "advisory" ? t.bgAlt : t.accentSoft,
                whiteSpace: "nowrap",
              }}
            >
              {KIND_BADGE[rx.kind]}
            </span>
          </div>
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsBody,
              color: t.ink2,
              marginTop: 4,
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
                marginTop: 6,
              }}
            >
              {rx.cpuNote}
            </div>
          )}
          {rx.chain && (
            <div style={{ marginTop: 10 }}>
              <SignalChainView size="sm" graph={chainGraph(rx.chain)} />
            </div>
          )}
          {rx.kind === "advisory" && (
            <div
              style={{
                fontFamily: t.sans,
                fontSize: t.fsLabel,
                color: t.mutedInk,
                marginTop: 8,
                lineHeight: 1.5,
              }}
            >
              {ADVISORY_LINE}
            </div>
          )}
          {scene && (
            <div
              style={{
                fontFamily: t.sans,
                fontSize: t.fsLabel,
                color: t.mutedInk,
                marginTop: 8,
                lineHeight: 1.5,
              }}
            >
              {SCENE_LINE}
            </div>
          )}
          {errorBlock}
          {applicable && (
            <div style={{ marginTop: 10 }}>
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
                      gap: 6,
                    }}
                  >
                    <span
                      className="tmp-spin"
                      style={{ display: "inline-flex" }}
                    >
                      <Icon
                        name="spinner"
                        size={13}
                        stroke={t.onInk}
                        strokeWidth={1.8}
                      />
                    </span>
                    Applying…
                  </span>
                ) : (
                  "Apply to the unit"
                )}
              </Button>
              {lockedByOther && (
                <div
                  style={{
                    marginTop: 6,
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
