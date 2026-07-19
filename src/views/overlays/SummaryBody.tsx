// src/views/overlays/SummaryBody.tsx — wizard step 3, "Summary".
//
// Reports a run's outcome and routes the user to the RIGHT next action. Sounds land in
// one of four classes, ordered by the action they need:
//   • offbranch — the amp doesn't reach the USB 1/2 capture; only a ROUTING change on
//     the unit fixes it (a re-level can't). Marked with the `x` icon in WARN color
//     (same icon as skipped, distinguished by color) and never offered to re-level.
//   • clamped   — headroom: already as loud as the preset allows; lower the target + re-level.
//   • done      — hit target; may carry a "by ear" caveat (dynamic peaks or rebalanced mix).
//   • skipped   — couldn't be read/levelled.
// A stopped run can leave items un-leveled (no outcome) → the "Not leveled" group.
// Guidance lives in always-visible callouts (click-only app — no hover tooltip).

import { Fragment, useState } from "react";

import { useTheme, useStyles } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { Icon, type IconName } from "../../ui/Icon";
import { Tag } from "../../ui/Tag";
import { WizardFooter, WizTitle } from "./WizardShell";
import { ByEarChip } from "./ByEarChip";
import { fmtLufs } from "../../lib/format";
import { restorePresetLevel } from "../../lib/invoke";
import { ceilingOf, offbranchStatus, type RunItem } from "../level/leveling";

/** True-peak clip threshold (dBTP): a Base row's PREDICTED true peak above this
 *  earns the "may clip" caveat (estimate, from `leveller::predicted_true_peak_dbtp`,
 *  never a re-measurement). */
const TRUE_PEAK_WARN_DBTP = -1;

/** Base rows only (the only path that estimates true peak); done/clamped only. */
const truePeakWarn = (it: RunItem): boolean =>
  it.isBase &&
  (it.outcome === "done" || it.outcome === "clamped") &&
  it.truePeakDbtp != null &&
  it.truePeakDbtp > TRUE_PEAK_WARN_DBTP;

/** Per-row restore progress, keyed by RunItem.key (local — the run data itself
 *  never mutates; a restore is a follow-up device write, not a run result). */
type RestoreState = "busy" | "done" | "failed";

/** A row can offer "Restore original" when the run WROTE the preset (done or
 *  clamped both save) and the pre-run `presetLevel` was captured. Base rows only —
 *  scene/footswitch rows write amp `outputLevel`, which has no revert yet. */
const restorable = (it: RunItem): boolean =>
  it.isBase &&
  it.previousLevel != null &&
  (it.outcome === "done" || it.outcome === "clamped");

const AMBER_SOFT = "rgba(176,125,28,0.10)";

/** The by-ear cause to show for a row — only leveled/clamped rows carry the marker. */
const byEarOf = (it: RunItem): RunItem["verifyByEar"] =>
  it.outcome === "done" || it.outcome === "clamped"
    ? it.verifyByEar
    : undefined;

/** A guidance callout above the list — uppercase kicker + plain-language next action. */
function Banner({
  icon,
  color,
  bg,
  border,
  strokeWidth = 1.7,
  size = 15,
  title,
  children,
}: {
  icon: IconName;
  color: string;
  bg: string;
  border: string;
  strokeWidth?: number;
  size?: number;
  title: string;
  children: React.ReactNode;
}) {
  const { t } = useTheme();
  const s = useStyles();
  return (
    <div
      style={{
        display: "flex",
        gap: t.space5,
        padding: `${String(t.space5)}px ${String(t.space6)}px`,
        borderRadius: 9,
        background: bg,
        border: `0.5px solid ${border}`,
      }}
    >
      <span style={{ flexShrink: 0, paddingTop: t.space1 }}>
        <Icon
          name={icon}
          size={size}
          stroke={color}
          strokeWidth={strokeWidth}
        />
      </span>
      <div style={{ minWidth: 0 }}>
        <div style={{ ...s.kickerWide(color), marginBottom: t.space2 }}>
          {title}
        </div>
        <div
          style={{
            fontFamily: t.sans,
            fontSize: 12.5,
            lineHeight: 1.5,
            color: t.ink2,
            textWrap: "pretty",
          }}
        >
          {children}
        </div>
      </div>
    </div>
  );
}

