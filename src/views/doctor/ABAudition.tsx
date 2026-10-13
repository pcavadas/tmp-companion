// src/views/doctor/ABAudition.tsx — the applied-prescription A/B compare: one
// round play/pause + a Before | After SegmentedControl you flip while it loops.
// Both clips loop in lockstep and the inactive side is muted, so a flip is
// seamless. VU decoration is deliberately skipped (decorative-only).

import { useEffect, useRef, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { SegmentedControl } from "../../ui/primitives";
import { microLabel } from "../../theme/tokens";

type Side = "before" | "after";

export interface ABAuditionProps {
  /** `data:audio/wav;base64,…` clip of the stored (pre-fix) state. */
  beforeClip: string;
  /** `data:audio/wav;base64,…` clip of the applied-but-unsaved edit. */
  afterClip: string;
}

export function ABAudition({ beforeClip, afterClip }: ABAuditionProps) {
  const { t } = useTheme();
  const beforeRef = useRef<HTMLAudioElement | null>(null);
  const afterRef = useRef<HTMLAudioElement | null>(null);
  const [playing, setPlaying] = useState(false);
  const [side, setSide] = useState<Side>("before");

  useEffect(() => {
    const before = new Audio(beforeClip);
    const after = new Audio(afterClip);
    before.loop = true;
    after.loop = true;
    beforeRef.current = before;
    afterRef.current = after;
    return () => {
      before.pause();
      after.pause();
      beforeRef.current = null;
      afterRef.current = null;
    };
  }, [beforeClip, afterClip]);

  // Mute the inactive side so both keep looping in lockstep and a flip is instant.
  function applyMute(active: Side) {
    if (beforeRef.current) beforeRef.current.muted = active !== "before";
    if (afterRef.current) afterRef.current.muted = active !== "after";
  }

  function toggle() {
    const before = beforeRef.current;
    const after = afterRef.current;
    if (!before || !after) return;
    if (playing) {
      before.pause();
      after.pause();
      setPlaying(false);
      return;
    }
    applyMute(side);
    before.currentTime = 0;
    after.currentTime = 0;
    void before.play();
    void after.play();
    setPlaying(true);
  }

  function flip(next: Side) {
    setSide(next);
    applyMute(next);
  }

  return (
    <div>
      <div style={{ ...microLabel(t), color: t.mutedInk }}>
        Listen &amp; compare
      </div>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space6,
          marginTop: t.space4,
        }}
      >
        <button
          type="button"
          aria-label={playing ? "Pause" : "Play"}
          onClick={toggle}
          style={{
            width: 40,
            height: 40,
            borderRadius: 999,
            border: `0.5px solid ${t.hairlineStrong}`,
            background: t.bg,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            cursor: "pointer",
            flexShrink: 0,
          }}
        >
          <Icon name={playing ? "pause" : "play"} size={16} stroke={t.ink} />
        </button>
        <div style={{ minWidth: 168 }}>
          <SegmentedControl
            ariaLabel="Compare before or after the fix"
            variant="light"
            options={[
              { value: "before", label: "Before" },
              { value: "after", label: "After" },
            ]}
            value={side}
            onChange={flip}
          />
        </div>
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsData,
            letterSpacing: t.lsTag,
            textTransform: "uppercase",
            color: side === "after" ? t.accentDeep : t.mutedInk,
            marginLeft: "auto",
            flexShrink: 0,
          }}
        >
          {side}
        </span>
      </div>
      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsLabel,
          color: t.mutedInk,
          marginTop: t.space4,
          lineHeight: 1.5,
        }}
      >
        Flip Before / After while it plays — the change is applied on the unit
        but not yet saved.
      </div>
      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsLabel,
          color: t.mutedInk,
          marginTop: 4,
          lineHeight: 1.5,
        }}
      >
        The fixed version can sound a touch quieter — that&apos;s the cut
        working, not a worse tone.
      </div>
    </div>
  );
}

export default ABAudition;
