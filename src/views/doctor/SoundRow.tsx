// src/views/doctor/SoundRow.tsx — one checked sound as a dense, expandable triage
// row: severity dot · name + FS/BASE tag · diagnosis chips (or "Sounds good" / an
// error) · band sparkline · LUFS · caret. Clicking a row toggles its expansion
// (open state threaded from the results page); the expanded region holds the
// shared-block caption, then per-diagnosis: the explainer, the full BandMeter, and
// the prescription card(s), then the cut-through estimate + the Match-reference
// picker/card. Only errored or unrun rows (no usable capture) are not expandable — a clean
// row still expands (to pick it as a reference / show its cut-through read).

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Tag } from "../../ui/Tag";
import { Button } from "../../ui/primitives";
import { BandSpark } from "./BandSpark";
import { CutThroughCard } from "./CutThroughCard";
import { DiagnosisChip } from "./DiagnosisChip";
import { LevelIndicator } from "./LevelIndicator";
import { MatchCard } from "./MatchCard";
import { DiagnosisPanel } from "./DiagnosisPanel";
import { findingRowHeaderStyle } from "./findingRow";
import { SevDot } from "./SevDot";
import { bandLayoutsMatch } from "./matchModel";
import { offbranchStatus } from "../level/leveling";
import {
  isPossible,
  possibleLabel,
  sevTone,
  sortedDiags,
  soundSev,
} from "./severity";
import type {
  DoctorDiag,
  DoctorInputArg,
  DoctorSoundResult,
  SilenceHint,
} from "../../lib/types";

/** The Doctor capture's own silent-inject error string (`leveller::NO_SIGNAL_CAPTURED`,
 *  surfaced verbatim as `DoctorSoundResult.error`) — the ONE case a backup-scan silence
 *  hint refines. `FLOOR_READ_ERR` (a stimulus that never reached the device — a
 *  different failure class, not "output routing") is deliberately left alone. */
const NO_SIGNAL_CAPTURED = "no signal captured";

const SHARED_CAPTION =
  "This block is shared — the change affects all sounds of this preset.";

/** The row text for a sound a stopped run never reached. */
const NOT_CHECKED = "Not checked — the run was stopped";

// ---- shared-block detection (hoisted from PrescriptionCard) -----------------

/** True when any fix on any of this sound's diagnoses touches a block OUTSIDE the
 *  footswitch's own toggled set — i.e. a shared block whose edit affects every
 *  sound of the preset. Only possible for FS sounds (`ownNodeIds != null`). */
function affectsSharedBlock(
  diags: DoctorDiag[],
  ownNodeIds: string[] | undefined,
): boolean {
  if (ownNodeIds == null) return false;
  return diags.some((d) =>
    d.rx.some((rx) =>
      rx.ops.some((op) => {
        const n = op.kind === "param" ? op.nodeId : op.fenderId;
        return !ownNodeIds.includes(n);
      }),
    ),
  );
}

export interface SoundRowProps {
  sound: DoctorSoundResult;
  listIndex: number;
  presetName: string;
  /** The nodes this footswitch sound's own switch toggles; undefined for
   *  Base/scene sounds (drives the "shared block" caption). */
  ownNodeIds?: string[];
  /** The EXACT input `doctor_check` diagnosed this sound with — its chain, its
   *  FULL footswitch roster and its stimulus. Threaded into every prescription
   *  card so the A/B captures under the same context and the same forced
   *  bypasses (`derived_force_bypass` forces off every on/off block of the
   *  roster it is given, so a different roster auditions a different sound). */
  input?: DoctorInputArg;
  /** This sound's preset's backup-scan silence hint (same source + rendering
   *  as the Level tab's offbranch row status, `leveling.ts::offbranchStatus`)
   *  — refines a "no signal captured" error into the JSON-visible cause when
   *  the backup scan found one. */
  silenceHint?: SilenceHint;
  open: boolean;
  onToggle: () => void;
  /** This row's page-wide composite id (`${listIndex}|${sound.key}`) — how
   *  the Match-reference picker identifies a sound across presets. */
  id: string;
  /** The page-wide picked Match-reference sound (any preset in the same
   *  run), or null before one is picked. */
  referenceSound: DoctorSoundResult | null;
  referenceId: string | null;
  onSetReference: (id: string) => void;
  onClearReference: () => void;
}