/** Lightweight group header in the result list. */
function SectionLabel({
  children,
  n,
  color,
}: {
  children: React.ReactNode;
  n: number;
  color: string;
}) {
  const { t } = useTheme();
  const s = useStyles();
  return (
    <div
      style={{
        display: "flex",
        alignItems: "baseline",
        gap: t.space4,
        padding: `${String(t.space6)}px ${String(t.space5)}px ${String(t.space3)}px`,
      }}
    >
      <span style={s.kickerWide(color)}>{children}</span>
      <span style={{ fontFamily: t.mono, fontSize: 9.5, color: t.faint }}>
        {n}
      </span>
    </div>
  );
}

interface ResultRowProps {
  it: RunItem;
  /** Present when the row can offer "Restore original". */
  restore?: { state?: RestoreState; busyAny: boolean; onClick: () => void };
}

/** One result row. Icon SHAPE + group + status word (not color alone) separate the states. */
function ResultRow({ it, restore }: ResultRowProps) {
  const { t } = useTheme();
  const dim = it.outcome === "skipped" || it.outcome == null;
  let icon: React.ReactNode;
  let statusColor: string;
  let status: string;
  if (it.outcome === "offbranch") {
    icon = <Icon name="x" size={13} stroke={t.warn} strokeWidth={2} />;
    statusColor = t.warn;
    status = offbranchStatus(it.silenceHint);
  } else if (it.outcome === "clamped") {
    icon = (
      <Icon name="warn-tri" size={13} stroke={t.sevWarn} strokeWidth={1.7} />
    );
    statusColor = t.sevWarn;
    status = `clamped · ${fmtLufs(it.value)}`;
  } else if (it.outcome === "done") {
    icon = <Icon name="check" size={14} stroke={t.good} strokeWidth={2} />;
    statusColor = t.good;
    status = `${fmtLufs(it.value)} LUFS`;
  } else if (it.outcome === "skipped") {
    icon = <Icon name="x" size={12} stroke={t.mutedInk} strokeWidth={2} />;
    statusColor = t.mutedInk;
    status = "read failed";
  } else {
    icon = <Icon name="x" size={12} stroke={t.mutedInk} strokeWidth={2} />;
    statusColor = t.mutedInk;
    status = "not run";
  }
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: t.space6,
        padding: `${String(t.space4)}px ${String(t.space5)}px`,
      }}
    >
      <span
        style={{
          width: 16,
          flexShrink: 0,
          display: "inline-flex",
          justifyContent: "center",
        }}
      >
        {icon}
      </span>
      <span
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          alignItems: "baseline",
          gap: t.space4,
        }}
      >
        <span
          style={{
            fontFamily: t.serif,
            fontSize: 14,
            color: dim ? t.mutedInk : t.ink,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {it.label}
        </span>
        {it.tag && (
          <span
            style={{
              fontFamily: t.mono,
              fontSize: 8.5,
              color: it.isBase ? t.faint : t.accentDeep,
              flexShrink: 0,
            }}
          >
            {it.tag}
          </span>
        )}
        {byEarOf(it) && <ByEarChip />}
        {truePeakWarn(it) && (
          <span
            title={`predicted ${fmtLufs(it.truePeakDbtp)} dBTP at the leveled setting`}
          >
            <Tag tone="warn">may clip</Tag>
          </span>
        )}
      </span>
      <span
        style={{
          fontFamily: t.mono,
          fontSize: 11,
          flexShrink: 0,
          color: statusColor,
          width: 104,
          textAlign: "right",
        }}
      >
        {status}
      </span>
      {restore &&
        (restore.state === "done" ? (
          <span
            style={{
              fontFamily: t.mono,
              fontSize: 10,
              color: t.good,
              flexShrink: 0,
            }}
          >
            restored
          </span>
        ) : (
          <Button
            variant="ghost"
            small
            disabled={restore.busyAny}
            onClick={restore.onClick}
            style={{
              height: 24,
              padding: `0 ${String(t.space4)}px`,
              flexShrink: 0,
            }}
          >
            {restore.state === "busy"
              ? "Restoring…"
              : restore.state === "failed"
                ? "Retry restore"
                : "Restore"}
          </Button>
        ))}
    </div>
  );
}

/** Gain-budget redistribution actions (loud-preset clamp class, single-amp v1), threaded
 *  from `useLevelingFlow`. `plan` returns what a redistribution would rewrite (or null when
 *  it doesn't apply); `run` applies it; `undo` reverts every one this Summary applied. */
export interface RedistributionActions {
  plan: (items: RunItem[]) => { presets: number; scenes: number } | null;
  run: (items: RunItem[]) => void;
  undoCount: number;
  undo: () => void;
}

