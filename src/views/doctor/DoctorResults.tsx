// src/views/doctor/DoctorResults.tsx — Doctor's Results page (look B).
//
// Top to bottom: the title ("3 sounds need a look", or "Nothing to change in these 3
// sounds") with Doctor's Steps; "Worst first. Make the change on your Tone Master Pro, then
// check again." when something needs a look; ONE flat Group of rows across every checked
// preset (`resultsModel.ts` sorts them: findings worst first, then sounds that couldn't be
// checked, then sounds a stopped check never reached, then the clean ones — folded into one
// "4 more sound good" row when there are findings above them); and the Footer: "Check
// other sounds" and "Check again", or only "Check other sounds" when everything was checked
// and nothing needs a look.
//
// The worst row is open on arrival. On a 900 px window an opened row shows its detail
// under itself (several may be open); on a wide window the list sits in a 400 px column
// and the opened row's detail fills a panel beside it, one at a time. Doctor only
// describes fixes: nothing here writes to the unit, so nothing is ever disabled while a
// command runs.

import { useMemo, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Footer } from "../../ui/Footer";
import { Group, Row } from "../../ui/List";
import { Icon } from "../../ui/Icon";
import { Button } from "../../ui/primitives";
import { plainButton } from "../../theme/tokens";
import { useWindowWidth } from "../../lib/useWindowWidth";
import type { DoctorCheckResult, DoctorInputArg } from "../../lib/types";
import { FlowPage } from "../level/FlowPage";
import { StatusMark } from "../level/StatusMark";
import { ResultRow } from "./ResultRow";
import { ResultDetail, type DoctorReference } from "./ResultDetail";
import {
  buildResults,
  moreGoodLabel,
  ownNodeIdsFor,
  type ResultRow as ResultRowModel,
  type ResultsContext,
} from "./resultsModel";
import { WIDE_WINDOW } from "./layout";

/** The list's column on a wide window (px). */
const LIST_COLUMN_W = 400;
/** The content's widest on a wide window, list and panel together (px). */
const WIDE_MAX_W = 1200;

export interface DoctorResultsProps {
  result: DoctorCheckResult;
  /** Names, the presets' silence hints. */
  ctx: ResultsContext;
  /** Sound key → the exact `doctor_check` input it was checked with (its chain for "To
   *  sound like …", its full footswitch roster for the shared-block line). */
  inputByKey: ReadonlyMap<string, DoctorInputArg>;
  /** Doctor's steps for this check (the Instrument page may have been skipped). */
  steps: readonly string[];
  /** Back to the list (ticks kept). */
  onCheckMore: () => void;
  /** The same sounds again, with the same instruments. */
  onCheckAgain: () => void;
}

