// src/views/copy/ChoosePresets.tsx — Step 1: pick the preset to copy from and the presets
// to copy into (look B).
//
// The title, then two equal columns that each scroll on their own: "Copy from" (one
// choice, a Radio per row) and "Copy into" (a Checkbox per row; the source preset is not
// listed), each with a 40-high head holding its section label and a Search field. "Copy
// into" also has "Select all N shown", which ticks every row the search shows, or clears
// them when all are ticked. A whole row is the click target (its <label> passes the click
// to the Radio or Checkbox). The Footer says what will happen ("From X into 2 presets")
// or why "Choose blocks" waits: no target yet, or the preset details still reading (a busy
// button with the read's percent). On a wide window the columns stay 1:1 and the page
// keeps 1100 px at most, centred.

import { useState, type ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import { slotLabel } from "../../lib/format";
import { Button, Checkbox, Radio, SearchInput } from "../../ui/primitives";
import { Badge } from "../../ui/Badge";
import { EmptyState } from "../../ui/EmptyState";
import { Footer } from "../../ui/Footer";
import { Group, Row } from "../../ui/List";
import { PageHead } from "../../ui/PageHead";
import { Skel } from "../../ui/Skeleton";
import { SlotLabel } from "../../ui/SlotLabel";
import { useLibraryScanPercent } from "../../device/libraryScan";
import {
  COPY_TEXT,
  copyFromName,
  copyIntoName,
  fromInto,
  noMatch,
  readingPercent,
  selectAllShown,
} from "./copyText";
import type { CopyPreset } from "./useCopyLibrary";

/** The page's widest (px) on a wide window. */
const MAX_PAGE_W = 1100;

export interface ChoosePresetsProps {
  presets: CopyPreset[];
  /** The preset list is still arriving (ghost rows). */
  listLoading: boolean;
  fromSlot: number | null;
  setFrom: (slot: number) => void;
  toSet: Set<number>;
  setTo: (next: Set<number>) => void;
  /** Per-preset signal graphs (the backup) have settled — gates "Choose blocks". */
  ready: boolean;
  /** The background backup is still streaming (its % is read by ReadingCaption). */
  scanning: boolean;
  onContinue: () => void;
}

function matches(p: CopyPreset, q: string): boolean {
  const query = q.trim().toLowerCase();
  if (!query) return true;
  return (
    p.name.toLowerCase().includes(query) || slotLabel(p.slot).includes(query)
  );
}

export function ChoosePresets({
  presets,
  listLoading,
  fromSlot,
  setFrom,
  toSet,
  setTo,
  ready,
  scanning,
  onContinue,
}: ChoosePresetsProps) {
  const { t } = useTheme();
  const [fq, setFq] = useState("");
  const [tq, setTq] = useState("");

  const fromList = presets.filter((p) => matches(p, fq));
  const toList = presets.filter((p) => p.slot !== fromSlot && matches(p, tq));
  const fromName = presets.find((p) => p.slot === fromSlot)?.name ?? "";

  // "Select all N shown" acts on the rows the search shows (like Level's).
  const shown = toList.map((p) => p.slot);
  const onCount = shown.filter((s) => toSet.has(s)).length;
  const allOn = shown.length > 0 && onCount === shown.length;
  const toggleAll = (): void => {
    const n = new Set(toSet);
    for (const s of shown) {
      if (allOn) n.delete(s);
      else n.add(s);
    }
    setTo(n);
  };

  const toggleTo = (slot: number): void => {
    const n = new Set(toSet);
    if (n.has(slot)) n.delete(slot);
    else n.add(slot);
    setTo(n);
  };
  const pickFrom = (slot: number): void => {
    setFrom(slot);
    // A preset can't copy into itself — drop it from the targets.
    if (toSet.has(slot)) {
      const n = new Set(toSet);
      n.delete(slot);
      setTo(n);
    }
  };

  // A row's line: the whole row is the <label> of its Radio or Checkbox.
  const line = (
    control: ReactNode,
    p: CopyPreset,
    showLoaded: boolean,
  ): ReactNode => (
    <label
      style={{
        flex: 1,
        minWidth: 0,
        height: "100%",
        display: "flex",
        alignItems: "center",
        gap: t.sp8 + t.sp2,
        cursor: "pointer",
      }}
    >
      {control}
      <SlotLabel index={p.slot} style={{ width: 30, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          minWidth: 0,
          fontSize: t.fontBody,
          fontWeight: t.weightMedium,
          color: t.text,
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
        }}
      >
        {p.name}
      </span>
      {showLoaded && p.onUnit && <Badge>{COPY_TEXT.loaded}</Badge>}
    </label>
  );

  const searchEmpty = (q: string, clear: () => void): ReactNode => (
    <Group>
      <EmptyState
        icon="search"
        title={noMatch(q.trim())}
        cta={
          <Button variant="link" onClick={clear}>
            {COPY_TEXT.clearSearch}
          </Button>
        }
      />
    </Group>
  );

  const fromBody: ReactNode = listLoading ? (
    <RowsSkeleton />
  ) : fromList.length > 0 ? (
    <Group>
      <div role="radiogroup" aria-label={COPY_TEXT.from}>
        {fromList.map((p) => {
          const sel = fromSlot === p.slot;
          return (
            <Row key={p.slot} selected={sel}>
              {line(
                <Radio
                  checked={sel}
                  onSelect={() => {
                    pickFrom(p.slot);
                  }}
                  label={copyFromName(p.name)}
                  labelHidden
                />,
                p,
                true,
              )}
            </Row>
          );
        })}
      </div>
    </Group>
  ) : fq.trim() ? (
    searchEmpty(fq, () => {
      setFq("");
    })
  ) : (
    <Group>
      <EmptyState icon="search" title={COPY_TEXT.noPresets} />
    </Group>
  );

  const toBody: ReactNode = listLoading ? (
    <RowsSkeleton />
  ) : toList.length > 0 ? (
    <Group label={COPY_TEXT.into}>
      {toList.map((p) => {
        const on = toSet.has(p.slot);
        return (
          <Row key={p.slot} selected={on}>
            {line(
              <Checkbox
                checked={on}
                onChange={() => {
                  toggleTo(p.slot);
                }}
                label={copyIntoName(p.name)}
                labelHidden
              />,
              p,
              false,
            )}
          </Row>
        );
      })}
    </Group>
  ) : tq.trim() ? (
    searchEmpty(tq, () => {
      setTq("");
    })
  ) : (
    <Group>
      <EmptyState icon="search" title={COPY_TEXT.noOther} />
    </Group>
  );

  const caption = {
    fontSize: t.fontCaption,
    color: t.textSecondary,
    whiteSpace: "nowrap",
  } as const;

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        background: t.window,
        fontFamily: t.fontSans,
        color: t.text,
      }}
    >
      <div
        style={{
          flex: 1,
          minHeight: 0,
          width: "100%",
          maxWidth: MAX_PAGE_W,
          margin: "0 auto",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <div
          style={{
            padding: `${String(t.sp12 + t.sp2)}px ${String(t.sp24)}px 0`,
          }}
        >
          <PageHead title={COPY_TEXT.title} />
        </div>
        <div
          style={{
            flex: 1,
            minHeight: 0,
            display: "grid",
            gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)",
            gap: t.sp16,
            padding: `${String(t.sp4)}px ${String(t.sp16)}px ${String(t.sp16)}px`,
          }}
        >
          <Column
            label={COPY_TEXT.from}
            search={
              <SearchInput
                value={fq}
                onChange={setFq}
                placeholder={COPY_TEXT.search}
                ariaLabel={COPY_TEXT.searchFrom}
                style={{ width: 200, flexShrink: 1, minWidth: 0 }}
              />
            }
          >
            {fromBody}
          </Column>
          <Column
            label={COPY_TEXT.into}
            extra={
              <Checkbox
                checked={allOn}
                indeterminate={onCount > 0 && !allOn}
                disabled={listLoading || shown.length === 0}
                onChange={toggleAll}
                label={selectAllShown(shown.length)}
                style={{ whiteSpace: "nowrap", flexShrink: 0 }}
              />
            }
            search={
              <SearchInput
                value={tq}
                onChange={setTq}
                placeholder={COPY_TEXT.search}
                ariaLabel={COPY_TEXT.searchInto}
                style={{ width: 200, flexShrink: 1, minWidth: 0 }}
              />
            }
          >
            {toBody}
          </Column>
        </div>
      </div>

      <Footer
        right={
          <>
            {!ready ? (
              <ReadingCaption scanning={scanning} />
            ) : (
              <span style={caption}>
                {toSet.size === 0
                  ? COPY_TEXT.chooseInto
                  : fromInto(fromName, toSet.size)}
              </span>
            )}
            <Button
              variant="primary"
              size="lg"
              busy={!ready}
              disabled={toSet.size === 0 || fromSlot == null}
              onClick={onContinue}
            >
              {COPY_TEXT.chooseBlocks}
            </Button>
          </>
        }
      />
    </div>
  );
}

