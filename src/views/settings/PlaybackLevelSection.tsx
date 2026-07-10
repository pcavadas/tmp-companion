// src/views/settings/PlaybackLevelSection.tsx — the "Playback level" block in
// the left column (under the targets). Fletcher–Munson compensation: equal-LUFS
// only sounds equal-loud at one SPL, so below stage volume bass presets get a
// small target boost. Stage = reference, no compensation (the serde default).
//
// The per-level chip figures mirror the REAL backend model
// (profiles::playback_offset_lu, bass: Quiet +1.5 / Rehearsal +0.5 / Stage 0 LU;
// guitar is always 0). Keep them in sync if those constants change.

import { useTheme, useStyles } from "../../theme/ThemeContext";
import type { PlaybackLevel } from "../../lib/types";
import { SegmentedControl } from "../../ui/primitives";
import type { SegmentedOption } from "../../ui/primitives";

const OPTIONS: SegmentedOption<PlaybackLevel>[] = [
  { value: "quiet", label: "Quiet" },
  { value: "rehearsal", label: "Rehearsal" },
  { value: "stage", label: "Stage" },
];

interface Info {
  tag: string;
  comp: boolean;
  text: string;
}

const INFO: Record<PlaybackLevel, Info> = {
  quiet: {
    tag: "bass +1.5 LU",
    comp: true,
    text: "Headphones or bedroom — bass gets the most boost.",
  },
  rehearsal: {
    tag: "bass +0.5 LU",
    comp: true,
    text: "Room volume — bass gets a small boost.",
  },
  stage: {
    tag: "no compensation",
    comp: false,
    text: "Stage volume — targets used as set.",
  },
};

export interface PlaybackLevelSectionProps {
  value: PlaybackLevel;
  onChange: (level: PlaybackLevel) => void;
}

export function PlaybackLevelSection({
  value,
  onChange,
}: PlaybackLevelSectionProps) {
  const { t } = useTheme();
  const s = useStyles();
  const info = INFO[value];

  return (
    <div>
      <div style={s.kicker(t.accentDeep)}>Playback level</div>
      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsControl,
          color: t.mutedInk,
          margin: "6px 0 4px",
          lineHeight: 1.5,
        }}
      >
        The volume you’ll play at — bass targets adjust to match how your ear
        hears bass there.
      </div>
      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsLabel,
          color: t.faint,
          margin: "0 0 11px",
          lineHeight: 1.45,
          fontStyle: "italic",
        }}
      >
        <span style={{ fontStyle: "normal", color: t.mutedInk }}>
          Fletcher–Munson
        </span>
        : the quieter you play, the less bass your ear hears — so bass presets
        get a small boost to stay even.
      </div>
      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsLabel,
          color: t.faint,
          margin: "0 0 11px",
          lineHeight: 1.45,
        }}
      >
        Only really matters when you level bass and guitar presets together —
        guitar is never adjusted.
      </div>

      <SegmentedControl
        options={OPTIONS}
        value={value}
        onChange={onChange}
        ariaLabel="Playback level"
      />

      <div style={{ marginTop: 11, minHeight: 34, lineHeight: 1.55 }}>
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsData2,
            letterSpacing: t.lsMeta,
            textTransform: "uppercase",
            color: info.comp ? t.accentDeep : t.mutedInk,
            whiteSpace: "nowrap",
          }}
        >
          {info.tag}
        </span>
        <span
          style={{
            fontFamily: t.sans,
            fontSize: t.fsControl,
            color: t.faint,
            margin: "0 7px",
          }}
        >
          ·
        </span>
        <span
          style={{
            fontFamily: t.sans,
            fontSize: t.fsControl,
            color: t.ink2,
          }}
        >
          {info.text}
        </span>
      </div>
    </div>
  );
}
