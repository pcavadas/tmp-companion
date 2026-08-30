// src/views/level/SummaryPage.tsx — the leveling wizard's "Summary" stage (design
// handoff 1a: full window, one row per preset). Replaces `overlays/SummaryBody.tsx`.
//
// Reports a run's outcome, grouped by PRESET (`PresetGroupRow`) — any preset that came
// back with a problem opens itself (`useGroupOpen`); the user can override any row on
// top of that. Sounds land in one of five classes, ordered by the action they need:
//   • offbranch    — the amp doesn't reach the USB 1/2 capture; only a ROUTING change
//     on the unit fixes it (a re-level can't).
//   • clamped      — headroom: already as loud as the preset allows.
//   • unconverged  — off target with knob room LEFT: the same target re-run improves
//     it (measurement ran out of tries, not of headroom).
//   • done         — hit target; may carry a "by ear" caveat.
//   • skipped      — couldn't be read/leveled.
// A stopped run can leave items un-leveled (no outcome).
//
// No revert of any kind lives here (design 1a, user directive): the backup-
// acknowledgment checkbox in Set up is the one revert path — restore from your Pro
// Control backup. The gain-budget redistribution / common-target / headroom-trade
// disclosure features are gone from the wizard entirely; a clamped row's message is
// always the one generic sentence below, regardless of clamp cause.

import { useMemo, type ReactNode } from "react";

import { useTheme, useStyles } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { Icon, type IconName } from "../../ui/Icon";
import { Tag } from "../../ui/Tag";
import { LevelPage } from "./LevelPage";
import { PresetGroupList, PresetGroupRow } from "./PresetGroupRow";
import { useGroupOpen } from "./useGroupOpen";
import { ByEarChip } from "../overlays/ByEarChip";
import { fmtLufs } from "../../lib/format";
import { groupItemsBySlot, offbranchStatus, type RunItem } from "./leveling";

const TRUE_PEAK_WARN_DBTP = -1;

const truePeakWarn = (it: RunItem): boolean =>
  it.isBase &&
  (it.outcome === "done" || it.outcome === "clamped") &&
  it.truePeakDbtp != null &&
  it.truePeakDbtp > TRUE_PEAK_WARN_DBTP;

const byEarOf = (it: RunItem): RunItem["verifyByEar"] =>
  it.outcome === "done" || it.outcome === "clamped"
    ? it.verifyByEar
    : undefined;

type ProblemKey =
  "clamped" | "unconverged" | "offbranch" | "skipped" | "notrun";

interface Problem {
  short: string;
  msg: string;
  fix?: string;
}

const PROBLEM: Record<ProblemKey, (it: RunItem) => Problem> = {
  clamped: () => ({
    short: "as loud as it goes",
    msg: "The knob is already all the way up. A quieter target would let this one match.",
    fix: "Level lower",
  }),
  unconverged: () => ({
    short: "off target",
    msg: "It had room left but ran out of tries. Running it again usually finishes the job.",
    fix: "Run again",
  }),
  offbranch: (it) => {
    const status = offbranchStatus(it.silenceHint);
    return {
      short: "can’t hear it",
      msg:
        status === "amp output at zero"
          ? "The amp’s output level is saved at 0. Raise it on the unit, then re-level."
          : status === "exp pedal may mute"
            ? "A pedal controls the amp’s output, with zero at one end. Park it at the other end, then re-level."
            : "Nothing came through USB 1/2, so we couldn’t hear this one.",
    };
  },
  skipped: () => ({
    short: "couldn’t read it",
    msg: "We couldn’t read this one from the unit.",
    fix: "Retry",
  }),
  notrun: () => ({
    short: "not run",
    msg: "Stopped before this one was reached.",
  }),
};

/** The problem key for a row's outcome, or null when it matched — a single mapping
 *  shared by the row-detail render and the color lookup, so the two can't drift. */
