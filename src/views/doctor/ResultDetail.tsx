// src/views/doctor/ResultDetail.tsx — what an opened Results row shows (look B), under the
// row on a 900 px window or in the panel beside the list on a wide one.
//
// A SOUND: the shared-block line when a footswitch's fix changes a block every sound uses;
// one block per finding (FindingBlock); "Some bands were too quiet to check." under the
// last one; "Cutting through the mix" (hidden with no factory comparison); "To sound like
// <reference>" when another sound is the reference; and "Use as reference" (or the
// "Reference" badge with "Clear" on the reference itself).
// The SCENES row: how far the furthest sound sits from the main sound, a bar per sound
// (coloured on the row's own scale, `sceneBarTone`), and what to change (the backend's
// steps, then "Level this preset's scenes on the Level tab." unless the louder sound is
// the first scene, whose reading isn't certain).
// The FOOTSWITCHES row: what those assignments do, one line per footswitch.
// Every word is a step the player makes on the unit: Doctor never writes.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import { Badge } from "../../ui/Badge";
import { Icon } from "../../ui/Icon";
import { Button } from "../../ui/primitives";
import { signedDb } from "../../lib/format";
import type {
  DoctorPresetResult,
  DoctorSceneConsistency,
  DoctorSoundResult,
  GraphNode,
} from "../../lib/types";
import { FindingBlock } from "./FindingBlock";
import {
  cutThroughLine,
  footswitchLines,
  matchAdvice,
  RESULTS_TEXT,
  sceneLoudness,
  touchesSharedBlock,
  type ResultRow,
  type ResultsContext,
} from "./resultsModel";
import { sceneBarTone, sortedDiags } from "./severity";

/** The page-wide reference sound ("Use as reference"). */
export interface DoctorReference {
  id: string;
  sound: DoctorSoundResult;
  /** "Tweed Warm, Main sound". */
  name: string;
}

export interface ResultDetailProps {
  row: ResultRow;
  ctx: ResultsContext;
  /** The blocks this footswitch sound's own switch toggles (the shared-block line). */
  ownNodeIds?: string[];
  /** The preset's chain the sound was checked with (for "To sound like …"). */
  nodes: readonly GraphNode[];
  reference: DoctorReference | null;
  onSetReference: (id: string) => void;
  onClearReference: () => void;
}

export function ResultDetail(props: ResultDetailProps) {
  const { row, ctx } = props;
  if (row.kind === "scenes" && row.preset.sceneConsistency)
    return (
      <ScenesDetail
        ctx={ctx}
        preset={row.preset}
        sc={row.preset.sceneConsistency}
      />
    );
  if (row.kind === "footswitches")
    return <FootswitchesDetail preset={row.preset} />;
  if (row.sound) return <SoundDetail {...props} sound={row.sound} />;
  return null;
}

interface SectionProps {
  label?: string;
  children: ReactNode;
}

/** A titled part of an opened row, ruled off from the one above. */
function Section({ label, children }: SectionProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: t.sp4,
        padding: `${String(t.sp12 + t.sp2)}px 0`,
        borderTop: `1px solid ${t.separator}`,
        fontSize: t.fontBody,
        lineHeight: `${String(t.lineBody)}px`,
      }}
    >
      {label != null && <span style={sectionLabel(t)}>{label}</span>}
      {children}
    </div>
  );
}

interface FootnoteProps {
  icon?: "link";
  children: ReactNode;
}

/** A 12 px secondary line inside an opened sound. */
function Footnote({ icon, children }: FootnoteProps) {
  const { t } = useTheme();
  return (
    <span
      style={{
        display: "flex",
        alignItems: "center",
        gap: t.sp6,
        fontSize: t.fontCaption,
        lineHeight: `${String(t.lineCaption)}px`,
        color: t.textSecondary,
      }}
    >
      {icon && <Icon name={icon} size={12} stroke={t.textSecondary} />}
      {children}
    </span>
  );
}

interface SoundDetailProps extends ResultDetailProps {
  sound: DoctorSoundResult;
}

