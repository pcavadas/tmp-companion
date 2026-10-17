// src/views/songs/SetlistDetail.tsx — one setlist (look B). The title is the setlist's
// name (a click renames it in place), the subtitle its song count; at the right, "Add
// songs" (the picker) and the setlist's ⋯ (Rename setlist, Delete setlist). Below, one
// Group with no header row: one row per entry, each with a grip (drag to reorder) and
// a ⋯ holding Move up / Move down and Remove from setlist — the click path for every
// change, so nothing on a row removes a song in one stray click.
//
// Membership is read from the device (global song slots by position), with SetlistRow
// co-located (its only parent is SetlistDetail). Each row carries its DEVICE position;
// an entry whose song slot resolves to no song is shown as an unknown row rather than
// dropped, so every row keeps the position the device holds it at.

import { useRef, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { plainButton } from "../../theme/tokens";
import { Button, MenuDivider, MenuItem } from "../../ui/primitives";
import { Group } from "../../ui/List";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { InlineEdit } from "../../ui/InlineEdit";
import { MoreMenu } from "../../ui/MoreMenu";
import { Spinner } from "../../ui/Spinner";
import type { SetlistRecord, SongRecord } from "../../lib/types";
import { songCount } from "./songUtil";
import type { SetlistEntry, SongDraft } from "./songUtil";
import { AddSongs } from "./AddSongs";
import { SongRow } from "./SongRow";
import { SongsPage } from "./SongsPage";

/** What an unresolved entry renders as: it is on the unit's setlist, but its song
 *  slot names no song in the list the app holds. Says so, rather than guessing. */
const unknownSong = (songSlot: number): SongRecord => ({
  slot: songSlot,
  name: "Unknown song",
  notes:
    "Not found on your Tone Master Pro. Reconnect it, or remove this song.",
  bpm: 0,
  bpm_active: false,
});

interface SetlistRowProps {
  song: SongRecord;
  unknown: boolean;
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
  unknown,
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
  const [open, setOpen] = useState(false);
  const pick = (fn: () => void) => () => {
    setOpen(false);
    fn();
  };
  const moves = onMoveUp != null || onMoveDown != null;
  return (
    <SongRow
      song={song}
      unknown={unknown}
      idx={idx}
      selected={open && !busy}
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
        // The drag handle. Not a button: dragging is its only use, and the ⋯ menu
        // is the click path for the same moves.
        <span
          role="img"
          aria-label={`Move ${song.name}`}
          style={{
            width: 22,
            height: 22,
            flexShrink: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            cursor: busy ? "default" : "grab",
          }}
        >
          <Icon
            name="grip"
            size={12}
            stroke={busy ? t.textDisabled : t.textSecondary}
          />
        </span>
      }
      trailing={
        <MoreMenu
          label={`More for ${song.name}`}
          open={open}
          onOpenChange={setOpen}
          disabled={busy}
        >
          {onMoveUp && <MenuItem label="Move up" onClick={pick(onMoveUp)} />}
          {onMoveDown && (
            <MenuItem label="Move down" onClick={pick(onMoveDown)} />
          )}
          {moves && <MenuDivider />}
          <MenuItem label="Remove from setlist" onClick={pick(onRemove)} />
        </MoreMenu>
      }
    />
  );
}

export interface SetlistDetailProps {
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
  const [picker, setPicker] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [menu, setMenu] = useState(false);
  const dragFrom = useRef<SetlistEntry | null>(null);

  // Close the rename editor when a different setlist (or a renamed one) is shown —
  // via React's "adjust state during render when a prop changes" pattern. The
  // editor owns its draft, so the next rename starts from the new name. A different
  // setlist also closes the Add songs picker (a click outside it leaves it open, so a
  // click on another setlist would otherwise carry its ticks over) and the ⋯ menu; a
  // rename of this one leaves the picker as it is.
  const [prevId, setPrevId] = useState({
    slot: setlist.slot,
    name: setlist.name,
  });
  if (prevId.slot !== setlist.slot || prevId.name !== setlist.name) {
    if (prevId.slot !== setlist.slot) {
      setPicker(false);
      setMenu(false);
    }
    setPrevId({ slot: setlist.slot, name: setlist.name });
    setRenaming(false);
  }