export function DoctorResults({
  result,
  ctx,
  inputByKey,
  steps,
  onCheckMore,
  onCheckAgain,
}: DoctorResultsProps) {
  const { t } = useTheme();
  const model = useMemo(() => buildResults(result, ctx), [result, ctx]);
  const wide = useWindowWidth() >= WIDE_WINDOW && model.findings.length > 0;

  // Narrow: the rows open under themselves. Wide: the one row shown beside the list.
  const [open, setOpen] = useState<ReadonlySet<string>>(
    () => new Set(model.firstOpen != null ? [model.firstOpen] : []),
  );
  const [picked, setPicked] = useState<string | null>(model.firstOpen);
  const [showClean, setShowClean] = useState(false);
  const [referenceId, setReferenceId] = useState<string | null>(null);

  const allRows = useMemo(
    () => [...model.findings, ...model.failed, ...model.unrun, ...model.clean],
    [model],
  );
  const referenceRow = allRows.find((r) => r.id === referenceId) ?? null;
  const reference: DoctorReference | null =
    referenceRow?.sound != null
      ? {
          id: referenceRow.id,
          sound: referenceRow.sound,
          name: `${referenceRow.presetName}, ${referenceRow.soundName}`,
        }
      : null;

  const toggle = (id: string) => {
    if (wide) {
      setPicked(id);
      return;
    }
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const detailOf = (row: ResultRowModel) => {
    const input = row.sound ? inputByKey.get(row.sound.key) : undefined;
    return (
      <ResultDetail
        row={row}
        ctx={ctx}
        ownNodeIds={
          row.sound ? ownNodeIdsFor(row.sound, input?.footswitches) : undefined
        }
        nodes={input?.nodes ?? []}
        reference={reference}
        onSetReference={setReferenceId}
        onClearReference={() => {
          setReferenceId(null);
        }}
      />
    );
  };

  const rowWithDetail = (row: ResultRowModel) => {
    const isOpen = wide ? picked === row.id : open.has(row.id);
    return (
      <div key={row.id}>
        <ResultRow row={row} open={isOpen} picks={wide} onToggle={toggle} />
        {!wide && isOpen && row.expandable && (
          <div
            style={{
              background: t.rowSelected,
              padding: `0 ${String(t.sp16)}px ${String(t.sp6)}px 44px`,
            }}
          >
            {detailOf(row)}
          </div>
        )}
      </div>
    );
  };

  const list = (
    <Group label="Results">
      {model.findings.map(rowWithDetail)}
      {model.failed.map(rowWithDetail)}
      {model.unrun.map(rowWithDetail)}
      {model.foldClean ? (
        <>
          <Row style={{ padding: 0 }}>
            <button
              type="button"
              aria-expanded={showClean}
              onClick={() => {
                setShowClean((v) => !v);
              }}
              style={plainButton({
                flex: 1,
                height: "100%",
                display: "flex",
                alignItems: "center",
                gap: t.sp8 + 2,
                padding: `0 ${String(t.sp12)}px`,
                textAlign: "left",
                fontFamily: t.fontSans,
                fontSize: t.fontBody,
                color: t.text,
              })}
            >
              <StatusMark kind="done" />
              <span style={{ flex: 1 }}>
                {moreGoodLabel(model.clean.length)}
              </span>
              <span
                aria-hidden
                style={{ width: 18, display: "flex", justifyContent: "center" }}
              >
                <Icon
                  name={showClean ? "chev-down" : "chev-right"}
                  size={12}
                  stroke={t.textSecondary}
                  strokeWidth={2.2}
                />
              </span>
            </button>
          </Row>
          {showClean && model.clean.map(rowWithDetail)}
        </>
      ) : (
        model.clean.map(rowWithDetail)
      )}
    </Group>
  );

  const pickedRow = wide
    ? (allRows.find((r) => r.id === picked) ?? null)
    : null;

  const subtitle = model.subtitle != null && (
    <p
      style={{
        margin: 0,
        flexShrink: 0,
        fontSize: t.fontBody,
        lineHeight: `${String(t.lineBody)}px`,
        color: t.textSecondary,
      }}
    >
      {model.subtitle}
    </p>
  );

  const footer = model.allClear ? (
    <Footer
      right={
        <Button variant="primary" size="lg" onClick={onCheckMore}>
          Check other sounds
        </Button>
      }
    />
  ) : (
    <Footer
      left={
        <Button variant="secondary" size="lg" onClick={onCheckMore}>
          Check other sounds
        </Button>
      }
      right={
        <Button variant="primary" size="lg" onClick={onCheckAgain}>
          Check again
        </Button>
      }
    />
  );

  if (!wide) {
    return (
      <FlowPage
        title={model.title}
        step={steps.length - 1}
        steps={steps}
        gap={t.sp12}
        footer={footer}
      >
        {subtitle}
        {list}
      </FlowPage>
    );
  }

  return (
    <FlowPage
      title={model.title}
      step={steps.length - 1}
      steps={steps}
      gap={t.sp12}
      maxWidth={WIDE_MAX_W}
      fill
      footer={footer}
    >
      {subtitle}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          display: "flex",
          alignItems: "flex-start",
          gap: t.sp16,
        }}
      >
        <div
          style={{
            width: LIST_COLUMN_W,
            flexShrink: 0,
            maxHeight: "100%",
            overflowY: "auto",
          }}
        >
          {list}
        </div>
        {pickedRow && (
          <div
            style={{
              flex: 1,
              minWidth: 0,
              maxHeight: "100%",
              overflowY: "auto",
              boxSizing: "border-box",
              background: t.surface,
              border: `1px solid ${t.border}`,
              borderRadius: t.radiusGroup,
              padding: `${String(t.sp12)}px ${String(t.sp16)}px ${String(t.sp6)}px`,
            }}
          >
            <h2
              style={{
                margin: `0 0 ${String(t.sp8)}px`,
                fontSize: t.fontLarge,
                lineHeight: `${String(t.lineLarge)}px`,
                fontWeight: t.weightSemibold,
              }}
            >
              {pickedRow.presetName}{" "}
              <span
                style={{
                  color: t.textSecondary,
                  fontWeight: t.weightRegular,
                }}
              >
                {pickedRow.soundName}
              </span>
            </h2>
            {detailOf(pickedRow)}
          </div>
        )}
      </div>
    </FlowPage>
  );
}