/** Reachable-common-target actions (quiet-preset clamp fallback), threaded from
 *  `useLevelingFlow`. `plan` is true when re-leveling to a reachable common target would help
 *  (a clamp exists with a measured ceiling to derive from); `run` applies it. */
export interface CommonTargetActions {
  plan: (items: RunItem[]) => boolean;
  run: (items: RunItem[]) => void;
}

export interface SummaryBodyProps {
  items: RunItem[];
  stopped: boolean;
  onAccept: () => void;
  onRelevel: (clamped: RunItem[]) => void;
  redistribution?: RedistributionActions;
  commonTarget?: CommonTargetActions;
}

export function SummaryBody({
  items,
  stopped,
  onAccept,
  onRelevel,
  redistribution,
  commonTarget,
}: SummaryBodyProps) {
  const { t } = useTheme();
  // "Restore original" progress per row. Restores run one at a time (every
  // device write is serialized backend-side anyway; the UI just mirrors that).
  const [restoreState, setRestoreState] = useState<
    Record<string, RestoreState>
  >({});
  const busyAny = Object.values(restoreState).includes("busy");
  const runRestore = (it: RunItem) => {
    if (it.previousLevel == null) return;
    const level = it.previousLevel;
    setRestoreState((s) => ({ ...s, [it.key]: "busy" }));
    restorePresetLevel(it.slot, level, it.presetName)
      .then(() => {
        setRestoreState((s) => ({ ...s, [it.key]: "done" }));
      })
      .catch((e: unknown) => {
        console.warn("restore_preset_level failed", e);
        setRestoreState((s) => ({ ...s, [it.key]: "failed" }));
      });
  };
  const restoreFor = (it: RunItem) =>
    restorable(it)
      ? {
          state: restoreState[it.key],
          busyAny,
          onClick: () => {
            runRestore(it);
          },
        }
      : undefined;
  const anyRestorable = items.some(restorable);
  const offbr = items.filter((it) => it.outcome === "offbranch");
  // Cause split (backup-scan silence hint): each class gets its own action banner —
  // "route it" advice on a zeroed amp / parked exp pedal would be wrong.
  const offbrAmpZero = offbr.filter((it) => it.silenceHint === "amp_zero");
  const offbrExpMute = offbr.filter((it) => it.silenceHint === "exp_mute");
  const offbrRouting = offbr.filter((it) => it.silenceHint == null);
  const clamped = items.filter((it) => it.outcome === "clamped"); // HEADROOM clamps only
  // What a gain-budget redistribution would rewrite (loud-preset class, single-amp) — null
  // when it doesn't apply (multi-amp, no headroom, or nothing clamped).
  const redistPlan = redistribution?.plan(items) ?? null;
  // Whether the reachable-common-target fallback (quiet-preset class) can help these clamps.
  const commonPlan = commonTarget?.plan(items) ?? false;
  // The lowest measured ceiling among the clamped sounds — named verbatim in the clamp banner
  // so the user sees WHY the target was unreachable (a clamped sound sits at max, value = C).
  const clampedCeilings = clamped.flatMap((it) => {
    const c = ceilingOf(it);
    return c != null ? [c] : [];
  });
  const clampedCeiling =
    clampedCeilings.length > 0 ? Math.min(...clampedCeilings) : null;
  const leveled = items.filter((it) => it.outcome === "done");
  const skipped = items.filter((it) => it.outcome === "skipped");
  const notrun = items.filter((it) => it.outcome == null); // only on a stopped run
  const total = items.length;
  const allGood =
    offbr.length === 0 &&
    clamped.length === 0 &&
    skipped.length === 0 &&
    notrun.length === 0 &&
    !stopped;
  // The footnote is reason-aware: a row earns the "by ear" chip for one of three causes,
  // which prompt DIFFERENT listening — keep one chip per row, but spell out only the
  // causes actually present, joined by "; ". Envelope first (it questions the
  // measurement itself, matching its precedence over the result-derived causes).
  const byEarReasons: string[] = [];
  if (items.some((it) => byEarOf(it) === "envelope"))
    byEarReasons.push(
      "an envelope filter responds to the test signal differently than to real playing",
    );
  if (items.some((it) => byEarOf(it) === "dynamic"))
    byEarReasons.push("loud/quiet swings make the number an average");
  if (items.some((it) => byEarOf(it) === "rebalance"))
    byEarReasons.push("parallel amps balanced by approximate isolation");

  const title = allGood
    ? `All ${String(total)} sound${total === 1 ? "" : "s"} leveled`
    : `${String(leveled.length)} of ${String(total)} leveled`;

  // Action-first sub-tally — only the classes that need a next step.
  const bits: string[] = [];
  if (stopped) bits.push("stopped");
  if (offbr.length) bits.push(`${String(offbr.length)} silent`);
  if (clamped.length) bits.push(`${String(clamped.length)} clamped`);
  if (skipped.length) bits.push(`${String(skipped.length)} skipped`);

  // Result groups, ordered by the action they need (routing first, leveled/skipped last).
  const groups: { label: string; color: string; rows: RunItem[] }[] = [
    { label: "No signal", color: t.warn, rows: offbr },
    { label: "Clamped", color: t.sevWarn, rows: clamped },
    { label: "Leveled", color: t.good, rows: leveled },
    { label: "Skipped", color: t.faint, rows: skipped },
    { label: "Not leveled", color: t.faint, rows: notrun },
  ];

  return (
    <>
      <div
        style={{
          flexShrink: 0,
          padding: `${String(t.space8)}px ${String(t.space10)}px ${String(t.space7)}px`,
          borderBottom: `0.5px solid ${t.hairline}`,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: t.space5 }}>
          <span
            style={{
              width: 30,
              height: 30,
              borderRadius: 8,
              flexShrink: 0,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              background: allGood ? t.goodSoft : AMBER_SOFT,
              border: `0.5px solid ${allGood ? t.goodBorder : "rgba(176,125,28,0.45)"}`,
            }}
          >
            <Icon
              name={allGood ? "check" : "warn-tri"}
              size={16}
              stroke={allGood ? t.good : t.sevWarn}
              strokeWidth={allGood ? 2 : 1.6}
            />
          </span>
          <div>
            <WizTitle size={21} style={{ textWrap: "balance" }}>
              {title}
            </WizTitle>
            {bits.length > 0 && (
              <div
                style={{
                  fontFamily: t.mono,
                  fontSize: 11,
                  color: t.mutedInk,
                  marginTop: t.space2,
                  letterSpacing: "0.02em",
                }}
              >
                {bits.join("  ·  ")}
              </div>
            )}
          </div>
        </div>
      </div>

      {/* action guidance — short, one banner per action-needed class, routing first */}
      {(offbr.length > 0 || clamped.length > 0) && (
        <div
          style={{
            flexShrink: 0,
            padding: `${String(t.space7)}px ${String(t.space9)}px ${String(t.space1)}px`,
            display: "flex",
            flexDirection: "column",
            gap: t.space4,
          }}
        >
          {(
            [
              {
                title: "Amp output at zero",
                rows: offbrAmpZero,
                body: (
                  <>
                    The amp’s output level is saved at{" "}
                    <strong style={{ color: t.ink }}>0</strong> — raise it on
                    the unit, then re-level.
                  </>
                ),
              },
              {
                title: "Expression pedal may be muting",
                rows: offbrExpMute,
                body: (
                  <>
                    A pedal controls the amp’s output with zero at one end —
                    park it at the{" "}
                    <strong style={{ color: t.ink }}>other end</strong>, then
                    re-level.
                  </>
                ),
              },
              {
                title: "Needs routing on the unit",
                rows: offbrRouting,
                body: (
                  <>
                    Route {offbrRouting.length === 1 ? "it" : "them"} to{" "}
                    <strong style={{ color: t.ink }}>USB&nbsp;1/2</strong> on
                    the unit, or set the level by ear. Re-leveling won’t help.
                  </>
                ),
              },
            ] as const
          )
            .filter((b) => b.rows.length > 0)
            .map((b) => (
              <Banner
                key={b.title}
                icon="x"
                size={15}
                strokeWidth={2}
                color={t.warn}
                bg={t.warnSoft}
                border="rgba(167,70,31,0.28)"
                title={b.title}
              >
                {b.body}
              </Banner>
            ))}
          {clamped.length > 0 && (
            <Banner
              icon="warn-tri"
              color={t.sevWarn}
              bg={AMBER_SOFT}
              border="rgba(176,125,28,0.3)"
              title="Clamped — already maxed"
            >
              Already as loud as the preset allows
              {clampedCeiling != null
                ? ` — ceiling ${fmtLufs(clampedCeiling)} LUFS`
                : ""}
              .{" "}
              {redistPlan ? (
                <>
                  But the preset has headroom to spare.{" "}
                  <strong style={{ color: t.ink }}>
                    Give clamped scenes headroom
                  </strong>{" "}
                  rewrites the preset level, the base amp, and{" "}
                  {redistPlan.scenes} scene
                  {redistPlan.scenes === 1 ? "" : "s"}
                  {redistPlan.presets > 1
                    ? ` across ${String(redistPlan.presets)} presets`
                    : ""}{" "}
                  so they reach target — non-clamped sounds stay put. Or lower
                  the target and re-level.
                </>
              ) : commonTarget && commonPlan ? (
                <>
                  <strong style={{ color: t.ink }}>
                    Re-level everything to a reachable common target
                  </strong>{" "}
                  brings every sound to one loudness the quietest can reach — no
                  on-stage jump. Or lower the target and re-level.
                </>
              ) : (
                <>Lower the target and re-level, or keep as-is.</>
              )}
            </Banner>
          )}
        </div>
      )}

      {/* grouped, action-ordered list (each group omitted when empty) */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          padding: `${String(t.space2)}px ${String(t.space7)}px ${String(t.space2)}px`,
          overflowY: "auto",
        }}
      >
        {groups.map(({ label, color, rows }) =>
          rows.length > 0 ? (
            <Fragment key={label}>
              <SectionLabel n={rows.length} color={color}>
                {label}
              </SectionLabel>
              {rows.map((it) => (
                <ResultRow key={it.key} it={it} restore={restoreFor(it)} />
              ))}
            </Fragment>
          ) : null,
        )}
        {anyRestorable && (
          <div
            style={{
              fontFamily: t.sans,
              fontSize: 11.5,
              lineHeight: 1.5,
              color: t.mutedInk,
              padding: `${String(t.space5)}px ${String(t.space5)}px 0`,
            }}
          >
            Restore rewrites a preset’s previous saved level — scene and
            footswitch changes stay.
          </div>
        )}
        {byEarReasons.length > 0 && (
          <div
            style={{
              display: "flex",
              alignItems: "flex-start",
              gap: t.space4,
              padding: `${String(t.space5)}px ${String(t.space5)}px ${String(t.space2)}px`,
            }}
          >
            <span style={{ paddingTop: t.space1 }}>
              <ByEarChip />
            </span>
            <span
              style={{
                fontFamily: t.sans,
                fontSize: 11.5,
                lineHeight: 1.5,
                color: t.mutedInk,
              }}
            >
              worth a quick listen — {byEarReasons.join("; ")}.
            </span>
          </div>
        )}
        {items.some(truePeakWarn) && (
          <div
            style={{
              display: "flex",
              alignItems: "flex-start",
              gap: t.space4,
              padding: `${String(t.space5)}px ${String(t.space5)}px ${String(t.space2)}px`,
            }}
          >
            <span style={{ paddingTop: t.space1 }}>
              <Tag tone="warn">may clip</Tag>
            </span>
            <span
              style={{
                fontFamily: t.sans,
                fontSize: 11.5,
                lineHeight: 1.5,
                color: t.mutedInk,
              }}
            >
              flagged rows are estimated to peak above −1 dBTP at the leveled
              setting — if your interface or FRFR clips, pick a lower target.
            </span>
          </div>
        )}
      </div>

      <WizardFooter
        left={
          redistribution && redistribution.undoCount > 0 ? (
            <Button
              variant="ghost"
              small
              icon="refresh"
              onClick={redistribution.undo}
              style={{ height: 32, padding: `0 ${String(t.space5)}px` }}
            >
              Undo redistribution
            </Button>
          ) : (
            <span />
          )
        }
        right={
          <>
            {redistPlan && redistribution && (
              <Button
                variant="ghost"
                small
                onClick={() => {
                  redistribution.run(items);
                }}
                style={{ height: 32, padding: `0 ${String(t.space6)}px` }}
              >
                Give clamped scenes headroom
              </Button>
            )}
            {commonPlan && commonTarget && (
              <Button
                variant="ghost"
                small
                onClick={() => {
                  commonTarget.run(items);
                }}
                style={{ height: 32, padding: `0 ${String(t.space6)}px` }}
              >
                Re-level to a reachable target
              </Button>
            )}
            {clamped.length > 0 && (
              <Button
                variant="ghost"
                small
                icon="refresh"
                onClick={() => {
                  onRelevel(clamped);
                }}
                style={{ height: 32, padding: `0 ${String(t.space7)}px` }}
              >
                Re-level clamped…
              </Button>
            )}
            <Button
              variant="primary"
              small
              onClick={onAccept}
              style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
            >
              {allGood ? "Done" : "Accept"}
            </Button>
          </>
        }
      />
    </>
  );
}

export default SummaryBody;
