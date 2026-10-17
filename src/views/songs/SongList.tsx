// src/views/songs/SongList.tsx — All songs (look B): the page title with the song count
// and "New song", then one Group — a "№ / Song" header row and one row per song, each
// with a ⋯ menu (Edit song, Delete song). "New song" opens the form as a card above
// the Group and stays in place, disabled, while it is open; "Edit song" puts the same
// form in place of that song's row. Setlist membership lives in the setlist page (it
// needs a device read per setlist), so it is not shown here.

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import { Button, MenuDivider, MenuItem } from "../../ui/primitives";
import { Group, Row } from "../../ui/List";
import { EmptyState } from "../../ui/EmptyState";
import { MoreMenu } from "../../ui/MoreMenu";
import type { SongRecord } from "../../lib/types";
import { songBpm, songCount } from "./songUtil";
import type { SongDraft } from "./songUtil";
import { SongForm } from "./SongForm";
import { SongRow } from "./SongRow";
import { SongsPage } from "./SongsPage";

interface LibraryRowProps {
  song: SongRecord;
  idx: number;
  busy: boolean;
  onEdit: () => void;
  onDelete: () => void;
}

function LibraryRow({ song, idx, busy, onEdit, onDelete }: LibraryRowProps) {
  const [open, setOpen] = useState(false);
  const pick = (fn: () => void) => () => {
    setOpen(false);
    fn();
  };
  return (
    <SongRow
      song={song}
      idx={idx}
      selected={open && !busy}
      trailing={
        <MoreMenu
          label={`More for ${song.name}`}
          open={open}
          onOpenChange={setOpen}
          disabled={busy}
        >
          <MenuItem label="Edit song" onClick={pick(onEdit)} />
          <MenuDivider />
          <MenuItem label="Delete song" danger onClick={pick(onDelete)} />
        </MoreMenu>
      }
    />
  );
}

export interface SongListProps {
  songs: SongRecord[];
  busy: boolean;
  onAddSong: (d: SongDraft) => void;
  onEditSong: (rec: SongRecord, d: SongDraft) => void;
  onDeleteSong: (rec: SongRecord) => void;
}

export function SongList({
  songs,
  busy,
  onAddSong,
  onEditSong,
  onDeleteSong,
}: SongListProps) {
  const { t } = useTheme();
  const [creating, setCreating] = useState(false);
  const [editingSlot, setEditingSlot] = useState<number | null>(null);

  return (
    <SongsPage
      busy={busy}
      title="All songs"
      subtitle={`${songCount(songs.length)} on your Tone Master Pro`}
      actions={
        <Button
          variant="primary"
          icon="plus"
          disabled={busy || creating}
          onClick={() => {
            setEditingSlot(null);
            setCreating(true);
          }}
        >
          New song
        </Button>
      }
    >
      {creating && (
        <SongForm
          card
          onSave={(d) => {
            onAddSong(d);
            setCreating(false);
          }}
          onCancel={() => {
            setCreating(false);
          }}
        />
      )}
      {songs.length === 0 ? (
        !creating && (
          <Group>
            <EmptyState
              icon="music"
              title="No songs on your Tone Master Pro yet"
            />
          </Group>
        )
      ) : (
        <Group label="Songs">
          <Row header style={{ height: t.control - 1 }}>
            <span style={{ ...sectionLabel(t), width: 30, flexShrink: 0 }}>
              №
            </span>
            <span style={sectionLabel(t)}>Song</span>
          </Row>
          {songs.map((rec, i) =>
            editingSlot === rec.slot ? (
              <div
                key={rec.slot}
                style={{ borderTop: `1px solid ${t.separator}` }}
              >
                <SongForm
                  initial={{
                    name: rec.name,
                    notes: rec.notes,
                    bpm: songBpm(rec),
                  }}
                  onSave={(d) => {
                    onEditSong(rec, d);
                    setEditingSlot(null);
                  }}
                  onCancel={() => {
                    setEditingSlot(null);
                  }}
                />
              </div>
            ) : (
              <LibraryRow
                key={rec.slot}
                song={rec}
                idx={i}
                busy={busy}
                onEdit={() => {
                  setCreating(false);
                  setEditingSlot(rec.slot);
                }}
                onDelete={() => {
                  onDeleteSong(rec);
                }}
              />
            ),
          )}
        </Group>
      )}
    </SongsPage>
  );
}
