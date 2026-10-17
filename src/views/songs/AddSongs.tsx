// src/views/songs/AddSongs.tsx — the Add songs picker (look B): a 320-wide popover under
// "Add songs". At the top, "Add songs to {setlist}" and a filter field; in the middle a
// scrolling checklist of the songs not yet in the setlist (a click on the box or the
// name ticks it; ticked rows are grey); at the foot, "New song" on the left and
// "Cancel" / "Add {n}" on the right. It stays open until Add or Cancel: a click
// outside does nothing, so a half-made choice is never thrown away by a stray click.
// "New song" turns the foot into the song form, and saving it creates the song and
// adds it to this setlist in one go.

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Button, Checkbox, SearchInput } from "../../ui/primitives";
import { Menu } from "../../ui/Menu";
import { Row } from "../../ui/List";
import type { SongRecord } from "../../lib/types";
import { tempoLabel } from "./songUtil";
import type { SongDraft } from "./songUtil";
import { SongForm } from "./SongForm";

/** The picker's outside-press handler: it stays open (only Add, Cancel or the
 *  "Add songs" button close it). */
function stayOpen(): void {
  // Deliberately nothing.
}

export interface AddSongsProps {
  /** The setlist the songs go into (named in the picker's title). */
  setlistName: string;
  /** Songs not yet in the setlist; null while its membership is still unknown — then
   *  nothing can be picked, since a song already in it can't be told apart. */
  available: SongRecord[] | null;
  onAdd: (songs: SongRecord[]) => void;
  onCreateAndAdd: (d: SongDraft) => void;
  onClose: () => void;
}

export function AddSongs({
  setlistName,
  available,
  onAdd,
  onCreateAndAdd,
  onClose,
}: AddSongsProps) {
  const { t } = useTheme();
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<Set<number>>(() => new Set());
  const [creating, setCreating] = useState(false);
  const filtered = (available ?? []).filter((s) =>
    s.name.toLowerCase().includes(q.trim().toLowerCase()),
  );
  const toggle = (slot: number) => {
    setSel((p) => {
      const n = new Set(p);
      if (n.has(slot)) n.delete(slot);
      else n.add(slot);
      return n;
    });
  };
  // What Add writes, and so what its label counts: the ticked songs still not in the
  // setlist (a tick on a song that has since joined it adds nothing).
  const picked = (available ?? []).filter((s) => sel.has(s.slot));
  const add = () => {
    if (picked.length) onAdd(picked);
    onClose();
  };
  const title = `Add songs to ${setlistName}`;

  return (
    <Menu
      role="dialog"
      label={title}
      surface="popover"
      gap={8}
      width={320}
      onClose={stayOpen}
    >
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          maxHeight: 420,
          fontFamily: t.fontSans,
        }}
      >
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: t.sp8,
            padding: `${String(t.sp12)}px ${String(t.sp12)}px ${String(t.sp8)}px`,
          }}
        >
          <span
            style={{
              fontSize: t.fontBody,
              lineHeight: `${String(t.lineBody)}px`,
              fontWeight: t.weightSemibold,
              color: t.text,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {title}
          </span>
          <SearchInput
            value={q}
            onChange={setQ}
            placeholder="Filter songs"
            autoFocus
          />
        </div>
        <div
          style={{
            flex: 1,
            minHeight: 0,
            overflowY: "auto",
            borderTop: `1px solid ${t.separator}`,
            borderBottom: `1px solid ${t.separator}`,
          }}
        >
          {/* 1 px up: the first row's top rule sits on the border above. */}
          <div style={{ marginTop: -1 }}>
            {filtered.map((rec) => {
              const on = sel.has(rec.slot);
              const tempo = tempoLabel(rec);
              return (
                <Row
                  key={rec.slot}
                  selected={on}
                  style={{ height: t.controlLarge - 1 }}
                >
                  {/* One <label>: a click on the name ticks the box too. */}
                  <label
                    style={{
                      flex: 1,
                      minWidth: 0,
                      alignSelf: "stretch",
                      display: "flex",
                      alignItems: "center",
                      gap: t.sp8 + 2,
                      cursor: "pointer",
                      userSelect: "none",
                    }}
                  >
                    <Checkbox
                      checked={on}
                      label={`Add ${rec.name}`}
                      labelHidden
                      onChange={() => {
                        toggle(rec.slot);
                      }}
                    />
                    <span
                      style={{
                        flex: 1,
                        minWidth: 0,
                        display: "flex",
                        alignItems: "baseline",
                        gap: t.sp8,
                        overflow: "hidden",
                      }}
                    >
                      <span
                        style={{
                          // Full width while the notes give way.
                          flexShrink: 0,
                          maxWidth: "100%",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {rec.name}
                      </span>
                      {rec.notes && (
                        <span
                          style={{
                            minWidth: 0,
                            fontSize: t.fontCaption,
                            color: t.textSecondary,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {rec.notes}
                        </span>
                      )}
                    </span>
                    {tempo != null && (
                      <span
                        style={{
                          flexShrink: 0,
                          fontSize: t.fontCaption,
                          color: t.textSecondary,
                          fontVariantNumeric: "tabular-nums",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {tempo}
                      </span>
                    )}
                  </label>
                </Row>
              );
            })}
          </div>
          {filtered.length === 0 && (
            <div
              style={{
                padding: `${String(t.sp24)}px ${String(t.sp12)}px`,
                textAlign: "center",
                fontSize: t.fontBody,
                lineHeight: `${String(t.lineBody)}px`,
                color: t.textSecondary,
              }}
            >
              {available == null
                ? "Reading this setlist…"
                : available.length === 0
                  ? "Every song is already in this setlist."
                  : "No songs match."}
            </div>
          )}
        </div>
        {creating ? (
          <SongForm
            onSave={(d) => {
              onCreateAndAdd(d);
              setCreating(false);
            }}
            onCancel={() => {
              setCreating(false);
            }}
          />
        ) : (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: t.sp8,
              padding: `${String(t.sp8 + 2)}px ${String(t.sp12)}px`,
            }}
          >
            <Button
              variant="plain"
              size="sm"
              icon="plus"
              onClick={() => {
                setCreating(true);
              }}
            >
              New song
            </Button>
            <span style={{ flex: 1 }} />
            <Button variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={picked.length === 0}
              onClick={add}
            >
              {picked.length > 0 ? `Add ${String(picked.length)}` : "Add"}
            </Button>
          </div>
        )}
      </div>
    </Menu>
  );
}
