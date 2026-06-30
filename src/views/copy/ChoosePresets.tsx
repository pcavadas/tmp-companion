// src/views/copy/ChoosePresets.tsx — Step 1: pick the reference + the targets.
//
// Header, then a two-column grid (1fr 1fr) filling the area above the action bar:
// "Copy from" (single-select radio rows) on the left with a hairline divider, "Copy to"
// (multi-select checkbox rows, with Select-all / Clear bulk actions) on the right. Each
// column scrolls independently. Search filters by name OR 3-digit slot, per list.

import { useState } from "react";
import { useTheme } from "../../theme/ThemeContext";
import { Button, Checkbox, SearchInput } from "../../ui/primitives";
import { ActionBar } from "../../ui/ActionBar";
import { ReadingPill } from "../../ui/ReadingPill";
import { slotLabel } from "../../lib/format";
import { OnUnitChip, StepBadge, MiniLink } from "./copyBits";
import type { CopyPreset } from "./useCopyLibrary";

export interface ChoosePresetsProps {
  presets: CopyPreset[];
  fromSlot: number | null;
  setFrom: (slot: number) => void;
  toSet: Set<number>;
  setTo: (next: Set<number>) => void;
  /** Per-preset signal graphs (the backup) have settled — gates "Place the blocks". */
  ready: boolean;
  /** The background backup is still streaming. */
  scanning: boolean;
  /** Determinate backup transfer %. */
  percent: number;
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
  fromSlot,
  setFrom,
  toSet,
  setTo,
  ready,
  scanning,
  percent,
  onContinue,
}: ChoosePresetsProps) {
  const { t } = useTheme();
  const [fq, setFq] = useState("");
  const [tq, setTq] = useState("");

  const fromList = presets.filter((p) => matches(p, fq));
  const toList = presets.filter((p) => p.slot !== fromSlot && matches(p, tq));
  const fromName = presets.find((p) => p.slot === fromSlot)?.name ?? "—";

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
  const selectAllFiltered = (): void => {
    const n = new Set(toSet);
    toList.forEach((p) => n.add(p.slot));
    setTo(n);
  };
  const clearAll = (): void => {
    setTo(new Set());
  };

  const empty = (q: string): React.ReactNode => (
    <div
      style={{
        fontFamily: t.sans,
        fontSize: t.fsBody2,
        color: t.faint,
        padding: "12px 4px",
      }}
    >
      No presets match “{q}”.
    </div>
  );

  const sectionLabel: React.CSSProperties = {
    fontFamily: t.mono,
    fontSize: t.fsMeta,
    letterSpacing: t.lsTag,
    textTransform: "uppercase",
    color: t.ink2,
  };
  const rowName: React.CSSProperties = {
    fontFamily: t.serif,
    fontSize: t.fsName,
    color: t.ink,
    flex: 1,
    whiteSpace: "nowrap",
    overflow: "hidden",
    textOverflow: "ellipsis",
  };
  const slotCell: React.CSSProperties = {
    fontFamily: t.mono,
    fontSize: t.fsData,
    color: t.mutedInk,
    width: 30,
    flexShrink: 0,
  };
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div style={{ flexShrink: 0, padding: "15px 22px 9px" }}>
        <div
          style={{
            fontFamily: t.serif,
            fontSize: t.fsTitle,
            color: t.ink,
            letterSpacing: t.lsTight,
          }}
        >
          Copy blocks between presets
        </div>
        <div
          style={{
            fontFamily: t.sans,
            fontSize: t.fsControl,
            color: t.mutedInk,
            marginTop: 3,
          }}
        >
          Pick the preset to copy from, then the presets to copy into. Nothing
          changes on the unit until you save.
        </div>
      </div>

      {/* FROM | TO — two equal columns, each independently scrolling. */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
        }}
      >
        {/* COPY FROM column */}
        <div
          style={{
            minHeight: 0,
            display: "flex",
            flexDirection: "column",
            borderRight: `0.5px solid ${t.hairline}`,
            padding: "4px 16px 0 22px",
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 9,
              margin: "0 0 8px",
            }}
          >
            <StepBadge n={1} />
            <span style={sectionLabel}>Copy from</span>
            <span style={{ flex: 1 }} />
            <span
              style={{
                fontFamily: t.mono,
                fontSize: t.fsData2,
                color: t.faint,
              }}
            >
              {fq
                ? `${String(fromList.length)} of ${String(presets.length)}`
                : `${String(presets.length)} presets`}
            </span>
          </div>
          <SearchInput
            value={fq}
            onChange={setFq}
            placeholder="Search by name or slot…"
            clearable
          />
          <div
            style={{
              flex: 1,
              minHeight: 0,
              overflowY: "auto",
              margin: "7px 0 0",
              display: "flex",
              flexDirection: "column",
              gap: 6,
              paddingRight: 2,
              paddingBottom: 12,
            }}
          >
            {fromList.length > 0
              ? fromList.map((p) => {
                  const sel = fromSlot === p.slot;
                  return (
                    <div
                      key={p.slot}
                      role="button"
                      onClick={() => {
                        pickFrom(p.slot);
                      }}
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 12,
                        padding: "9px 13px",
                        borderRadius: t.rLg,
                        border: `${String(sel ? 1 : 0.5)}px solid ${sel ? t.accent : t.hairlineStrong}`,
                        background: sel ? t.accentSoft : t.bg,
                        cursor: "pointer",
                        flexShrink: 0,
                      }}
                    >
                      <span
                        style={{
                          width: 17,
                          height: 17,
                          borderRadius: t.rPill,
                          border: `1.5px solid ${sel ? t.accent : t.hairlineStrong}`,
                          display: "inline-flex",
                          alignItems: "center",
                          justifyContent: "center",
                          flexShrink: 0,
                        }}
                      >
                        {sel && (
                          <span
                            style={{
                              width: 9,
                              height: 9,
                              borderRadius: t.rPill,
                              background: t.accent,
                            }}
                          />
                        )}
                      </span>
                      <span style={slotCell}>{slotLabel(p.slot)}</span>
                      <span style={rowName}>{p.name}</span>
                      {p.onUnit && <OnUnitChip />}
                    </div>
                  );
                })
              : empty(fq)}
          </div>
        </div>

        {/* COPY TO column */}
        <div
          style={{
            minHeight: 0,
            display: "flex",
            flexDirection: "column",
            padding: "4px 22px 0 16px",
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 9,
              margin: "0 0 8px",
            }}
          >
            <StepBadge n={2} />
            <span style={sectionLabel}>Copy to</span>
            <span
              style={{
                fontFamily: t.mono,
                fontSize: t.fsData2,
                color: toSet.size > 0 ? t.accentDeep : t.faint,
              }}
            >
              {String(toSet.size)} chosen
            </span>
            <span style={{ flex: 1 }} />
            <MiniLink onClick={selectAllFiltered}>
              {fq || tq
                ? `Select these ${String(toList.length)}`
                : "Select all"}
            </MiniLink>
            <span style={{ color: t.hairline }}>·</span>
            <MiniLink onClick={clearAll} disabled={toSet.size === 0}>
              Clear
            </MiniLink>
          </div>
          <SearchInput
            value={tq}
            onChange={setTq}
            placeholder="Search by name or slot…"
            clearable
          />
          <div
            style={{
              flex: 1,
              minHeight: 0,
              overflowY: "auto",
              margin: "7px 0 0",
              display: "flex",
              flexDirection: "column",
              gap: 6,
              paddingRight: 2,
              paddingBottom: 12,
            }}
          >
            {toList.length > 0
              ? toList.map((p) => {
                  const on = toSet.has(p.slot);
                  return (
                    <div
                      key={p.slot}
                      role="button"
                      onClick={() => {
                        toggleTo(p.slot);
                      }}
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 12,
                        padding: "9px 13px",
                        borderRadius: t.rLg,
                        border: `0.5px solid ${on ? t.accent : t.hairlineStrong}`,
                        background: on ? t.accentSoft : t.bg,
                        cursor: "pointer",
                        flexShrink: 0,
                      }}
                    >
                      <Checkbox checked={on} />
                      <span style={slotCell}>{slotLabel(p.slot)}</span>
                      <span style={rowName}>{p.name}</span>
                    </div>
                  );
                })
              : empty(tq)}
          </div>
        </div>
      </div>

      <ActionBar
        left={
          <span
            style={{
              fontFamily: t.sans,
              fontSize: t.fsControl,
              color: t.mutedInk,
            }}
          >
            {toSet.size === 0
              ? "Choose at least one preset to copy into."
              : `From ${fromName} → ${String(toSet.size)} preset${toSet.size === 1 ? "" : "s"}`}
          </span>
        }
        right={
          ready ? (
            <Button
              variant="primary"
              icon="arrow-right"
              disabled={toSet.size === 0 || fromSlot == null}
              onClick={onContinue}
            >
              Place the blocks
            </Button>
          ) : (
            <ReadingPill
              label={
                scanning
                  ? `Reading presets… ${String(Math.round(percent))}%`
                  : undefined
              }
            />
          )
        }
      />
    </div>
  );
}

export default ChoosePresets;
