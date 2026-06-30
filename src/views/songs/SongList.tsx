// src/views/songs/SongList.tsx — LIBRARY view: all songs (create / edit / delete),
// with LibraryRow co-located (its only parent is SongList). Setlist membership lives
// in the setlist detail (it needs per-setlist device reads), so it is NOT shown here.
import { useState } from "react";
import { useTheme, useStyles } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button, MenuItem, MenuDivider } from "../../ui/primitives";
import { Menu } from "../../ui/Menu";
import { pad2 } from "../../lib/format";
import type { SongRecord } from "../../lib/types";
import { SONG_COLS } from "./shared";
import { songBpm, bpmStr } from "./songUtil";
import type { SongDraft } from "./shared";
import { SongForm } from "./SongForm";

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
  const bpm = songBpm(song);

  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: SONG_COLS,
        alignItems: "center",
        height: 48,
        padding: "0 16px 0 18px",
        borderBottom: `0.5px solid ${t.hairline}`,
      }}
    >
      <span
        style={{ fontFamily: t.mono, fontSize: t.fsData, color: t.mutedInk }}
      >
        {pad2(idx + 1)}
      </span>
      <div style={{ minWidth: 0, paddingRight: 12 }}>
        <div
          style={{
            fontFamily: t.serif,
            fontSize: t.fsName,
            color: t.ink,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {song.name}
        </div>
        {song.notes && (
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: t.mutedInk,
              marginTop: 1,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {song.notes}
          </div>
        )}
      </div>
      <span
        style={{
          fontFamily: t.mono,
          fontSize: t.fsLabel,
          color: bpm != null ? t.ink2 : t.faint,
        }}
      >
        {bpmStr(bpm)}
      </span>
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
    </div>
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
      <div
        style={{
          display: "grid",
          gridTemplateColumns: SONG_COLS,
          alignItems: "center",
          height: 28,
          padding: "0 16px 0 18px",
          borderBottom: `0.5px solid ${t.hairline}`,
          borderTop: `0.5px solid ${t.hairline}`,
          fontFamily: t.mono,
          fontSize: t.fsMicro,
          letterSpacing: t.lsLabel,
          color: t.faint,
          textTransform: "uppercase",
        }}
      >
        <span>№</span>
        <span>song</span>
        <span>bpm</span>
        <span />
      </div>
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
