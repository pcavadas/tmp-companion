// src/views/settings/PlaybackPane.tsx — the "Playback" page (look B): two settings, each
// a ChoiceGroup that saves the moment it is clicked.
//
// How loud you play (Quiet | Rehearsal | Stage; Stage is the serde default):
// equal-loudness compensation. Equal LUFS only sounds equally loud near the ~60 dBA
// K-weighting was fitted at, and loud low end sounds bigger, so at higher volumes a
// bass is leveled a little lower. The body's figures mirror the REAL backend model
// (`profiles::playback_offset_lu`, bass and Bass VI: Quiet 0 / Rehearsal −1.0 /
// Stage −1.5; a guitar is always 0). Keep them in step if those constants change.
//
// How your speakers are connected (Stereo | Mono (both outputs) | Left output only;
// serde default "stereo"): a stereo rig hears USB 1/2 as a pair, a mono rig hears them
// summed, a rig wired from output 1 alone hears only the left side. Leveling meters what
// the listener hears (`audio::metered_frame`), so a genuinely stereo preset can level
// differently per choice; a mono preset never does.

import type { PlaybackChannels, PlaybackLevel } from "../../lib/types";
import type { SegmentedOption } from "../../ui/primitives";
import { ChoiceGroup } from "./ChoiceGroup";
import { SettingsPage } from "./SettingsPage";
import { StoreNotices } from "./StoreNotices";
import type { SettingsStore } from "./useSettingsStore";

const LEVELS: SegmentedOption<PlaybackLevel>[] = [
  { value: "quiet", label: "Quiet" },
  { value: "rehearsal", label: "Rehearsal" },
  { value: "stage", label: "Stage" },
];

const LEVEL_BODY =
  "The louder you play, the bigger bass sounds. So when you level with a bass, sounds are set 1 dB lower at Rehearsal and 1.5 dB lower at Stage than at Quiet. Guitars don't change.";

const CHANNELS: SegmentedOption<PlaybackChannels>[] = [
  { value: "stereo", label: "Stereo" },
  { value: "mono_sum", label: "Mono (both outputs)" },
  { value: "left_only", label: "Left output only" },
];

const CHANNELS_BODY: Record<PlaybackChannels, string> = {
  stereo: "Both outputs count: two speakers or stereo headphones.",
  mono_sum:
    "Both outputs are mixed into one: one speaker, or a mono PA fed by both outputs.",
  left_only: "Only the left output (1) is plugged in.",
};

export interface PlaybackPaneProps {
  store: Pick<
    SettingsStore,
    | "load"
    | "ready"
    | "saveError"
    | "retryLoad"
    | "playback"
    | "channels"
    | "persistPlayback"
    | "persistChannels"
  >;
}

export function PlaybackPane({ store }: PlaybackPaneProps) {
  return (
    <SettingsPage title="Playback">
      <StoreNotices store={store} />
      {store.ready && (
        <>
          <ChoiceGroup
            label="How loud you play"
            options={LEVELS}
            value={store.playback}
            onChange={(level) => {
              void store.persistPlayback(level);
            }}
            body={LEVEL_BODY}
          />
          <ChoiceGroup
            label="How your speakers are connected"
            options={CHANNELS}
            value={store.channels}
            onChange={(next) => {
              void store.persistChannels(next);
            }}
            body={CHANNELS_BODY[store.channels]}
          />
        </>
      )}
    </SettingsPage>
  );
}
