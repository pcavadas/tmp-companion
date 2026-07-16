// src/views/doctor/MatchCard.tsx — the "Match reference" card: computes the
// EQ-10 moves (matchModel.ts) that bring THIS sound's spectrum toward the
// player-picked reference sound (same run, same band layout — the caller
// gates that), and offers to apply them via a synthetic DoctorRx routed
// through PrescriptionCard's existing apply -> A/B audition -> ack-gated
// save/discard flow. No new backend command, no new capture: everything it
// needs (balanceDb, the preset's chain) is already on the wire.

import { useTheme } from "../../theme/ThemeContext";
import { signedDb } from "../../lib/format";
import { cpuForBid } from "../../models/cpu";
import { PrescriptionCard } from "./PrescriptionCard";
import { doctorCard } from "./severity";
import {
  eqBandLabel,
  eqMovesFor,
  lastGuitarGroup,
  matchDeltas,
  matchResidualLarge,
} from "./matchModel";
import type {
  DoctorOp,
  DoctorRx,
  DoctorSoundResult,
  FootswitchInfo,
  GraphNode,
} from "../../lib/types";

const EQ10_STEREO = "ACD_TenBandEQStereo";

const RESIDUAL_LINE =
  "Big spectral gap — an EQ can only get partway; a different cab/amp choice may be the honest fix.";

export interface MatchCardProps {
  sound: DoctorSoundResult;
  reference: DoctorSoundResult;
  listIndex: number;
  presetName: string;
  nodes: GraphNode[];
  footswitches: FootswitchInfo[];
}

export function MatchCard({
  sound,
  reference,
  listIndex,
  presetName,
  nodes,
  footswitches,
}: MatchCardProps) {
  const { t } = useTheme();
  const deltas = matchDeltas(reference.balanceDb, sound.balanceDb);
  const moves = eqMovesFor(deltas, sound.bandLabels);
  const residualLarge = matchResidualLarge(deltas);

  // Spectrally already close, and no gap worth flagging → nothing to show.
  if (moves.length === 0 && !residualLarge) return null;

  const movesLine = moves
    .map((m) => `${eqBandLabel(m.controlId)} ${signedDb(m.gainDb)} dB`)
    .join(" · ");

  const residualLine = residualLarge ? (
    <div
      style={{
        fontFamily: t.sans,
        fontSize: t.fsLabel,
        color: t.warn,
        marginTop: t.space3,
        lineHeight: 1.5,
      }}
    >
      {RESIDUAL_LINE}
    </div>
  ) : null;

  const groupId = lastGuitarGroup(nodes);

  // No guitar chain to insert into, or nothing actionable → read-only, no
  // Apply (PrescriptionCard's apply path always needs a real op to send).
  if (groupId == null || moves.length === 0) {
    return (
      <div style={doctorCard(t)}>
        <div style={{ fontFamily: t.serif, fontSize: t.fsName, color: t.ink }}>
          Match reference
        </div>
        {moves.length > 0 && (
          <div
            style={{
              fontFamily: t.mono,
              fontSize: 12.5,
              color: t.mutedInk,
              marginTop: t.space2,
            }}
          >
            {movesLine}
          </div>
        )}
        {residualLine}
      </div>
    );
  }

  const cost = cpuForBid(EQ10_STEREO);
  const cpuNote = cost == null ? "" : `+${cost.toFixed(1)}% CPU`;

  const ops: DoctorOp[] = [
    {
      kind: "insert_node",
      groupId,
      beforeFenderId: null,
      fenderId: EQ10_STEREO,
      params: moves.map((m): [string, number] => [m.controlId, m.gainDb]),
    },
  ];

  const rx: DoctorRx = {
    kind: "chain",
    title: "Match reference",
    detail: `Moves ${sound.label} toward ${reference.label}: ${movesLine}.`,
    cpuNote,
    ops,
  };

  return (
    <div>
      <PrescriptionCard
        rx={rx}
        listIndex={listIndex}
        presetName={presetName}
        soundScene={sound.scene}
        soundFootswitch={sound.footswitch}
        nodes={nodes}
        footswitches={footswitches}
      />
      {residualLine}
    </div>
  );
}

export default MatchCard;
