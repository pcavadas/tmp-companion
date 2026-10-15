// src/views/settings/InstrumentRow.tsx — status dot + serif name + "Type ·
// Pickup" + status sub-line; right side = the calibrate control + a ⋯ menu (Edit
// / Move up / Move down / Delete — reorder is click-only, so the row carries no
// drag grip). The calibration state machine is ./useCalibration (one tagged union,
// local per row; never disables other rows). Every attempt gets its own token, so a
// cancelled capture that resolves late can't touch a newer attempt's timers or result.

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button, MenuItem, MenuDivider } from "../../ui/primitives";
import { Menu } from "../../ui/Menu";
import type { Profile, TopologyInfo } from "../../lib/types";
import { NeedsDevicePill } from "./NeedsDevicePill";
import {
  CALIBRATE_SECS,
  stimulusSource,
  useCalibration,
} from "./useCalibration";

// ===========================================================================
// InstrumentRow
// ===========================================================================

interface InstrumentRowProps {
  profile: Profile;
  topology: TopologyInfo | null;
  connected: boolean;
  onCalibrated: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onMove: (dir: -1 | 1) => void;
}

export function InstrumentRow({
  profile,
  topology,
  connected,
  onCalibrated,
  onEdit,
  onDelete,
  onMove,
}: InstrumentRowProps) {
  const { t } = useTheme();

  const [menu, setMenu] = useState(false);
  const { cal, start, cancel } = useCalibration(profile.id, onCalibrated);
  const startCountdown = () => {
    setMenu(false);
    start();
  };
  // The render reads the union through these views of it.
  const phase = cal.kind;
  const count = cal.kind === "countdown" ? cal.n : 3;
  const rec = cal.kind === "recording" ? cal.secs : 0;
  const calibErr = cal.kind === "error" ? cal.message : null;
  const calibWarn = cal.kind === "idle" ? cal.warn : null;
  const calibResult = cal.kind === "idle" ? cal.result : null;

  const calibrated = profile.calibration_lufs != null;

  // The status dot and the status sub-line share one phase-derived colour.
  const statusColor =
    phase === "error"
      ? t.warn
      : phase === "recording"
        ? t.record
        : calibrated && phase === "idle"
          ? calibWarn != null
            ? t.sevWarn
            : t.good
          : t.sevWarn;

  const sub =
    phase === "countdown"
      ? "get ready to play…"
      : phase === "recording"
        ? "recording — play steadily"
        : phase === "error"
          ? (calibErr ?? "too quiet to read — the last attempt failed")
          : calibrated
            ? `calibrated ${profile.calibration_lufs?.toFixed(1) ?? ""} LUFS${
                calibWarn != null ? ` — ${calibWarn}` : ""
              }`
            : "not calibrated";
  // What leveling re-amps (#361): its own line, shown in every phase.
  const source = stimulusSource(profile);

  // Type = the topology's instrument label; Pickup = the topology's label.
  const typeLabel = topology?.instrument ?? null;
  const pickupLabel = topology?.label ?? null;
  const caption =
    typeLabel && pickupLabel
      ? `${typeLabel} · ${pickupLabel}`
      : (typeLabel ?? pickupLabel ?? null);

  const closeMenuThen = (fn: () => void) => () => {
    setMenu(false);
    fn();
  };

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: t.space7,
        padding: `${String(t.space5)}px ${String(t.space6)}px ${String(t.space5)}px ${String(t.space4)}px`,
        borderRadius: t.rCard,
        border: `0.5px solid ${phase === "error" ? t.warnBorderSoft : t.hairline}`,
        background: t.bg,
        marginBottom: t.space4,
        minHeight: 60,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space4,
          minWidth: 0,
          flex: 1,
        }}
      >
        <span
          className={phase === "recording" ? "tmp-pulse" : undefined}
          style={{
            width: 7,
            height: 7,
            borderRadius: t.rPill,
            background: statusColor,
            flexShrink: 0,
          }}
        />
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={{ display: "flex", alignItems: "baseline", minWidth: 0 }}>
            <span
              style={{
                fontFamily: t.serif,
                fontSize: t.fsName,
                color: t.ink,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                minWidth: 0,
                flex: "0 1 auto",
              }}
            >
              {profile.name}
            </span>
            {caption && (
              <span
                style={{
                  fontFamily: t.mono,
                  fontSize: t.fsMicro,
                  color: t.faint,
                  letterSpacing: t.lsMeta,
                  whiteSpace: "nowrap",
                  flexShrink: 0,
                  marginLeft: t.space4,
                }}
              >
                {caption}
              </span>
            )}
          </div>
          <div
            title={sub}
            style={{
              fontFamily: t.mono,
              fontSize: t.fsData2,
              marginTop: t.space1,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              color: statusColor,
            }}
          >
            {sub}
          </div>
          <div
            style={{
              fontFamily: t.mono,
              fontSize: t.fsData2,
              marginTop: t.space1,
              whiteSpace: "nowrap",
              color: t.faint,
            }}
          >
            {source}
          </div>
          {/* What-was-captured readout — SESSION-ONLY (calibResult isn't persisted),
              so it's visible right after a calibration and clears on the next attempt. */}
          {phase === "idle" && calibResult && (
            <div
              style={{
                marginTop: t.space2,
                display: "flex",
                flexWrap: "wrap",
                alignItems: "center",
                gap: t.space4,
              }}
            >
              <span
                style={{
                  fontFamily: t.mono,
                  fontSize: t.fsMicro,
                  color: t.faint,
                }}
              >
                spread {calibResult.spread_lu.toFixed(1)} LU
              </span>
              {calibResult.band_labels.map((label, i) => {
                const covered = calibResult.band_coverage[i];
                return (
                  <span
                    // Index-qualified: band labels are backend-provided, not
                    // guaranteed unique.
                    key={`${String(i)}:${label}`}
                    title={`${label} — ${covered ? "played" : "not played"}`}
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: t.space2,
                      fontFamily: t.mono,
                      fontSize: t.fsMicro,
                      color: covered ? t.ink2 : t.faint,
                      opacity: covered ? 1 : 0.45,
                    }}
                  >
                    ● {label}
                  </span>
                );
              })}
            </div>
          )}
          {/* The "sparse take" caveat ignores the Air band (6–12 kHz): a passive
              pickup's DI has no energy up there no matter how the take is played,
              so an uncovered Air is physics, not a playing gap. The chip row above
              still shows it dimmed (honest data); only the warning is gated. */}
          {phase === "idle" &&
            calibResult &&
            calibResult.band_coverage.some(
              (c, i) => !c && calibResult.band_labels[i] !== "Air",
            ) && (
              <div
                style={{
                  marginTop: t.space2,
                  fontFamily: t.sans,
                  fontSize: t.fsMicro,
                  color: t.mutedInk,
                  lineHeight: 1.4,
                }}
              >
                some bands weren&rsquo;t played — a sparse take (for example,
                only EBow drones) limits tone analysis
              </div>
            )}
        </div>
      </div>

      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space3,
          flexShrink: 0,
        }}
      >
        {phase === "idle" &&
          (connected ? (
            <Button
              variant="ghost"
              size="sm"
              icon={calibrated ? "refresh" : "gauge"}
              onClick={startCountdown}
              style={{ whiteSpace: "nowrap" }}
            >
              {calibrated ? "Re-calibrate" : "Calibrate…"}
            </Button>
          ) : (
            <NeedsDevicePill />
          ))}

        {phase === "countdown" && (
          <span
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: t.space4,
              fontFamily: t.mono,
              fontSize: t.fsData,
              color: t.sevWarn,
            }}
          >
            Get ready
            <span
              style={{
                fontFamily: t.mono,
                fontSize: t.fsCard,
                color: t.ink,
                fontVariantNumeric: "tabular-nums",
                minWidth: 12,
                textAlign: "center",
              }}
            >
              {count}
            </span>
            <Button
              variant="bare"
              aria-label="Cancel"
              onClick={cancel}
              title="Cancel"
              style={{ display: "inline-flex" }}
            >
              <Icon name="x" size={12} stroke={t.mutedInk} />
            </Button>
          </span>
        )}

        {phase === "recording" && (
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              alignItems: "flex-end",
              gap: t.space3,
            }}
          >
            <span
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: t.space4,
                fontFamily: t.mono,
                fontSize: t.fsMeta,
                color: t.record,
              }}
            >
              <span
                className="tmp-pulse"
                style={{
                  width: 7,
                  height: 7,
                  borderRadius: t.rPill,
                  background: t.record,
                }}
              />
              Recording {rec.toFixed(1)} / {CALIBRATE_SECS}s
              <Button
                variant="bare"
                aria-label="Cancel"
                onClick={cancel}
                title="Cancel"
                style={{
                  display: "inline-flex",
                  marginLeft: t.space1,
                }}
              >
                <Icon name="x" size={11} stroke={t.mutedInk} />
              </Button>
            </span>
            <div
              style={{
                width: 144,
                height: 3,
                borderRadius: t.rPill,
                background: t.recordSoft,
                overflow: "hidden",
              }}
            >
              <div
                style={{
                  width: `${String((rec / CALIBRATE_SECS) * 100)}%`,
                  height: "100%",
                  background: t.record,
                  borderRadius: t.rPill,
                  transition: "width 0.1s linear",
                }}
              />
            </div>
          </div>
        )}

        {phase === "error" &&
          (connected ? (
            <Button
              variant="ghost"
              size="sm"
              icon="refresh"
              onClick={startCountdown}
            >
              Retry
            </Button>
          ) : (
            <NeedsDevicePill />
          ))}

        {/* per-row menu: edit / reorder / delete */}
        <div style={{ position: "relative", display: "flex" }}>
          <Button
            variant="bare"
            aria-label={`More actions for ${profile.name}`}
            aria-haspopup="menu"
            aria-expanded={menu}
            onClick={() => {
              setMenu((o) => !o);
            }}
            title="More"
            style={{
              display: "flex",
              color: t.faint,
              padding: t.space1,
              borderRadius: t.rMenuItem,
            }}
          >
            <Icon name="more" size={16} stroke="currentColor" />
          </Button>
          {menu && (
            <Menu
              onClose={() => {
                setMenu(false);
              }}
              zIndex={9}
              minWidth={148}
            >
              <MenuItem label="Edit…" onClick={closeMenuThen(onEdit)} />
              <MenuItem
                label="Move up"
                onClick={closeMenuThen(() => {
                  onMove(-1);
                })}
              />
              <MenuItem
                label="Move down"
                onClick={closeMenuThen(() => {
                  onMove(1);
                })}
              />
              <MenuDivider />
              <MenuItem
                label="Delete"
                onClick={closeMenuThen(onDelete)}
                danger
              />
            </Menu>
          )}
        </div>
      </div>
    </div>
  );
}
