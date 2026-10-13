// src/views/songs/SetlistDetail.tsx — SETLIST DETAIL: membership is read from the
// device (global song slots by position), with SetlistRow co-located (its only parent
// is SetlistDetail). Each row carries its DEVICE position; an entry whose song slot
// resolves to no song is shown as an unknown row rather than dropped, so every row
// keeps the position the device holds it at. Reorder works by drag OR by the
// row's ⋯ Move up / Move down (the click-only path the UX north-star asks for).
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
import type { SetlistEntry } from "./songUtil";
import { AddSongs } from "./AddSongs";
import { SongRow } from "./SongRow";
import { ListHeader } from "./ListHeader";

/** What an unresolved entry renders as: it is on the unit's setlist, but its song
 *  slot names no song in the list the app holds. Says so, rather than guessing. */
const unknownSong = (songSlot: number): SongRecord => ({
  slot: songSlot,
  name: "Unknown song",
  notes: "Not in the unit's song list. Reconnect to refresh, or remove it.",
  bpm: 0,
  bpm_active: false,
});

interface SetlistRowProps {
  song: SongRecord;
  idx: number;
  /** A device write is in flight — disable reorder + remove so a second mutation can't
   *  race the first (the backend serializes anyway, but the UI shouldn't invite it). */
  busy: boolean;
  onRemove: () => void;
  /** Click-only reorder; undefined at the list's edge. */
  onMoveUp?: () => void;
  onMoveDown?: () => void;
  onGrab: () => void;
  onDropOn: () => void;
  /** A drag ended (dropped or abandoned) — clear the drag source. */
  onDragEnd: () => void;
}

function SetlistRow({
  song,
  idx,
  busy,
  onRemove,
  onMoveUp,
  onMoveDown,
  onGrab,
  onDropOn,
  onDragEnd,
}: SetlistRowProps) {
  const { t } = useTheme();
  const [menu, setMenu] = useState(false);
  const closeMenuThen = (fn: () => void) => () => {
    setMenu(false);
    fn();
  };
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
        onDragEnd,
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
        <span
          style={{
            display: "flex",
            justifyContent: "flex-end",
            alignItems: "center",
          }}
        >
          {(onMoveUp ?? onMoveDown) && (
            <span style={{ position: "relative", display: "flex" }}>
              <Button
                variant="bare"
                aria-label="Move"
                aria-haspopup="menu"
                aria-expanded={menu && !busy}
                disabled={busy}
                onClick={() => {
                  setMenu((o) => !o);
                }}
                title="Move"
                style={{
                  display: "flex",
                  padding: t.space2,
                  borderRadius: t.rSm,
                }}
              >
                <Icon name="more" size={14} stroke={t.faint} />
              </Button>
              {menu && !busy && (
                <Menu
                  onClose={() => {
                    setMenu(false);
                  }}
                  minWidth={130}
                >
                  {onMoveUp && (
                    <MenuItem
                      label="Move up"
                      onClick={closeMenuThen(onMoveUp)}
                    />
                  )}
                  {onMoveDown && (
                    <MenuItem
                      label="Move down"
                      onClick={closeMenuThen(onMoveDown)}
                    />
                  )}
                </Menu>
              )}
            </span>
          )}
          <Button
            variant="bare"
            aria-label="Remove from setlist"
            disabled={busy}
            onClick={onRemove}
            title="Remove from setlist"
            style={{
              display: "flex",
              padding: t.space2,
              borderRadius: t.rSm,
            }}
          >
            <Icon name="x" size={14} stroke={t.faint} />
          </Button>
        </span>
      }
    />
  );
}

interface SetlistDetailProps {
  setlist: SetlistRecord;
  /** The setlist's entries in device order, or null while not yet read. */
  entries: SetlistEntry[] | null;
  /** Songs not yet in the setlist, or null while its membership is unknown. */
  available: SongRecord[] | null;
  busy: boolean;
  onRename: (name: string) => void;
  onDelete: () => void;
  /** Addressed by the entry's DEVICE position. */
  onRemoveSong: (entry: SetlistEntry) => void;
  onReorder: (from: SetlistEntry, to: SetlistEntry) => void;
  onAdd: (songs: SongRecord[]) => void;
  onCreateAndAdd: (d: SongDraft) => void;
}

export function SetlistDetail({
  setlist,
  entries,
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
  const dragFrom = useRef<SetlistEntry | null>(null);

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

  const rows = entries ?? [];
  const count = entries?.length ?? 0;
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
          padding: `${String(t.space8)}px ${String(t.space8)}px ${String(t.space6)}px`,
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: t.space6,
        }}
      >
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={s.kicker(t.accentDeep)}>
            Setlist
            {entries != null
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
                marginTop: t.space2,
                width: "100%",
                maxWidth: 360,
                borderBottom: `1.5px solid ${t.accent}`,
                fontFamily: t.serif,
                fontSize: t.fsTitle,
                padding: `0 0 ${String(t.space1)}px`,
              })}
            />
          ) : (
            <Button
              variant="bare"
              onClick={() => {
                setRenaming(true);
              }}
              title="Rename"
              style={{
                display: "block",
                width: "100%",
                textAlign: "left",
                marginTop: t.space2,
                cursor: "text",
              }}
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
            </Button>
          )}
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: t.space4,
            flexShrink: 0,
            position: "relative",
          }}
        >
          {/* The picker's anchor box: the trigger + popover only, so a press on the
              neighbouring options button counts as OUTSIDE and dismisses it. */}
          <span style={{ position: "relative", display: "flex" }}>
            <Button
              variant="primary"
              icon="plus"
              size="sm"
              disabled={busy}
              onClick={() => {
                setPicker((o) => !o);
              }}
            >
              Add songs
            </Button>
            {picker && picker_el}
          </span>
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

      {entries == null ? (
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
          Reading this setlist…
        </div>
      ) : rows.length === 0 ? (
        <PaneEmpty
          icon="music"
          title="No songs in this setlist yet"
          body="Add songs from your library — a song can sit in this setlist and others at the same time."
          cta={
            <Button
              variant="primary"
              icon="plus"
              size="sm"
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
            {rows.map((entry, i) => {
              const above = i > 0 ? rows[i - 1] : undefined;
              const below = i + 1 < rows.length ? rows[i + 1] : undefined;
              return (
                <SetlistRow
                  key={entry.position}
                  song={entry.song ?? unknownSong(entry.songSlot)}
                  idx={i}
                  busy={busy}
                  onRemove={() => {
                    onRemoveSong(entry);
                  }}
                  onMoveUp={
                    above
                      ? () => {
                          onReorder(entry, above);
                        }
                      : undefined
                  }
                  onMoveDown={
                    below
                      ? () => {
                          onReorder(entry, below);
                        }
                      : undefined
                  }
                  onDragEnd={() => {
                    dragFrom.current = null;
                  }}
                  onGrab={() => {
                    dragFrom.current = entry;
                  }}
                  onDropOn={() => {
                    if (dragFrom.current != null)
                      onReorder(dragFrom.current, entry);
                    dragFrom.current = null;
                  }}
                />
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
