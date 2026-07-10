// src/views/songs/SetlistDetail.tsx — SETLIST DETAIL: membership is read from the
// device (ordered global song slots), with SetlistRow co-located (its only parent is
// SetlistDetail).
import { useRef, useState } from "react";
import { useTheme, useStyles } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button, MenuItem, MenuDivider } from "../../ui/primitives";
import { Menu } from "../../ui/Menu";
import { PaneEmpty } from "../../ui/PaneEmpty";
import type { SetlistRecord, SongRecord } from "../../lib/types";
import { LIST_COLS, IconBtn } from "./shared";
import { plainInput } from "../../theme/tokens";
import type { SongDraft } from "./shared";
import { AddSongs } from "./AddSongs";
import { SongRow } from "./SongRow";
import { ListHeader } from "./ListHeader";

interface SetlistRowProps {
  song: SongRecord;
  idx: number;
  /** A device write is in flight — disable reorder + remove so a second mutation can't
   *  race the first (the backend serializes anyway, but the UI shouldn't invite it). */
  busy: boolean;
  onRemove: () => void;
  onGrab: () => void;
  onDropOn: () => void;
}

function SetlistRow({
  song,
  idx,
  busy,
  onRemove,
  onGrab,
  onDropOn,
}: SetlistRowProps) {
  const { t } = useTheme();
  return (
    <SongRow
      song={song}
      idx={idx}
      gridCols={LIST_COLS}
      bpmAlign="right"
      bpmSuffix
      rootProps={{
        draggable: !busy,
        onDragStart: busy ? undefined : onGrab,
        onDragOver: (e) => {
          e.preventDefault();
        },
        onDrop: busy ? undefined : onDropOn,
      }}
      leading={
        <span
          title="Drag to reorder"
          style={{ cursor: busy ? "default" : "grab", display: "flex" }}
        >
          <Icon name="grip" size={14} stroke={t.faint} />
        </span>
      }
      trailing={
        <span style={{ display: "flex", justifyContent: "flex-end" }}>
          <span
            onClick={busy ? undefined : onRemove}
            title="Remove from setlist"
            style={{
              cursor: busy ? "default" : "pointer",
              display: "flex",
              padding: 4,
              borderRadius: t.rSm,
            }}
          >
            <Icon name="x" size={14} stroke={t.faint} />
          </span>
        </span>
      }
    />
  );
}

interface SetlistDetailProps {
  setlist: SetlistRecord;
  /** The setlist's member songs in device order, or null while not yet read. */
  members: SongRecord[] | null;
  available: SongRecord[];
  busy: boolean;
  onRename: (name: string) => void;
  onDelete: () => void;
  /** position = 1-based index within the setlist. */
  onRemoveSong: (position: number) => void;
  onReorder: (oldPos: number, newPos: number) => void;
  onAdd: (songSlots: number[]) => void;
  onCreateAndAdd: (d: SongDraft) => void;
}