export function SoundRow({
  sound,
  listIndex,
  presetName,
  ownNodeIds,
  input,
  silenceHint,
  open,
  onToggle,
  id,
  referenceSound,
  referenceId,
  onSetReference,
  onClearReference,
}: SoundRowProps) {
  const { t } = useTheme();
  const hasDiags = sound.diags.length > 0;
  const isError = sound.status === "error";
  // Never captured — the run was stopped first. No loudness, no spectrum.
  const isUnrun = sound.status === "unrun";
  // A row with no usable capture: flat, no metrics, not expandable.
  const flat = isError || isUnrun;
  // A silent-inject capture refines to the SAME offbranch status text the
  // Level tab shows for the identical failure — never the raw backend string.
  // Any other error (a real device/connection failure) is shown verbatim.
  const errorText = sound.error?.includes(NO_SIGNAL_CAPTURED)
    ? offbranchStatus(silenceHint)
    : sound.error;
  const coverageGated = sound.skippedBandCount > 0;
  const sev = soundSev(sound);
  const tone = sevTone(t, sev);
  const isTagged = sound.scene != null || sound.footswitch != null;
  // Worst-first, confident above "possible" — one order for the chips + panels.
  const diags = sortedDiags(sound.diags);
  const hotBands = [...new Set(diags.flatMap((d) => d.bands))];
  const shared = hasDiags && affectsSharedBlock(sound.diags, ownNodeIds);
  const lufs =
    sound.integratedLufs != null && Number.isFinite(sound.integratedLufs)
      ? sound.integratedLufs
      : null;
  // A clean sound still expands — to show its cut-through read or let the
  // player pick it as the Match reference. Only an errored capture (no
  // usable balanceDb) or an unrun one stays flat/non-interactive.
  const expandable = !flat;
  const isReference = id === referenceId;
  // A null cutThrough on a non-errored row marks a DEGENERATE capture (the
  // presence/low contrast came out non-finite) — such a spectrum must neither
  // serve as a Match reference nor receive generated EQ moves.
  const hasMatchSpectrum = sound.cutThrough != null;
  // Label-identity, not just count: Bass and Bass VI are both 7 bands, and a
  // cross-layout pair must not offer a Match section that can't render.
  const canMatch =
    hasMatchSpectrum &&
    referenceSound?.cutThrough != null &&
    !isReference &&
    bandLayoutsMatch(referenceSound, sound);

  const headerStyle = findingRowHeaderStyle(t, open);
  // An expandable row's header is a real disclosure button; a flat (error /
  // unrun) row has nothing to open, so it stays a plain, inert div.
  const header = (
    <>
      <SevDot sev={sev} />
      {/* label + tag */}
      <span
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space4,
          minWidth: 96,
          flexShrink: 0,
        }}
      >
        <span
          style={{
            fontFamily: t.serif,
            fontSize: t.fsName2,
            color: hasDiags ? t.ink : t.ink2,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
            maxWidth: 150,
          }}
        >
          {sound.label}
        </span>
        {sound.tag != null && sound.tag !== "" && (
          <Tag tone={isTagged ? "accent" : "neutral"}>{sound.tag}</Tag>
        )}
      </span>
      {/* middle: clear state, error, or diagnosis chips */}
      <span
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          alignItems: "center",
          gap: t.space3,
          overflow: "hidden",
        }}
      >
        {flat ? (
          <span
            style={{
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: isError ? t.warn : t.mutedInk,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {isError ? errorText : NOT_CHECKED}
          </span>
        ) : hasDiags ? (
          diags.map((d) => (
            <span
              key={d.key}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: t.space3,
              }}
            >
              <DiagnosisChip
                label={possibleLabel(d)}
                sev={d.sev}
                possible={isPossible(d)}
              />
              <LevelIndicator
                level={d.fromLevel}
                sev={d.sev}
                finding={possibleLabel(d)}
                size="tiny"
              />
            </span>
          ))
        ) : (
          <span
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: t.space3,
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: t.good,
            }}
          >
            <Icon name="check" size={12} stroke={t.good} />
            Sounds good
            <LevelIndicator
              level="clean"
              sev="ok"
              finding="Sounds good"
              size="tiny"
            />
          </span>
        )}
        {!flat && coverageGated && (
          <span
            title={`Some checks skipped — signal too quiet on ${String(sound.skippedBandCount)} band${sound.skippedBandCount === 1 ? "" : "s"}`}
            style={{ display: "inline-flex", flexShrink: 0, opacity: 0.7 }}
          >
            <Icon name="info" size={12} stroke={t.mutedInk} />
          </span>
        )}
      </span>
      {/* metrics: sparkline + LUFS (suppressed on error and unrun rows) */}
      {!flat && (
        <>
          <BandSpark
            balanceDb={sound.balanceDb}
            bandCount={sound.bandLabels.length}
            hotBands={hotBands}
            color={tone.fg}
            muted={!hasDiags}
          />
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMeta,
              color: hasDiags ? t.ink2 : t.faint,
              fontVariantNumeric: "tabular-nums",
              width: 62,
              textAlign: "right",
              flexShrink: 0,
            }}
          >
            {lufs != null ? (
              <>
                {lufs.toFixed(1)}
                <span style={{ color: t.faint }}> LUFS</span>
              </>
            ) : (
              "—"
            )}
          </span>
        </>
      )}
      {/* caret */}
      <span
        style={{
          width: 14,
          flexShrink: 0,
          display: "inline-flex",
          justifyContent: "center",
          opacity: expandable ? 0.6 : 0,
        }}
      >
        {expandable && (
          <span
            style={{
              display: "inline-flex",
              transform: open ? "rotate(90deg)" : "none",
              transition: "transform 0.12s",
            }}
          >
            <Icon name="chev-right" size={13} stroke={tone.fg} />
          </span>
        )}
      </span>
    </>
  );

  return (
    <div
      // e2e hook: count a preset card's rendered sound rows, mirroring
      // TargetEditCard/BlockEditor's `data-target-card`/`data-candidate`.
      data-sound-row={sound.key}
      style={{ borderTop: `0.5px solid ${t.hairline}` }}
    >
      {expandable ? (
        <Button
          variant="bare"
          aria-expanded={open}
          onClick={onToggle}
          style={{ ...headerStyle, width: "100%", textAlign: "left" }}
        >
          {header}
        </Button>
      ) : (
        <div style={headerStyle}>{header}</div>
      )}

      {open && expandable && (
        <div
          style={{
            padding: `${String(t.space1)}px ${String(t.space5)}px ${String(t.space7)}px ${String(t.space11)}px`,
            display: "flex",
            flexDirection: "column",
            gap: t.space6,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: t.space4 }}>
            {isReference ? (
              <>
                <Tag tone="accent">Reference</Tag>
                <Button variant="ghost" size="sm" onClick={onClearReference}>
                  Clear reference
                </Button>
              </>
            ) : hasMatchSpectrum ? (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  onSetReference(id);
                }}
              >
                Set as reference
              </Button>
            ) : null}
          </div>
          {shared && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: t.space4,
                fontFamily: t.sans,
                fontSize: t.fsLabel,
                lineHeight: 1.4,
                color: t.accentDeep,
                background: t.accentSoft,
                border: `0.5px solid ${t.accentBorderSoft}`,
                borderRadius: t.rCard,
                padding: `${String(t.space3)}px ${String(t.space5)}px`,
              }}
            >
              <Icon name="link" size={13} stroke={t.accentDeep} />
              <span>{SHARED_CAPTION}</span>
            </div>
          )}
          {diags.map((diag) => (
            <DiagnosisPanel
              key={diag.key}
              diag={diag}
              sound={sound}
              cardIdBase={`${id}|${diag.key}`}
              listIndex={listIndex}
              presetName={presetName}
              input={input}
            />
          ))}
          {sound.cutThrough && <CutThroughCard cutThrough={sound.cutThrough} />}
          {canMatch && (
            <MatchCard
              cardId={`${id}|match`}
              sound={sound}
              reference={referenceSound}
              listIndex={listIndex}
              presetName={presetName}
              nodes={input?.nodes ?? []}
              footswitches={input?.footswitches ?? []}
              stimulus={input}
            />
          )}
        </div>
      )}
    </div>
  );
}
