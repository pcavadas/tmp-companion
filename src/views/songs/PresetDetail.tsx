// src/views/songs/PresetDetail.tsx — the Presets side's page (look B): a READ-ONLY list
// of the songs that use one preset. Which songs use a preset is set ON THE UNIT; this
// page only shows it, from the startup backup scan, so its rows have no ⋯ and no click,
// and one caption under the list says where it is set. The title is the preset's name,
// the subtitle how many songs use it (hidden while that is unknown, and at none).

import { useTheme } from "../../theme/ThemeContext";
import { Group } from "../../ui/List";
import { EmptyState } from "../../ui/EmptyState";
import type { SongRecord } from "../../lib/types";
import { songCount } from "./songUtil";
import { SongRow } from "./SongRow";
import { SongsPage } from "./SongsPage";

const SET_ON_UNIT = "Which songs use a preset is set on your Tone Master Pro.";

export interface PresetDetailProps {
  /** `slot` is the 0-based My-Presets list index. */
  preset: { slot: number; name: string };
  /** The songs that use this preset (read-only); null = UNKNOWN, because a song was
   *  added or removed since the startup scan and the song slots it joined on shifted. */
  members: SongRecord[] | null;
}

export function PresetDetail({ preset, members }: PresetDetailProps) {
  const { t } = useTheme();
  return (
    <SongsPage
      title={preset.name}
      // Hidden while unknown, and when no song uses it (its empty state says so).
      subtitle={
        members == null || members.length === 0
          ? undefined
          : `Used by ${songCount(members.length)}`
      }
    >
      {members == null ? (
        <Group>
          <EmptyState
            icon="wave"
            title="Not known right now"
            body="Adding or removing a song renumbers your songs. Reconnect your Tone Master Pro to read this again."
          />
        </Group>
      ) : members.length === 0 ? (
        <Group>
          <EmptyState
            icon="wave"
            title="No songs use this preset"
            body={SET_ON_UNIT}
          />
        </Group>
      ) : (
        <>
          <Group label={`Songs that use ${preset.name}`}>
            {members.map((s, i) => (
              <SongRow key={s.slot} song={s} idx={i} />
            ))}
          </Group>
          <p
            style={{
              margin: 0,
              fontSize: t.fontCaption,
              lineHeight: `${String(t.lineCaption)}px`,
              color: t.textSecondary,
            }}
          >
            {SET_ON_UNIT}
          </p>
        </>
      )}
    </SongsPage>
  );
}