function SoundDetail({
  row,
  sound,
  ownNodeIds,
  nodes,
  reference,
  onSetReference,
  onClearReference,
}: SoundDetailProps) {
  const { t } = useTheme();
  const diags = sortedDiags(sound.diags);
  const tooQuiet = sound.skippedBandCount > 0;
  const shared = touchesSharedBlock(sound, ownNodeIds);
  const cut = cutThroughLine(sound);
  const isReference = reference?.id === row.id;
  const canBeReference = sound.cutThrough != null;
  const advice =
    reference && !isReference && canBeReference && reference.sound.cutThrough
      ? matchAdvice(sound, reference.sound, nodes)
      : null;

  return (
    <div data-result-detail={row.id}>
      {shared && (
        <div style={{ paddingTop: t.sp8 }}>
          <Footnote icon="link">{RESULTS_TEXT.shared}</Footnote>
        </div>
      )}
      {diags.map((d, i) => (
        <FindingBlock
          key={d.key}
          diag={d}
          sound={sound}
          footnote={
            tooQuiet && i === diags.length - 1 ? (
              <Footnote>{RESULTS_TEXT.tooQuiet}</Footnote>
            ) : undefined
          }
        />
      ))}
      {tooQuiet && diags.length === 0 && (
        <Section>
          <Footnote>{RESULTS_TEXT.tooQuiet}</Footnote>
        </Section>
      )}
      {cut != null && (
        <Section label={RESULTS_TEXT.cutThrough}>
          <span>{cut}</span>
          {sound.cutThrough?.advisory === true && (
            <span>{RESULTS_TEXT.cutThroughWeak}</span>
          )}
        </Section>
      )}
      {advice && reference && (
        <Section label={`To sound like ${reference.name}`}>
          {advice.step != null && <span>{advice.step}</span>}
          {advice.residual != null && <span>{advice.residual}</span>}
        </Section>
      )}
      {(isReference || canBeReference) && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: t.sp8,
            padding: `${String(t.sp8)}px 0 ${String(t.sp8 + t.sp2)}px`,
            borderTop: `1px solid ${t.separator}`,
          }}
        >
          {isReference ? (
            <>
              <Badge>Reference</Badge>
              <Button variant="link" onClick={onClearReference}>
                Clear
              </Button>
            </>
          ) : (
            <Button
              variant="link"
              onClick={() => {
                onSetReference(row.id);
              }}
            >
              Use as reference
            </Button>
          )}
        </div>
      )}
    </div>
  );
}

interface ScenesDetailProps {
  ctx: ResultsContext;
  preset: DoctorPresetResult;
  sc: DoctorSceneConsistency;
}

function ScenesDetail({ ctx, preset, sc }: ScenesDetailProps) {
  const { t } = useTheme();
  const { sentence, bars, steps } = sceneLoudness(ctx, preset, sc);
  const maxAbs = Math.max(0, ...bars.map((b) => Math.abs(b.deltaDb)));
  return (
    <div data-result-detail={`${String(preset.listIndex)}|consistency`}>
      <Section>
        <span>{sentence}</span>
        <div
          style={{
            marginTop: t.sp6,
            display: "grid",
            gridTemplateColumns: "fit-content(220px) minmax(0, 1fr) 64px",
            alignItems: "center",
            columnGap: t.sp8,
            fontSize: t.fontCaption,
          }}
        >
          {bars.map((b, i) => {
            const tone = sceneBarTone(b.deltaDb);
            const color = b.isRef
              ? t.meter
              : tone === "danger"
                ? t.danger
                : tone === "caution"
                  ? t.cautionLine
                  : t.good;
            const widthPct =
              maxAbs > 0 ? (Math.abs(b.deltaDb) / maxAbs) * 46 : 0;
            const neg = b.deltaDb < 0;
            const key = `${b.label}-${String(i)}`;
            return [
              <span
                key={`${key}:label`}
                style={{
                  padding: `${String(t.sp2)}px 0`,
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {b.label}
              </span>,
              <div
                key={`${key}:bar`}
                style={{ position: "relative", height: 12 }}
              >
                <div
                  style={{
                    position: "absolute",
                    left: "50%",
                    top: 0,
                    bottom: 0,
                    width: 1,
                    background: t.border,
                  }}
                />
                <div
                  style={{
                    position: "absolute",
                    top: 3,
                    bottom: 3,
                    width: `${String(widthPct)}%`,
                    left: neg ? undefined : "50%",
                    right: neg ? "50%" : undefined,
                    background: color,
                    borderRadius: 2,
                  }}
                />
              </div>,
              <span
                key={`${key}:db`}
                style={{
                  textAlign: "right",
                  color: t.textSecondary,
                  fontVariantNumeric: "tabular-nums",
                }}
              >
                {b.isRef ? "" : `${signedDb(b.deltaDb)} dB`}
              </span>,
            ];
          })}
        </div>
      </Section>
      <Section label={RESULTS_TEXT.whatToChange}>
        <ul style={{ margin: 0, paddingLeft: t.sp16 + t.sp2 }}>
          {steps.map((s, i) => (
            <li
              key={`${String(i)}:${s}`}
              style={{ padding: `${String(t.sp2)}px 0` }}
            >
              {s}
            </li>
          ))}
        </ul>
      </Section>
    </div>
  );
}

interface FootswitchesDetailProps {
  preset: DoctorPresetResult;
}

function FootswitchesDetail({ preset }: FootswitchesDetailProps) {
  const { t } = useTheme();
  const lines = footswitchLines(preset.levelingDamage);
  return (
    <div data-result-detail={`${String(preset.listIndex)}|leveling-damage`}>
      <Section>
        <span>{RESULTS_TEXT.footswitchIntro}</span>
        <ul
          style={{
            margin: `${String(t.sp4)}px 0 0`,
            paddingLeft: t.sp16 + t.sp2,
          }}
        >
          {lines.map((l, i) => (
            <li
              key={`${String(i)}:${l.name}`}
              style={{ padding: `${String(t.sp2)}px 0` }}
            >
              <span style={{ fontWeight: t.weightSemibold }}>{l.name}</span>
              {": "}
              {l.does.join(" ")}
            </li>
          ))}
        </ul>
      </Section>
    </div>
  );
}
