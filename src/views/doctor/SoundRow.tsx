// src/views/doctor/SoundRow.tsx — one checked sound as a dense, expandable triage
// row: severity dot · name + FS/BASE tag · diagnosis chips (or "Sounds good" / an
// error) · band sparkline · LUFS · caret. Clicking a problem row toggles its
// expansion (open state threaded from the results page); the expanded region holds
// the shared-block caption, then per-diagnosis: the explainer, the full BandMeter,
// and the prescription card(s). Clear and errored rows are not expandable.

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Tag } from "../../ui/Tag";
import { BandMeter } from "./BandMeter";
import { BandSpark } from "./BandSpark";
import { DiagnosisChip } from "./DiagnosisChip";
import { LevelIndicator } from "./LevelIndicator";
import { PrescriptionCard } from "./PrescriptionCard";
import { diagSevLabel, sevTone, soundSev, type Sev } from "./severity";
import type { DoctorDiag, DoctorSoundResult } from "../../lib/types";

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
  open: boolean;
  onToggle: () => void;
}

export function SoundRow({
  sound,
  listIndex,
  presetName,
  ownNodeIds,
  open,
  onToggle,
}: SoundRowProps) {
  const { t } = useTheme();
  const hasDiags = sound.diags.length > 0;
  const isError = sound.error != null;
  const sev = soundSev(sound);
  const tone = sevTone(t, sev);
  const isTagged = sound.scene != null || sound.footswitch != null;
  const hotBands = [...new Set(sound.diags.flatMap((d) => d.bands))];
  const shared = hasDiags && affectsSharedBlock(sound.diags, ownNodeIds);
  const lufsOk = Number.isFinite(sound.integratedLufs);

  return (
    <div style={{ borderTop: `0.5px solid ${t.hairline}` }}>
      <div
        onClick={hasDiags ? onToggle : undefined}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          minHeight: 38,
          padding: "5px 8px 5px 6px",
          cursor: hasDiags ? "pointer" : "default",
          background: open ? t.rowSel : "transparent",
        }}
      >
        <SevDot sev={sev} />
        {/* label + tag */}
        <span
          style={{
            display: "flex",
            alignItems: "center",
            gap: 7,
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
            gap: 5,
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
            sound.diags.map((d) => (
              <span
                key={d.key}
                style={{ display: "inline-flex", alignItems: "center", gap: 6 }}
              >
                <DiagnosisChip label={d.label} sev={d.sev} />
                <LevelIndicator
                  level={d.fromLevel}
                  sev={d.sev}
                  finding={d.label}
                  size="tiny"
                />
              </span>
            ))
          ) : (
            <span
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 5,
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
            opacity: hasDiags ? 0.6 : 0,
          }}
        >
          {hasDiags && (
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

      {open && hasDiags && (
        <div
          style={{
            padding: "2px 10px 14px 30px",
            display: "flex",
            flexDirection: "column",
            gap: 12,
          }}
        >
          {shared && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 7,
                fontFamily: t.sans,
                fontSize: t.fsLabel,
                lineHeight: 1.4,
                color: t.accentDeep,
                background: t.accentSoft,
                border: `0.5px solid rgba(217,119,87,0.3)`,
                borderRadius: 8,
                padding: "6px 10px",
              }}
            >
              <Icon name="link" size={13} stroke={t.accentDeep} />
              <span>{SHARED_CAPTION}</span>
            </div>
          )}
          {sound.diags.map((diag) => {
            const dTone = sevTone(t, diag.sev);
            return (
              <div
                key={diag.key}
                style={{ borderLeft: `2px solid ${dTone.fg}`, paddingLeft: 13 }}
              >
                <div
                  style={{
                    display: "flex",
                    alignItems: "baseline",
                    gap: 8,
                    flexWrap: "wrap",
                  }}
                >
                  <span
                    style={{
                      fontFamily: t.sans,
                      fontSize: 13,
                      fontWeight: 600,
                      color: dTone.fg,
                    }}
                  >
                    {diag.label}
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
                    finding={diag.label}
                    size="rich"
                  />
                </div>
                <div
                  style={{
                    fontFamily: t.sans,
                    fontSize: 12.5,
                    lineHeight: 1.5,
                    color: t.ink2,
                    marginTop: 4,
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
                    gap: 10,
                    marginTop: 11,
                  }}
                >
                  {diag.rx.map((rx, i) => (
                    <PrescriptionCard
                      key={`${rx.kind}-${String(i)}`}
                      rx={rx}
                      listIndex={listIndex}
                      presetName={presetName}
                    />
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

export default SoundRow;
