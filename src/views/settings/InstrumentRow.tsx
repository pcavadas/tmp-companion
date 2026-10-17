// src/views/settings/InstrumentRow.tsx — one instrument in the Instruments Group (look
// B): a two-line block, not a 36 px Row. Left: the name (13/600), its "{type} ·
// {pickup}" caption and a status Badge ("Recorded" / "Not recorded"), then — when there
// is something to say — note lines (the last take's caveats, or why it failed), the
// last take's bands as words with dots, and what leveling uses. Right: the row's control
// (Record / Record again, the countdown, the recording's progress, or Try again) and
// a ⋯ menu (Edit instrument / Move up / Move down / Delete instrument; reorder is
// click-only, so the row carries no drag grip). While the unit is away, Record is shown
// disabled and the page's one Notice says why.
//
// The recording's state machine is ./useCalibration (one tagged union, local per row;
// never disables other rows). Every attempt gets its own token, so a cancelled capture
// that resolves late can't touch a newer attempt's timers or result.

import { useState, type CSSProperties } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Badge } from "../../ui/Badge";
import { Dot } from "../../ui/Dot";
import { MoreMenu } from "../../ui/MoreMenu";
import { ProgressBar } from "../../ui/ProgressBar";
import { Button, MenuDivider, MenuItem } from "../../ui/primitives";
import { instrumentCaption } from "../../lib/instrumentTypes";
import type { Profile, TopologyInfo } from "../../lib/types";
import {
  CALIBRATE_SECS,
  stimulusSource,
  useCalibration,
} from "./useCalibration";

/** Text only a screen reader reads ("Lows played"): the dot beside a band says it to
 *  the eye. */
const SR_ONLY: CSSProperties = {
  position: "absolute",
  width: 1,
  height: 1,
  overflow: "hidden",
  clip: "rect(0 0 0 0)",
  whiteSpace: "nowrap",
};

export interface InstrumentRowProps {
  profile: Profile;
  topology: TopologyInfo | null;
  connected: boolean;
  onCalibrated: () => void;
  onEdit: () => void;
  onDelete: () => void;
  /** Click-only reorder; undefined at the list's edge (the item is hidden). */
  onMoveUp?: () => void;
  onMoveDown?: () => void;
}