export function SetlistDetail({
  setlist,
  members,
  available,
  busy,
  onRename,
  onDelete,
  onRemoveSong,
  onReorder,
  onAdd,
  onCreateAndAdd,
}: SetlistDetailProps) {
  const { t } = useTheme();
  const s = useStyles();
  const [picker, setPicker] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(setlist.name);
  const [menu, setMenu] = useState(false);
  const dragPos = useRef<number | null>(null);

  // Reset the rename draft when a different setlist (or a renamed one) is shown —
  // via React's "adjust state during render when a prop changes" pattern.
  const [prevId, setPrevId] = useState({
    slot: setlist.slot,
    name: setlist.name,
  });
  if (prevId.slot !== setlist.slot || prevId.name !== setlist.name) {
    setPrevId({ slot: setlist.slot, name: setlist.name });
    setName(setlist.name);
    setRenaming(false);
  }

  const songsInList = members ?? [];
  const count = members?.length ?? 0;
  const saveName = () => {
    const n = name.trim();
    if (n && n !== setlist.name) onRename(n);
    else setName(setlist.name);
    setRenaming(false);
  };

  const picker_el = (
    <AddSongs
      available={available}
      onAdd={onAdd}
      onCreateAndAdd={onCreateAndAdd}
      onClose={() => {
        setPicker(false);
      }}
    />
  );

  return (
    <div style={{ minHeight: 0, display: "flex", flexDirection: "column" }}>
      <div
        style={{
          padding: "16px 18px 13px",
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: 12,
        }}
      >
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={s.kicker(t.accentDeep)}>
            Setlist
            {members != null
              ? ` · ${String(count)} song${count === 1 ? "" : "s"}`
              : ""}
          </div>
          {renaming ? (
            <input
              autoFocus
              value={name}
              onChange={(e) => {
                setName(e.target.value);
              }}
              onBlur={saveName}
              onKeyDown={(e) => {
                if (e.key === "Enter") saveName();
                if (e.key === "Escape") {
                  setName(setlist.name);
                  setRenaming(false);
                }
              }}
              style={plainInput(t, {
                marginTop: 4,
                width: "100%",
                maxWidth: 360,
                borderBottom: `1.5px solid ${t.accent}`,
                fontFamily: t.serif,
                fontSize: t.fsTitle,
                padding: "0 0 2px",
              })}
            />
          ) : (
            <div
              onClick={() => {
                setRenaming(true);
              }}
              title="Rename"
              style={{ marginTop: 4, cursor: "text" }}
            >
              <span
                style={{
                  fontFamily: t.serif,
                  fontSize: t.fsTitle,
                  color: t.ink,
                  display: "block",
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {setlist.name}
              </span>
            </div>
          )}
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            flexShrink: 0,
            position: "relative",
          }}
        >
          <Button
            variant="primary"
            icon="plus"
            small
            disabled={busy}
            onClick={() => {
              setPicker((o) => !o);
            }}
          >
            Add songs
          </Button>
          {picker && picker_el}
          <div style={{ position: "relative", display: "flex" }}>
            <IconBtn
              icon="more"
              title="Setlist options"
              disabled={busy}
              onClick={() => {
                setMenu((o) => !o);
              }}
            />
            {menu && (
              <Menu
                onClose={() => {
                  setMenu(false);
                }}
                minWidth={150}
              >
                <MenuItem
                  label="Rename setlist…"
                  onClick={() => {
                    setMenu(false);
                    setRenaming(true);
                  }}
                />
                <MenuDivider />
                <MenuItem
                  label="Delete setlist"
                  danger
                  onClick={() => {
                    setMenu(false);
                    onDelete();
                  }}
                />
              </Menu>
            )}
          </div>
        </div>
      </div>

      {members == null ? (
        <div
          style={{
            flex: 1,
            minHeight: 0,
            borderTop: `0.5px solid ${t.hairline}`,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            fontFamily: t.mono,
            fontSize: t.fsUi,
            color: t.mutedInk,
          }}
        >
          reading this setlist…
        </div>
      ) : songsInList.length === 0 ? (
        <PaneEmpty
          icon="music"
          title="No songs in this setlist yet"
          body="Add songs from your library — a song can sit in this setlist and others at the same time."
          cta={
            <Button
              variant="primary"
              icon="plus"
              small
              disabled={busy}
              onClick={() => {
                setPicker(true);
              }}
            >
              Add songs
            </Button>
          }
        />
      ) : (
        <>
          <ListHeader
            cols={LIST_COLS}
            cells={[
              { label: "" },
              { label: "№" },
              { label: "song" },
              { label: "bpm", align: "right" },
              { label: "" },
            ]}
          />
          <div
            style={{
              flex: 1,
              minHeight: 0,
              overflow: "hidden",
              overflowY: "auto",
            }}
          >
            {songsInList.map((rec, i) => (
              <SetlistRow
                key={rec.slot}
                song={rec}
                idx={i}
                busy={busy}
                onRemove={() => {
                  onRemoveSong(i + 1);
                }}
                onGrab={() => {
                  dragPos.current = i + 1;
                }}
                onDropOn={() => {
                  if (dragPos.current != null)
                    onReorder(dragPos.current, i + 1);
                  dragPos.current = null;
                }}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}