function problemKeyOf(it: RunItem): ProblemKey | null {
  if (it.outcome === "clamped") return "clamped";
  if (it.outcome === "unconverged") return "unconverged";
  if (it.outcome === "offbranch") return "offbranch";
  if (it.outcome === "skipped") return "skipped";
  if (it.outcome == null) return "notrun";
  return null;
}

function problemFor(
  it: RunItem,
  colorOf: (key: ProblemKey) => string,
): (Problem & { color: string }) | null {
  const key = problemKeyOf(it);
  if (!key) return null;
  return { ...PROBLEM[key](it), color: colorOf(key) };
}

export interface SummaryPageProps {
  items: RunItem[];
  stopped: boolean;
  onAccept: () => void;
  onRelevel: (subset: RunItem[]) => void;
}

export function SummaryPage({
  items,
  stopped,
  onAccept,
  onRelevel,
}: SummaryPageProps) {
  const { t } = useTheme();
  const s = useStyles();

  const colorOf = (key: keyof typeof PROBLEM): string =>
    key === "offbranch"
      ? t.warn
      : key === "clamped" || key === "unconverged"
        ? t.sevWarn
        : t.mutedInk;

  const offbr = items.filter((it) => it.outcome === "offbranch");
  const offbrAmpZero = offbr.filter((it) => it.silenceHint === "amp_zero");
  const offbrExpMute = offbr.filter((it) => it.silenceHint === "exp_mute");
  const offbrRouting = offbr.filter((it) => it.silenceHint == null);
  const clamped = items.filter((it) => it.outcome === "clamped");
  const unconverged = items.filter((it) => it.outcome === "unconverged");
  const leveled = items.filter((it) => it.outcome === "done");
  const skipped = items.filter((it) => it.outcome === "skipped");
  const notrun = items.filter((it) => it.outcome == null);
  const total = items.length;
  const allGood =
    offbr.length === 0 &&
    clamped.length === 0 &&
    unconverged.length === 0 &&
    skipped.length === 0 &&
    notrun.length === 0 &&
    !stopped;

  const byEarReasons: string[] = [];
  if (items.some((it) => byEarOf(it) === "envelope"))
    byEarReasons.push(
      "an envelope filter responds to the test signal differently than to real playing",
    );
  if (items.some((it) => byEarOf(it) === "dynamic"))
    byEarReasons.push("loud/quiet swings make the number an average");
  if (items.some((it) => byEarOf(it) === "wet_floor"))
    byEarReasons.push("floored at 25% of its designed mix");
  if (items.some((it) => byEarOf(it) === "rebalance"))
    byEarReasons.push("parallel amps balanced by approximate isolation");

  const groups = useMemo(() => groupItemsBySlot(items), [items]);
  const badSlots = useMemo(
    () => [
      ...new Set(
        items.filter((it) => it.outcome !== "done").map((it) => it.slot),
      ),
    ],
    [items],
  );
  const [isOpen, toggle] = useGroupOpen(badSlots);

  const title = stopped
    ? `Stopped — ${String(leveled.length)} sound${leveled.length === 1 ? "" : "s"} match`
    : allGood
      ? `All ${String(total)} sound${total === 1 ? "" : "s"} match now.`
      : `${String(leveled.length)} sound${leveled.length === 1 ? "" : "s"} match now. ${String(total - leveled.length)} need${total - leveled.length === 1 ? "s" : ""} you.`;
  const sub = allGood
    ? "Every sound sits at the same loudness, and each one is saved."
    : "Each one below says why and what to try.";

  const guidanceBanner = (
    icon: IconName,
    color: string,
    bg: string,
    border: string,
    heading: string,
    body: ReactNode,
  ) => (
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
        <Icon name={icon} size={15} stroke={color} strokeWidth={1.8} />
      </span>
      <div style={{ minWidth: 0 }}>
        <div style={{ ...s.kickerWide(color), marginBottom: t.space2 }}>
          {heading}
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
          {body}
        </div>
      </div>
    </div>
  );

  return (
    <LevelPage
      step={2}
      title={title}
      sub={sub}
      footerLeft={<span />}
      footerRight={
        <>
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
          {unconverged.length > 0 && (
            <Button
              variant="ghost"
              small
              icon="refresh"
              onClick={() => {
                onRelevel(unconverged);
              }}
              style={{ height: 32, padding: `0 ${String(t.space7)}px` }}
            >
              Re-run off target…
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
    >
      {(offbrAmpZero.length > 0 ||
        offbrExpMute.length > 0 ||
        offbrRouting.length > 0 ||
        clamped.length > 0) && (
        <div
          style={{
            flexShrink: 0,
            display: "flex",
            flexDirection: "column",
            gap: t.space4,
          }}
        >
          {offbrAmpZero.length > 0 &&
            guidanceBanner(
              "x",
              t.warn,
              t.warnSoft,
              t.warnBorder,
              "Amp output at zero",
              <>
                The amp’s output level is saved at{" "}
                <strong style={{ color: t.ink }}>0</strong>. Raise it on the
                unit, then re-level.
              </>,
            )}
          {offbrExpMute.length > 0 &&
            guidanceBanner(
              "x",
              t.warn,
              t.warnSoft,
              t.warnBorder,
              "Expression pedal may be muting",
              <>
                A pedal controls the amp’s output, with zero at one end. Park it
                at the <strong style={{ color: t.ink }}>other end</strong>, then
                re-level.
              </>,
            )}
          {offbrRouting.length > 0 &&
            guidanceBanner(
              "x",
              t.warn,
              t.warnSoft,
              t.warnBorder,
              "Needs routing on the unit",
              <>
                Route {offbrRouting.length === 1 ? "it" : "them"} to{" "}
                <strong style={{ color: t.ink }}>USB 1/2</strong> on the unit,
                or set the level by ear. Re-leveling won’t help.
              </>,
            )}
          {clamped.length > 0 &&
            guidanceBanner(
              "warn-tri",
              t.sevWarn,
              t.sevWarnSoft,
              t.sevWarnBorder,
              "Clamped — already maxed",
              <>
                The knob is already all the way up. A quieter target would let
                these match.
              </>,
            )}
        </div>
      )}

      <PresetGroupList
        label={`${String(groups.length)} preset${groups.length === 1 ? "" : "s"} · ${String(total)} sound${total === 1 ? "" : "s"}`}
      >
        {groups.map((g) => {
          const bad = g.items.filter((it) => it.outcome !== "done");
          const isBad = bad.length > 0;
          const p = isBad ? problemFor(bad[0], colorOf) : null;
          return (
            <PresetGroupRow
              key={g.slot}
              slot={g.slot}
              name={g.name}
              open={isOpen(g.slot)}
              onToggle={() => {
                toggle(g.slot);
              }}
              glyph={
                isBad ? (
                  <Icon
                    name="warn-tri"
                    size={14}
                    stroke={p?.color ?? t.sevWarn}
                    strokeWidth={1.7}
                  />
                ) : (
                  <Icon
                    name="check"
                    size={14}
                    stroke={t.good}
                    strokeWidth={2}
                  />
                )
              }
              tag={
                <span
                  style={{
                    fontFamily: t.mono,
                    fontSize: 9.5,
                    letterSpacing: "0.07em",
                    color: isBad ? (p?.color ?? t.sevWarn) : t.good,
                  }}
                >
                  {isBad
                    ? `${String(bad.length)} need${bad.length === 1 ? "s" : ""} you`
                    : "all matched"}
                </span>
              }
              note=""
              value={
                isBad
                  ? `${String(g.items.length - bad.length)} of ${String(g.items.length)}`
                  : "matched"
              }
              valueColor={isBad ? t.sevWarn : t.good}
            >
              {g.items.map((it) => {
                const q = problemFor(it, colorOf);
                return (
                  <div
                    key={it.key}
                    style={{
                      display: "flex",
                      flexDirection: "column",
                      gap: t.space2,
                      padding: `${String(t.space2)}px 0`,
                    }}
                  >
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: t.space6,
                        minHeight: 30,
                      }}
                    >
                      <span
                        style={{
                          width: 14,
                          flexShrink: 0,
                          display: "inline-flex",
                          justifyContent: "center",
                        }}
                      >
                        {q ? (
                          <Icon
                            name="warn-tri"
                            size={13}
                            stroke={q.color}
                            strokeWidth={1.7}
                          />
                        ) : (
                          <Icon
                            name="check"
                            size={13}
                            stroke={t.good}
                            strokeWidth={2}
                          />
                        )}
                      </span>
                      <span
                        style={{
                          fontFamily: t.sans,
                          fontSize: 13,
                          color: t.ink2,
                          width: 138,
                          flexShrink: 0,
                          whiteSpace: "nowrap",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        }}
                      >
                        {it.sceneName}
                      </span>
                      {byEarOf(it) && <ByEarChip />}
                      {truePeakWarn(it) && (
                        <span
                          title={`predicted ${fmtLufs(it.truePeakDbtp)} dBTP at the leveled setting`}
                        >
                          <Tag tone="warn">may clip</Tag>
                        </span>
                      )}
                      <span
                        style={{
                          width: 96,
                          textAlign: "right",
                          flexShrink: 0,
                          fontFamily: t.sans,
                          fontSize: 12,
                          color: q ? q.color : t.good,
                          marginLeft: "auto",
                        }}
                      >
                        {q ? q.short : "matched"}
                      </span>
                      <span
                        style={{
                          width: 56,
                          textAlign: "right",
                          flexShrink: 0,
                          fontFamily: t.mono,
                          fontSize: 11.5,
                          fontVariantNumeric: "tabular-nums",
                          color: q ? t.faint : t.good,
                        }}
                      >
                        {fmtLufs(it.value)}
                      </span>
                    </div>
                    {q && (
                      <div
                        style={{
                          display: "flex",
                          alignItems: "flex-start",
                          gap: t.space6,
                          padding: "0 0 4px 26px",
                        }}
                      >
                        <span
                          style={{
                            flex: 1,
                            minWidth: 0,
                            fontFamily: t.sans,
                            fontSize: 12.5,
                            lineHeight: 1.5,
                            color: q.color,
                            textWrap: "pretty",
                          }}
                        >
                          {q.msg}
                        </span>
                        {q.fix &&
                          (it.outcome === "clamped" ||
                            it.outcome === "unconverged" ||
                            it.outcome === "skipped") && (
                            <span
                              onClick={() => {
                                onRelevel([it]);
                              }}
                              style={{
                                flexShrink: 0,
                                fontFamily: t.sans,
                                fontSize: 12.5,
                                color: t.accentDeep,
                                cursor: "pointer",
                                whiteSpace: "nowrap",
                              }}
                            >
                              {q.fix}
                            </span>
                          )}
                      </div>
                    )}
                  </div>
                );
              })}
            </PresetGroupRow>
          );
        })}
      </PresetGroupList>

      {byEarReasons.length > 0 && (
        <div
          style={{
            flexShrink: 0,
            display: "flex",
            alignItems: "flex-start",
            gap: t.space4,
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
            Worth a quick listen — {byEarReasons.join("; ")}.
          </span>
        </div>
      )}
      {items.some(truePeakWarn) && (
        <div
          style={{
            flexShrink: 0,
            display: "flex",
            alignItems: "flex-start",
            gap: t.space4,
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
            Flagged rows are estimated to peak above −1 dBTP at the leveled
            setting. If your interface or FRFR clips, pick a lower target.
          </span>
        </div>
      )}
    </LevelPage>
  );
}

export default SummaryPage;