export function InstrumentRow({
  profile,
  topology,
  connected,
  onCalibrated,
  onEdit,
  onDelete,
  onMoveUp,
  onMoveDown,
}: InstrumentRowProps) {
  const { t } = useTheme();
  const [menu, setMenu] = useState(false);
  const { cal, start, cancel } = useCalibration(profile.id, onCalibrated);
  const pick = (fn: () => void) => () => {
    setMenu(false);
    fn();
  };
  const startCountdown = () => {
    setMenu(false);
    start();
  };

  const recorded = profile.calibration_lufs != null;
  const busy = cal.kind === "countdown" || cal.kind === "recording";
  // The last take's readout — SESSION-ONLY (a CalibrateResult isn't persisted), so it
  // shows right after a recording and clears on the next attempt.
  const result = cal.kind === "idle" ? cal.result : null;
  const warnings = cal.kind === "idle" ? cal.warnings : [];
  // The "sparse take" note ignores the Air band (6–12 kHz): a passive pickup's DI has
  // no energy up there however the take is played, so an unplayed Air is physics, not
  // a playing gap. The band list still shows it unplayed (honest data).
  const sparse =
    result?.band_coverage.some(
      (covered, i) => !covered && result.band_labels[i] !== "Air",
    ) ?? false;

  const caption = topology ? instrumentCaption(topology) : null;
  const secondary: CSSProperties = {
    fontSize: t.fontCaption,
    lineHeight: `${String(t.lineCaption)}px`,
    color: t.textSecondary,
  };
  const note: CSSProperties = {
    fontSize: t.fontCaption,
    lineHeight: `${String(t.lineCaption)}px`,
    color: t.text,
  };

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: t.sp12,
        padding: `${String(t.sp8 + 2)}px ${String(t.sp12)}px`,
        borderTop: `1px solid ${t.separator}`,
        background: menu ? t.rowSelected : "transparent",
      }}
    >
      <div
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          flexDirection: "column",
          gap: t.sp2,
        }}
      >
        <span
          style={{
            display: "flex",
            alignItems: "center",
            gap: t.sp8,
            minWidth: 0,
          }}
        >
          <span
            style={{
              fontSize: t.fontBody,
              fontWeight: t.weightSemibold,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
              minWidth: 0,
            }}
          >
            {profile.name}
          </span>
          {caption && (
            <span style={{ ...secondary, whiteSpace: "nowrap", flexShrink: 0 }}>
              {caption}
            </span>
          )}
          {cal.kind !== "recording" &&
            (recorded ? (
              <Badge tone="good" icon="check">
                Recorded
              </Badge>
            ) : (
              <Badge tone="caution">Not recorded</Badge>
            ))}
        </span>

        {cal.kind === "error" && (
          <span style={{ ...note, color: t.danger }}>
            Couldn&rsquo;t record it. Check that your instrument is plugged into
            the Tone Master Pro, then try again.
          </span>
        )}
        {warnings.map((w) => (
          <span key={w} style={note}>
            {w}
          </span>
        ))}
        {sparse && (
          <span style={note}>
            Some ranges weren&rsquo;t played, so Doctor sees less of your tone.
            Record again with chords and lead.
          </span>
        )}
        {result && result.band_labels.length > 0 && (
          <span
            style={{
              display: "flex",
              flexWrap: "wrap",
              alignItems: "center",
              columnGap: t.sp8 + 2,
              rowGap: t.sp2,
            }}
          >
            {result.band_labels.map((label, i) => {
              const played = result.band_coverage[i];
              return (
                <span
                  // Index-qualified: band labels are backend-provided, not
                  // guaranteed unique.
                  key={`${String(i)}:${label}`}
                  style={{
                    ...secondary,
                    position: "relative",
                    display: "inline-flex",
                    alignItems: "center",
                    gap: t.sp4,
                  }}
                >
                  <Dot size={6} tone="neutral" hollow={!played} />
                  {label}
                  <span style={SR_ONLY}>
                    {played ? " played" : " not played"}
                  </span>
                </span>
              );
            })}
          </span>
        )}
        <span style={secondary}>{stimulusSource(profile)}</span>
      </div>

      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.sp8 + 2,
          flexShrink: 0,
        }}
      >
        {cal.kind === "idle" && (
          <Button
            variant="secondary"
            size="sm"
            disabled={!connected}
            onClick={startCountdown}
          >
            {recorded ? "Record again" : "Record"}
          </Button>
        )}

        {cal.kind === "countdown" && (
          <>
            <span
              role="status"
              style={{ ...note, fontVariantNumeric: "tabular-nums" }}
            >
              Start playing in {cal.n}
            </span>
            <Button variant="secondary" size="sm" onClick={cancel}>
              Cancel
            </Button>
          </>
        )}

        {cal.kind === "recording" && (
          <>
            <span className="tmp-pulse" style={{ display: "flex" }}>
              <Dot tone="danger" />
            </span>
            <span
              role="status"
              style={{
                ...note,
                whiteSpace: "nowrap",
                fontVariantNumeric: "tabular-nums",
              }}
            >
              Recording {cal.secs.toFixed(1)} of {CALIBRATE_SECS} s
            </span>
            <span
              role="progressbar"
              aria-label="Recording"
              aria-valuemin={0}
              aria-valuemax={CALIBRATE_SECS}
              aria-valuenow={cal.secs}
              style={{ width: 120, flexShrink: 0 }}
            >
              <ProgressBar percent={(cal.secs / CALIBRATE_SECS) * 100} />
            </span>
            <Button variant="secondary" size="sm" onClick={cancel}>
              Cancel
            </Button>
          </>
        )}

        {cal.kind === "error" && (
          <Button
            variant="secondary"
            size="sm"
            disabled={!connected}
            onClick={startCountdown}
          >
            Try again
          </Button>
        )}

        {!busy && (
          <MoreMenu
            label={`More for ${profile.name}`}
            open={menu}
            onOpenChange={setMenu}
          >
            <MenuItem label="Edit instrument" onClick={pick(onEdit)} />
            {onMoveUp && <MenuItem label="Move up" onClick={pick(onMoveUp)} />}
            {onMoveDown && (
              <MenuItem label="Move down" onClick={pick(onMoveDown)} />
            )}
            <MenuDivider />
            <MenuItem
              label="Delete instrument"
              danger
              onClick={pick(onDelete)}
            />
          </MoreMenu>
        )}
      </div>
    </div>
  );
}
