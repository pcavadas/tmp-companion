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

import { Fragment } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { Icon, type IconName } from "../../ui/Icon";
import { WizardFooter, WizTitle } from "./WizardShell";
import { ByEarChip } from "./ByEarChip";
import { fmtLufs } from "../../lib/format";
import type { RunItem } from "../level/leveling";

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
  return (
    <div
      style={{
        display: "flex",
        gap: 11,
        padding: "10px 13px",
        borderRadius: 9,
        background: bg,
        border: `0.5px solid ${border}`,
      }}
    >
      <span style={{ flexShrink: 0, paddingTop: 1 }}>
        <Icon
          name={icon}
          size={size}
          stroke={color}
          strokeWidth={strokeWidth}
        />
      </span>
      <div style={{ minWidth: 0 }}>
        <div
          style={{
            fontFamily: t.mono,
            fontSize: 9.5,
            letterSpacing: "0.12em",
            textTransform: "uppercase",
            color,
            marginBottom: 3,
          }}
        >
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
  return (
    <div
      style={{
        display: "flex",
        alignItems: "baseline",
        gap: 8,
        padding: "12px 10px 5px",
      }}
    >
      <span
        style={{
          fontFamily: t.mono,
          fontSize: 9.5,
          letterSpacing: "0.12em",
          textTransform: "uppercase",
          color,
        }}
      >
        {children}
      </span>
      <span style={{ fontFamily: t.mono, fontSize: 9.5, color: t.faint }}>
        {n}
      </span>
    </div>
  );
}

/** One result row. Icon SHAPE + group + status word (not color alone) separate the states. */
function ResultRow({ it }: { it: RunItem }) {
  const { t } = useTheme();
  const dim = it.outcome === "skipped" || it.outcome == null;
  let icon: React.ReactNode;
  let statusColor: string;
  let status: string;
  if (it.outcome === "offbranch") {
    icon = <Icon name="x" size={13} stroke={t.warn} strokeWidth={2} />;
    statusColor = t.warn;
    status = "not on USB 1/2";
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
        gap: 12,
        padding: "7px 10px",
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
          gap: 8,
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
    </div>
  );
}

export interface SummaryBodyProps {
  items: RunItem[];
  stopped: boolean;
  onAccept: () => void;
  onRelevel: (clamped: RunItem[]) => void;
}

export function SummaryBody({
  items,
  stopped,
  onAccept,
  onRelevel,
}: SummaryBodyProps) {
  const { t } = useTheme();
  const offbr = items.filter((it) => it.outcome === "offbranch");
  const clamped = items.filter((it) => it.outcome === "clamped"); // HEADROOM clamps only
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
  if (offbr.length) bits.push(`${String(offbr.length)} need routing`);
  if (clamped.length) bits.push(`${String(clamped.length)} clamped`);
  if (skipped.length) bits.push(`${String(skipped.length)} skipped`);

  // Result groups, ordered by the action they need (routing first, leveled/skipped last).
  const groups: { label: string; color: string; rows: RunItem[] }[] = [
    { label: "Needs routing", color: t.warn, rows: offbr },
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
          padding: "16px 24px 14px",
          borderBottom: `0.5px solid ${t.hairline}`,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 11 }}>
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
                  marginTop: 3,
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
            padding: "14px 20px 2px",
            display: "flex",
            flexDirection: "column",
            gap: 9,
          }}
        >
          {offbr.length > 0 && (
            <Banner
              icon="x"
              size={15}
              strokeWidth={2}
              color={t.warn}
              bg={t.warnSoft}
              border="rgba(167,70,31,0.28)"
              title="Needs routing on the unit"
            >
              Route {offbr.length === 1 ? "it" : "them"} to{" "}
              <strong style={{ color: t.ink }}>USB&nbsp;1/2</strong> on the
              unit, or set the level by ear. Re-leveling won’t help.
            </Banner>
          )}
          {clamped.length > 0 && (
            <Banner
              icon="warn-tri"
              color={t.sevWarn}
              bg={AMBER_SOFT}
              border="rgba(176,125,28,0.3)"
              title="Clamped — already maxed"
            >
              Already as loud as the preset allows. Lower the target and
              re-level, or keep as-is.
            </Banner>
          )}
        </div>
      )}

      {/* grouped, action-ordered list (each group omitted when empty) */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          padding: "4px 14px 4px",
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
                <ResultRow key={it.key} it={it} />
              ))}
            </Fragment>
          ) : null,
        )}
        {byEarReasons.length > 0 && (
          <div
            style={{
              display: "flex",
              alignItems: "flex-start",
              gap: 8,
              padding: "10px 10px 4px",
            }}
          >
            <span style={{ paddingTop: 1 }}>
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
      </div>

      <WizardFooter
        left={<span />}
        right={
          <>
            {clamped.length > 0 && (
              <Button
                variant="ghost"
                small
                icon="refresh"
                onClick={() => {
                  onRelevel(clamped);
                }}
                style={{ height: 32, padding: "0 14px" }}
              >
                Re-level clamped…
              </Button>
            )}
            <Button
              variant="primary"
              small
              onClick={onAccept}
              style={{ height: 32, padding: "0 18px" }}
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
