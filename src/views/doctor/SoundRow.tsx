// src/views/doctor/SoundRow.tsx — one checked sound as a dense, expandable triage
// row: severity dot · name + FS/BASE tag · diagnosis chips (or "Sounds good" / an
// error) · band sparkline · LUFS · caret. Clicking a row toggles its expansion
// (open state threaded from the results page); the expanded region holds the
// shared-block caption, then per-diagnosis: the explainer, the full BandMeter, and
// the prescription card(s), then the cut-through estimate + the Match-reference
// picker/card. Only errored rows (no usable capture) are not expandable — a clean
// row still expands (to pick it as a reference / show its cut-through read).

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Tag } from "../../ui/Tag";
import { Button } from "../../ui/primitives";
import { BandMeter } from "./BandMeter";
import { BandSpark } from "./BandSpark";
import { CutThroughCard } from "./CutThroughCard";
import { DiagnosisChip } from "./DiagnosisChip";
import { LevelIndicator } from "./LevelIndicator";
import { MatchCard } from "./MatchCard";
import { PrescriptionCard } from "./PrescriptionCard";
import {
  diagSevLabel,
  isPossible,
  possibleLabel,
  sevTone,
  sortedDiags,
  soundSev,
  type Sev,
} from "./severity";
import type {
  DoctorDiag,
  DoctorSoundResult,
  FootswitchInfo,
  GraphNode,
} from "../../lib/types";

const SHARED_CAPTION =
  "This block is shared — the change affects all sounds of this preset.";

// ---- severity dot ----------------------------------------------------------

export interface SevDotProps {
  sev: Sev;
}