interface ColumnProps {
  /** The section label. */
  label: string;
  /** Left of the search (Copy into's "Select all N shown"). */
  extra?: ReactNode;
  search: ReactNode;
  children: ReactNode;
}

// One column: a 40-high head (label left, search right), then its own scrolling list.
function Column({ label, extra, search, children }: ColumnProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        minHeight: 0,
        minWidth: 0,
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div
        style={{
          height: 40,
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          gap: t.sp8,
        }}
      >
        <span style={{ ...sectionLabel(t), whiteSpace: "nowrap" }}>
          {label}
        </span>
        <span style={{ flex: 1 }} />
        {extra}
        {search}
      </div>
      <div style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>{children}</div>
    </div>
  );
}

// Ghost rows while the preset list arrives, on the real row's line.
function RowsSkeleton() {
  return (
    <Group>
      {Array.from({ length: 8 }).map((_, i) => (
        <Row key={i}>
          <Skel w={14} h={14} r={7} />
          <Skel w={22} h={10} style={{ marginRight: 8 }} />
          <Skel w={[120, 150, 96, 168, 110, 136, 90, 158][i]} h={11} />
        </Row>
      ))}
    </Group>
  );
}

interface ReadingCaptionProps {
  scanning: boolean;
}

// The only part of the page that follows the backup's transfer %, so a progress tick
// repaints this caption instead of the whole page.
function ReadingCaption({ scanning }: ReadingCaptionProps) {
  const { t } = useTheme();
  const percent = useLibraryScanPercent();
  return (
    <span
      style={{
        fontSize: t.fontCaption,
        color: t.textSecondary,
        whiteSpace: "nowrap",
        fontVariantNumeric: "tabular-nums",
      }}
    >
      {scanning ? readingPercent(percent) : COPY_TEXT.readingPresets}
    </span>
  );
}
