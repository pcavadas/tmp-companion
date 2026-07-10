// src/views/songs/SongList.tsx — LIBRARY view: all songs (create / edit / delete),
// with LibraryRow co-located (its only parent is SongList). Setlist membership lives
// in the setlist detail (it needs per-setlist device reads), so it is NOT shown here.
import { useState } from "react";
import { useTheme, useStyles } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button, MenuItem, MenuDivider } from "../../ui/primitives";
import { Menu } from "../../ui/Menu";
import type { SongRecord } from "../../lib/types";
import { SONG_COLS } from "./shared";
import { songBpm } from "./songUtil";
import type { SongDraft } from "./shared";
import { SongForm } from "./SongForm";
import { SongRow } from "./SongRow";
import { ListHeader } from "./ListHeader";

interface LibraryRowProps {
  song: SongRecord;
  idx: number;
  busy: boolean;
  onEdit: () => void;
  onDelete: () => void;
}

function LibraryRow({ song, idx, busy, onEdit, onDelete }: LibraryRowProps) {
  const { t } = useTheme();
  const [open, setOpen] = useState(false);

  return (
    <SongRow
      song={song}
      idx={idx}
      gridCols={SONG_COLS}
      trailing={
        <div
          style={{
            display: "flex",
            justifyContent: "flex-end",
            alignItems: "center",
            gap: 7,
          }}
        >
          <div style={{ position: "relative", display: "flex" }}>
            <span
              onClick={
                busy
                  ? undefined
                  : () => {
                      setOpen((o) => !o);
                    }
              }
              title="More"
              style={{
                cursor: busy ? "default" : "pointer",
                display: "flex",
                padding: 2,
                borderRadius: t.rSm,
                opacity: busy ? 0.4 : 1,
              }}
            >
              <Icon name="more" size={16} stroke={t.faint} />
            </span>
            {open && (
              <Menu
                onClose={() => {
                  setOpen(false);
                }}
                minWidth={138}
              >
                <MenuItem
                  label="Edit song…"
                  onClick={() => {
                    setOpen(false);
                    onEdit();
                  }}
                />
                <MenuDivider />
                <MenuItem
                  label="Delete song"
                  danger
                  onClick={() => {
                    setOpen(false);
                    onDelete();
                  }}
                />
              </Menu>
            )}
          </div>
        </div>
      }
    />
  );
}

interface SongListProps {
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
  const s = useStyles();
  const [creating, setCreating] = useState(false);
  const [editingSlot, setEditingSlot] = useState<number | null>(null);

  return (
    <div style={{ minHeight: 0, display: "flex", flexDirection: "column" }}>
      <div
        style={{
          padding: "16px 18px 13px",
          display: "flex",
          alignItems: "flex-end",
          justifyContent: "space-between",
          gap: 12,
        }}
      >
        <div>
          <div style={s.kicker(t.accentDeep)}>Library</div>
          <div
            style={{
              fontFamily: t.serif,
              fontSize: t.fsTitle,
              color: t.ink,
              marginTop: 4,
            }}
          >
            All songs
          </div>
          <div
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMeta,
              color: t.mutedInk,
              marginTop: 5,
            }}
          >
            {songs.length} song{songs.length === 1 ? "" : "s"} on the unit
          </div>
        </div>
        {!creating && (
          <Button
            variant="primary"
            icon="plus"
            small
            disabled={busy}
            onClick={() => {
              setEditingSlot(null);
              setCreating(true);
            }}
          >
            New song
          </Button>
        )}
      </div>
      <ListHeader
        cols={SONG_COLS}
        cells={[
          { label: "№" },
          { label: "song" },
          { label: "bpm" },
          { label: "" },
        ]}
      />
      <div
        style={{ flex: 1, minHeight: 0, overflow: "hidden", overflowY: "auto" }}
      >
        {creating && (
          <div
            style={{
              padding: "10px 16px 10px 18px",
              borderBottom: `0.5px solid ${t.hairline}`,
              background: t.bgAlt,
            }}
          >
            <SongForm
              onSave={(d) => {
                onAddSong(d);
                setCreating(false);
              }}
              onCancel={() => {
                setCreating(false);
              }}
            />
          </div>
        )}
        {songs.map((rec, i) =>
          editingSlot === rec.slot ? (
            <div
              key={rec.slot}
              style={{
                padding: "10px 16px 10px 18px",
                borderBottom: `0.5px solid ${t.hairline}`,
                background: t.bgAlt,
              }}
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
        {songs.length === 0 && !creating && (
          <div
            style={{
              padding: "48px 0",
              textAlign: "center",
              color: t.faint,
              fontFamily: t.sans,
              fontSize: t.fsBody,
            }}
          >
            No songs on the unit. Create your first song.
          </div>
        )}
      </div>
    </div>
  );
}
