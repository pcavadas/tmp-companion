// src/views/settings/PlaybackChannelsSection.tsx — the "Outputs" block in the
// Playback category (under the playback level). Which output(s) the rig plays:
// a stereo rig hears USB 1/2 as a pair, a mono rig hears them summed, and a rig
// wired from output 1 alone hears only the left side. Leveling meters what the
// listener hears, so a genuinely stereo preset (ping-pong delay, wide reverb,
// stereo split) can level differently per choice; a mono preset never does.
//
// Mirrors profiles::PlaybackChannels (serde default "stereo" — existing users
// level exactly as before). The meters themselves: audio::metered_frame.

import { useTheme, useStyles } from "../../theme/ThemeContext";
import type { PlaybackChannels } from "../../lib/types";
import { SegmentedControl } from "../../ui/primitives";
import type { SegmentedOption } from "../../ui/primitives";

const OPTIONS: SegmentedOption<PlaybackChannels>[] = [
  { value: "stereo", label: "Stereo" },
  { value: "mono_sum", label: "Mono (summed)" },
  { value: "left_only", label: "Left output only" },
];

interface Info {
  tag: string;
  text: string;
}

const INFO: Record<PlaybackChannels, Info> = {
  stereo: {
    tag: "both outputs",
    text: "Two speakers or stereo headphones — both outputs count.",
  },
  mono_sum: {
    tag: "L + R summed",
    text: "One speaker or a mono PA fed by both outputs.",
  },
  left_only: {
    tag: "output 1 only",
    text: "Only the left output (1) is plugged in.",
  },
};

export interface PlaybackChannelsSectionProps {
  value: PlaybackChannels;
  onChange: (channels: PlaybackChannels) => void;
}

export function PlaybackChannelsSection({
  value,
  onChange,
}: PlaybackChannelsSectionProps) {
  const { t } = useTheme();
  const s = useStyles();
  const info = INFO[value];

  return (
    <div>
      <div style={s.kicker(t.accentDeep)}>Outputs</div>
      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsControl,
          color: t.mutedInk,
          margin: `${String(t.space3)}px 0 ${String(t.space5)}px`,
          lineHeight: 1.5,
        }}
      >
        How your speakers are connected. Leveling measures what you actually
        hear, so stereo effects are judged the way they reach your ears.
      </div>

      <SegmentedControl
        options={OPTIONS}
        value={value}
        onChange={onChange}
        ariaLabel="Outputs"
      />

      <div style={{ marginTop: t.space5, minHeight: 34, lineHeight: 1.55 }}>
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsData2,
            letterSpacing: t.lsMeta,
            textTransform: "uppercase",
            color: value === "stereo" ? t.mutedInk : t.accentDeep,
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
            margin: `0 ${String(t.space4)}px`,
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
