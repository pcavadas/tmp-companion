// src/views/settings/InstrumentRow.tsx — status dot + serif name + "Type ·
// Pickup" + status sub-line; right side = the calibrate control + a ⋯ menu. The
// calibration state machine lives here (local per-row state; never disables other
// rows). Split out of SettingsView.tsx (mechanical extraction).

import { useEffect, useRef, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button, MenuItem, MenuDivider } from "../../ui/primitives";
import { Menu } from "../../ui/Menu";
import { calibrateProfile } from "../../lib/invoke";
import { errMsg } from "../../lib/format";
import type { Profile, TopologyInfo } from "../../lib/types";
import { NeedsDevicePill } from "./SettingsView";

// Tier-2 capture length passed to calibrate_profile (backend clamps 2..30).
const CALIBRATE_SECS = 8;

// ===========================================================================
// InstrumentRow
// ===========================================================================

type Phase = "idle" | "countdown" | "recording" | "error";

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

  const [phase, setPhase] = useState<Phase>("idle");
  const [count, setCount] = useState(3);
  const [rec, setRec] = useState(0);
  const [menu, setMenu] = useState(false);
  const [calibErr, setCalibErr] = useState<string | null>(null);
  // Non-fatal quality caveats from the last calibration (clip / stimulus ceiling).
  const [calibWarn, setCalibWarn] = useState<string | null>(null);

  // Live timers. `abortedRef` gates the in-flight calibrate_profile promise's resolve so a
  // cancelled / unmounted row never applies its result (setState after unmount, or a
  // spurious onCalibrated). It does NOT stop the backend capture itself — that still runs
  // to completion on the device and may persist; the recording-cancel path re-reads the
  // store to surface any value the device already saved.
  const countdownRef = useRef<number | null>(null);
  const recIntervalRef = useRef<number | null>(null);
  const abortedRef = useRef(false);

  function clearTimers() {
    if (countdownRef.current != null) {
      window.clearTimeout(countdownRef.current);
      countdownRef.current = null;
    }
    if (recIntervalRef.current != null) {
      window.clearInterval(recIntervalRef.current);
      recIntervalRef.current = null;
    }
  }

  // On unmount, abort the in-flight promise too (not just the timers) so its .then()/
  // .catch() can't setState on an unmounted row or fire a spurious onCalibrated.
  useEffect(
    () => () => {
      abortedRef.current = true;
      clearTimers();
    },
    [],
  );

  const calibrated = profile.calibration_lufs != null;

  // Countdown 3 → 2 → 1 at ~850ms steps, then start recording.
  function startCountdown() {
    setMenu(false);
    abortedRef.current = false;
    clearTimers();
    setCalibWarn(null);
    setPhase("countdown");
    let n = 3;
    setCount(n);
    const tick = () => {
      n -= 1;
      if (n <= 0) {
        startRecording();
        return;
      }
      setCount(n);
      countdownRef.current = window.setTimeout(tick, 850);
    };
    countdownRef.current = window.setTimeout(tick, 850);
  }

  // Recording: a DETERMINATE bar animates 0→CALIBRATE_SECS over ~8s while the
  // REAL backend calibration runs concurrently. Backend result wins the phase.
  function startRecording() {
    setPhase("recording");
    setRec(0);
    const stepMs = 100;
    const stepLu = (CALIBRATE_SECS * stepMs) / 1000; // seconds advanced per step
    recIntervalRef.current = window.setInterval(() => {
      setRec((r) => Math.min(CALIBRATE_SECS, +(r + stepLu).toFixed(1)));
    }, stepMs);

    calibrateProfile(profile.id, CALIBRATE_SECS)
      .then((res) => {
        if (abortedRef.current) return;
        clearTimers();
        const warns: string[] = [];
        if (res.clipped)
          warns.push(
            "signal clipped — re-calibrate with softer playing or the guitar volume rolled back",
          );
        if (res.stimulus_shortfall_lu != null)
          warns.push(
            `instrument hotter than the test signal can reproduce — leveling drives ~${res.stimulus_shortfall_lu.toFixed(1)} LU softer`,
          );
        setCalibWarn(warns.join("; ") || null);
        setPhase("idle");
        onCalibrated();
      })
      .catch((e: unknown) => {
        if (abortedRef.current) return;
        clearTimers();
        setCalibErr(errMsg(e));
        setPhase("error");
      });
  }

  function cancel() {
    const wasRecording = phase === "recording";
    abortedRef.current = true;
    clearTimers();
    setPhase("idle");
    // The backend calibrate_profile may have ALREADY persisted before the user hit cancel;
    // re-read the store so a just-saved value surfaces instead of staying "not calibrated".
    // (The in-flight promise's .then() is aborted, so this can't double-apply.)
    if (wasRecording) onCalibrated();
  }

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
  const dot = statusColor;
  const subColor = statusColor;

  const sub =
    phase === "countdown"
      ? "get ready to play…"
      : phase === "recording"
        ? "recording — play steadily"
        : phase === "error"
          ? (calibErr ?? "too quiet to read — last attempt failed")
          : calibrated
            ? `calibrated ${profile.calibration_lufs?.toFixed(1) ?? ""} LUFS${
                calibWarn != null ? ` — ${calibWarn}` : ""
              }`
            : "not calibrated";

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
        gap: 14,
        padding: "11px 12px 11px 8px",
        borderRadius: t.rCard,
        border: `0.5px solid ${phase === "error" ? "rgba(167,70,31,0.4)" : t.hairline}`,
        background: t.bg,
        marginBottom: 8,
        minHeight: 60,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          minWidth: 0,
          flex: 1,
        }}
      >
        <span style={{ display: "flex", color: t.faint, flexShrink: 0 }}>
          <Icon name="grip" size={14} stroke="currentColor" />
        </span>
        <span
          className={phase === "recording" ? "tmp-pulse" : undefined}
          style={{
            width: 7,
            height: 7,
            borderRadius: t.rPill,
            background: dot,
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
                  marginLeft: 8,
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
              marginTop: 2,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              color: subColor,
            }}
          >
            {sub}
          </div>
        </div>
      </div>

      <div
        style={{ display: "flex", alignItems: "center", gap: 6, flexShrink: 0 }}
      >
        {phase === "idle" &&
          (connected ? (
            <Button
              variant="ghost"
              small
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
              gap: 9,
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
            <span
              role="button"
              aria-label="Cancel"
              onClick={cancel}
              title="Cancel"
              style={{ cursor: "pointer", display: "inline-flex" }}
            >
              <Icon name="x" size={12} stroke={t.mutedInk} />
            </span>
          </span>
        )}

        {phase === "recording" && (
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              alignItems: "flex-end",
              gap: 5,
            }}
          >
            <span
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 7,
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
              <span
                role="button"
                aria-label="Cancel"
                onClick={cancel}
                title="Cancel"
                style={{
                  cursor: "pointer",
                  display: "inline-flex",
                  marginLeft: 1,
                }}
              >
                <Icon name="x" size={11} stroke={t.mutedInk} />
              </span>
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
              small
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
          <span
            onClick={() => {
              setMenu((o) => !o);
            }}
            title="More"
            style={{
              cursor: "pointer",
              display: "flex",
              color: t.faint,
              padding: 2,
              borderRadius: t.rMenuItem,
            }}
          >
            <Icon name="more" size={16} stroke="currentColor" />
          </span>
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
