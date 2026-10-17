// src/views/level/SummaryPage.tsx — the leveling Summary, a page under the toolbar (look B).
//
// Top to bottom: the title says the outcome ("4 sounds leveled. 4 need you.", "All 6
// sounds leveled." with "Everything is saved on your Tone Master Pro.", "Stopped. …") with
// the flow's Steps; at most one Notice (re-amp may still be on, or a message two or more
// sounds share); then the sections "Need you", "Not reached" and "Leveled", each one Group
// of rows in run order under its label (no label when it is the only section); and the
// Footer: "Try again (N)" when another run can fix something, then "Done".
//
// Everything is already saved, so the main button is always "Done" (it closes the flow and
// unchooses on the list what the runs resolved). "Try again (N)" runs those sounds again
// at once with the same settings — no Set up, no backup tick — and comes back to this
// Summary with their rows updated. A sound that can't reach its target says so and what to
// change, with no button. What each row says is `summaryModel`'s, from typed fields only.

import { useMemo, useSyncExternalStore } from "react";

import { getLibraryScan, subscribeLibraryScan } from "../../device/libraryScan";
import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import { Footer } from "../../ui/Footer";
import { Group } from "../../ui/List";
import { Button, Notice } from "../../ui/primitives";
import { FlowPage } from "./FlowPage";
import { SummaryRow } from "./SummaryRow";
import { setupRowHookKey, type RunItem, type TargetLufsOf } from "./leveling";
import { buildSummary } from "./summaryModel";

export interface SummaryPageProps {
  /** Every sound a run of the flow ran, with its latest result, in run order. */
  items: RunItem[];
  /** The last run ended by a Stop. */
  stopped: boolean;
  /** Re-amp may still be on, by the flow's latest reading: the restart Notice. */
  reampStillOn: boolean;
  targetLufsByName: TargetLufsOf;
  /** Footswitch scenes changed here, waiting for Try again. */
  sceneOverrides: Partial<Record<string, number | null>>;
  /** A footswitch's scene picked on its row (`undefined`: back to the one used). */
  onSceneChange: (key: string, scene: number | null | undefined) => void;
  onTryAgain: (keys: readonly string[]) => void;
  onDone: () => void;
}

export function SummaryPage({
  items,
  stopped,
  reampStillOn,
  targetLufsByName,
  sceneOverrides,
  onSceneChange,
  onTryAgain,
  onDone,
}: SummaryPageProps) {
  const { t } = useTheme();
  const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan);

  const summary = useMemo(
    () =>
      buildSummary(items, {
        stopped,
        reampStillOn,
        targetLufsByName,
        sceneNamesOf: (slot) =>
          (lib.sceneInfo.get(slot) ?? []).map((sc) => sc.name),
        enablingScenesOf: (slot, sw) =>
          lib.fsSceneContextsByIndex.get(slot)?.find((r) => r.switch === sw)
            ?.enablingScenes ?? null,
        sceneOverrides,
        hookKeyOf: setupRowHookKey,
      }),
    [items, stopped, reampStillOn, targetLufsByName, lib, sceneOverrides],
  );
  const n = summary.retryKeys.length;

  const onScene = (key: string, value: string) => {
    const row = summary.sections
      .flatMap((s) => s.rows)
      .find((r) => r.scene?.key === key);
    if (!row?.scene) return;
    onSceneChange(
      key,
      value === row.scene.used
        ? undefined
        : value === "base"
          ? null
          : Number(value),
    );
  };

  return (
    <FlowPage
      title={summary.title}
      subtitle={summary.subtitle ?? undefined}
      step={2}
      footer={
        <Footer
          right={
            <>
              {n > 0 && (
                <Button
                  variant="secondary"
                  size="lg"
                  onClick={() => {
                    onTryAgain(summary.retryKeys);
                  }}
                >
                  {`Try again (${String(n)})`}
                </Button>
              )}
              <Button variant="primary" size="lg" onClick={onDone}>
                Done
              </Button>
            </>
          }
        />
      }
    >
      {summary.notice && (
        <Notice
          tone={summary.notice.tone}
          title={summary.notice.title ?? undefined}
        >
          {summary.notice.text}
        </Notice>
      )}
      {summary.sections.map((s) => (
        <section
          key={s.label}
          aria-label={s.label}
          style={{
            display: "flex",
            flexDirection: "column",
            gap: t.sp6,
            marginTop: t.sp4,
          }}
        >
          {/* A lone section needs no label: the title already says it. */}
          {summary.sections.length > 1 && (
            <h2 style={{ ...sectionLabel(t), margin: 0 }}>{s.label}</h2>
          )}
          <Group>
            {s.rows.map((r) => (
              <SummaryRow key={r.key} row={r} onScene={onScene} />
            ))}
          </Group>
        </section>
      ))}
    </FlowPage>
  );
}