  const rows = entries ?? [];
  const empty = entries != null && rows.length === 0;

  /** "Add songs" and the picker it opens, in one anchor box (the picker lines up
   *  under the button, right edges flush). The empty page carries its own. */
  const addSongs = (variant: "primary" | "secondary") => (
    <span style={{ position: "relative", display: "flex" }}>
      <Button
        variant={variant}
        icon="plus"
        disabled={busy}
        onClick={() => {
          setPicker((o) => !o);
        }}
      >
        Add songs
      </Button>
      {picker && (
        <AddSongs
          setlistName={setlist.name}
          available={available}
          onAdd={onAdd}
          onCreateAndAdd={onCreateAndAdd}
          onClose={() => {
            setPicker(false);
          }}
        />
      )}
    </span>
  );

  const title = renaming ? (
    <InlineEdit
      initial={setlist.name}
      ariaLabel="Setlist name"
      // The ✓ / ✕ beside the field are its click path, so a blur (onto ✕) must not
      // save.
      commitOnBlur={false}
      commitLabel="Save name"
      onCommit={(n) => {
        if (n && n !== setlist.name) onRename(n);
        setRenaming(false);
      }}
      onCancel={() => {
        setRenaming(false);
      }}
      wrapStyle={{
        display: "flex",
        alignItems: "center",
        gap: t.sp8,
        maxWidth: 420,
      }}
      style={{
        flex: 1,
        minWidth: 0,
        height: t.lineTitle + 2,
        boxSizing: "border-box",
        padding: `0 ${String(t.sp6)}px`,
        border: `1px solid ${t.controlBorder}`,
        borderRadius: t.radiusControl,
        background: t.surface,
        // The field sits in PageHead's title: it takes the title's own type and
        // tracking, so the name doesn't move when the rename opens.
        font: "inherit",
        letterSpacing: "inherit",
      }}
    />
  ) : (
    <button
      type="button"
      onClick={() => {
        setRenaming(true);
      }}
      style={plainButton({
        display: "block",
        maxWidth: "100%",
        textAlign: "left",
        cursor: "text",
        whiteSpace: "nowrap",
        overflow: "hidden",
        textOverflow: "ellipsis",
      })}
    >
      {setlist.name}
    </button>
  );

  return (
    <SongsPage
      busy={busy}
      title={title}
      // Hidden while the setlist is read, and on an empty one (its page says it).
      subtitle={entries != null && !empty ? songCount(rows.length) : undefined}
      actions={
        <>
          {!empty && addSongs("secondary")}
          <MoreMenu
            label="Setlist options"
            size="page"
            open={menu}
            onOpenChange={setMenu}
            disabled={busy}
          >
            <MenuItem
              label="Rename setlist"
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
          </MoreMenu>
        </>
      }
    >
      {entries == null ? (
        <Group>
          <div
            role="status"
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              gap: t.sp8,
              padding: `${String(t.sp24)}px ${String(t.sp16)}px`,
              fontSize: t.fontBody,
              color: t.textSecondary,
            }}
          >
            <Spinner size={12} strokeWidth={2.2} />
            Reading this setlist…
          </div>
        </Group>
      ) : empty ? (
        <Group>
          <EmptyState
            icon="music"
            title="No songs in this setlist yet"
            cta={addSongs("primary")}
          />
        </Group>
      ) : (
        <Group label={setlist.name}>
          {rows.map((entry, i) => {
            const above = i > 0 ? rows[i - 1] : undefined;
            const below = i + 1 < rows.length ? rows[i + 1] : undefined;
            return (
              <SetlistRow
                key={entry.position}
                song={entry.song ?? unknownSong(entry.songSlot)}
                unknown={entry.song == null}
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
        </Group>
      )}
    </SongsPage>
  );
}