export function SevDot({ sev }: SevDotProps) {
  const { t } = useTheme();
  const tone = sevTone(t, sev);
  if (sev === "high") {
    return (
      <span
        style={{
          width: 9,
          height: 9,
          borderRadius: 999,
          background: tone.fg,
          boxShadow: `0 0 0 2.5px ${tone.soft}`,
          flexShrink: 0,
        }}
      />
    );
  }
  if (sev === "med") {
    return (
      <span
        style={{
          width: 8,
          height: 8,
          borderRadius: 999,
          background: tone.fg,
          flexShrink: 0,
        }}
      />
    );
  }
  return (
    <span
      style={{
        width: 7,
        height: 7,
        borderRadius: 999,
        border: `1.5px solid ${tone.fg}`,
        boxSizing: "border-box",
        flexShrink: 0,
      }}
    />
  );
}

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
  /** The preset's full chain + block-acting footswitches (same data
   *  `doctor_check` was given) — threaded into every prescription card so its
   *  A/B captures under the diagnosed sound's own context, not the as-saved
   *  base (`derived_force_bypass` needs the whole preset, not just this
   *  sound's own blocks). */
  nodes: GraphNode[];
  footswitches: FootswitchInfo[];
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
  nodes,
  footswitches,
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
  const isError = sound.error != null;
  const sev = soundSev(sound);
  const tone = sevTone(t, sev);
  const isTagged = sound.scene != null || sound.footswitch != null;
  // Worst-first, confident above "possible" — one order for the chips + panels.
  const diags = sortedDiags(sound.diags);
  const hotBands = [...new Set(diags.flatMap((d) => d.bands))];
  const shared = hasDiags && affectsSharedBlock(sound.diags, ownNodeIds);
  const lufsOk = Number.isFinite(sound.integratedLufs);
  // A clean sound still expands — to show its cut-through read or let the
  // player pick it as the Match reference. Only an errored capture (no
  // usable balanceDb) stays flat/non-interactive.
  const expandable = !isError;
  const isReference = id === referenceId;
  const canMatch =
    referenceSound != null &&
    !isReference &&
    referenceSound.bandLabels.length === sound.bandLabels.length;

  return (
    <div style={{ borderTop: `0.5px solid ${t.hairline}` }}>
      <div
        onClick={expandable ? onToggle : undefined}
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space5,
          minHeight: 38,
          padding: `${String(t.space3)}px ${String(t.space4)}px ${String(t.space3)}px ${String(t.space3)}px`,
          cursor: expandable ? "pointer" : "default",
          background: open ? t.rowSel : "transparent",
        }}
      >
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
              fontSize: 14,
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
          {isError ? (
            <span
              style={{
                fontFamily: t.sans,
                fontSize: t.fsLabel,
                color: t.warn,
                whiteSpace: "nowrap",
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              {sound.error}
            </span>
          ) : hasDiags ? (
            diags.map((d) => (
              <span
                key={d.key}
                style={{ display: "inline-flex", alignItems: "center", gap: 6 }}
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
        </span>
        {/* metrics: sparkline + LUFS (suppressed on error rows) */}
        {!isError && (
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
                fontSize: 10.5,
                color: hasDiags ? t.ink2 : t.faint,
                fontVariantNumeric: "tabular-nums",
                width: 62,
                textAlign: "right",
                flexShrink: 0,
              }}
            >
              {lufsOk ? (
                <>
                  {sound.integratedLufs.toFixed(1)}
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
      </div>

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
                <Button variant="ghost" small onClick={onClearReference}>
                  Clear reference
                </Button>
              </>
            ) : (
              <Button
                variant="ghost"
                small
                onClick={() => {
                  onSetReference(id);
                }}
              >
                Set as reference
              </Button>
            )}
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
                border: `0.5px solid rgba(217,119,87,0.3)`,
                borderRadius: 8,
                padding: `${String(t.space3)}px ${String(t.space5)}px`,
              }}
            >
              <Icon name="link" size={13} stroke={t.accentDeep} />
              <span>{SHARED_CAPTION}</span>
            </div>
          )}
          {diags.map((diag) => {
            const dTone = sevTone(t, diag.sev);
            const possible = isPossible(diag);
            return (
              <div
                key={diag.key}
                style={{
                  borderLeft: `2px solid ${dTone.fg}`,
                  paddingLeft: t.space6,
                }}
              >
                <div
                  style={{
                    display: "flex",
                    alignItems: "baseline",
                    gap: t.space4,
                    flexWrap: "wrap",
                  }}
                >
                  <span
                    style={{
                      fontFamily: t.sans,
                      fontSize: 13,
                      fontWeight: 600,
                      color: possible ? t.mutedInk : dTone.fg,
                    }}
                  >
                    {possibleLabel(diag)}
                  </span>
                  <span
                    style={{
                      fontFamily: t.mono,
                      fontSize: 8.5,
                      letterSpacing: "0.08em",
                      textTransform: "uppercase",
                      color: dTone.fg,
                    }}
                  >
                    {diagSevLabel(diag.sev)}
                  </span>
                  <span
                    style={{
                      fontFamily: t.mono,
                      fontSize: 10,
                      color: t.mutedInk,
                    }}
                  >
                    {diag.detail}
                  </span>
                  <LevelIndicator
                    level={diag.fromLevel}
                    sev={diag.sev}
                    finding={possibleLabel(diag)}
                    size="rich"
                  />
                </div>
                <div
                  style={{
                    fontFamily: t.sans,
                    fontSize: 12.5,
                    lineHeight: 1.5,
                    color: t.ink2,
                    marginTop: t.space2,
                  }}
                >
                  {diag.explain}
                </div>
                {diag.bands.length > 0 && (
                  <BandMeter
                    balanceDb={sound.balanceDb}
                    bandLabels={sound.bandLabels}
                    bands={diag.bands}
                    sev={diag.sev}
                  />
                )}
                <div
                  style={{
                    display: "flex",
                    flexDirection: "column",
                    gap: t.space5,
                    marginTop: t.space5,
                  }}
                >
                  {diag.rx.map((rx, i) => (
                    <PrescriptionCard
                      key={`${rx.kind}-${String(i)}`}
                      rx={rx}
                      listIndex={listIndex}
                      presetName={presetName}
                      soundScene={sound.scene}
                      soundFootswitch={sound.footswitch}
                      nodes={nodes}
                      footswitches={footswitches}
                    />
                  ))}
                </div>
              </div>
            );
          })}
          {sound.cutThrough && <CutThroughCard cutThrough={sound.cutThrough} />}
          {canMatch && (
            <MatchCard
              sound={sound}
              reference={referenceSound}
              listIndex={listIndex}
              presetName={presetName}
              nodes={nodes}
              footswitches={footswitches}
            />
          )}
        </div>
      )}
    </div>
  );
}

export default SoundRow;
