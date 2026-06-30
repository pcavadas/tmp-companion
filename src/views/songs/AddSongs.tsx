// src/views/songs/AddSongs.tsx — the "add songs from library" picker popover used by
// the setlist detail (membership is read from the device; ordered global song slots).
import { useState } from "react";
import { useTheme } from "../../theme/ThemeContext";
import { Button, Checkbox, SearchInput } from "../../ui/primitives";
import { Menu } from "../../ui/Menu";
import { DASH } from "../../lib/format";
import type { SongRecord } from "../../lib/types";
import { songBpm } from "./songUtil";
import type { SongDraft } from "./shared";
import { SongForm } from "./SongForm";

interface AddSongsProps {
  available: SongRecord[];
  onAdd: (songSlots: number[]) => void;
  onCreateAndAdd: (d: SongDraft) => void;
  onClose: () => void;
}

export function AddSongs({
  available,
  onAdd,
  onCreateAndAdd,
  onClose,
}: AddSongsProps) {
  const { t } = useTheme();
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<Set<number>>(() => new Set());
  const [creating, setCreating] = useState(false);
  const filtered = available.filter((s) =>
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
  const add = () => {
    if (sel.size) onAdd([...sel]);
    onClose();
  };

  return (
    <Menu surface="popover" gap={8} width={320} onClose={onClose}>
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          maxHeight: 420,
        }}
      >
        <div
          style={{
            padding: "12px 13px 10px",
            borderBottom: `0.5px solid ${t.hairline}`,
          }}
        >
          <div
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMicro,
              letterSpacing: t.lsWide,
              color: t.faint,
              textTransform: "uppercase",
              marginBottom: 9,
            }}
          >
            Add songs from library
          </div>
          <SearchInput
            value={q}
            onChange={setQ}
            placeholder="Filter songs…"
            autoFocus
          />
        </div>
        <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: 5 }}>
          {filtered.map((rec) => {
            const on = sel.has(rec.slot);
            const bpm = songBpm(rec);
            return (
              <div
                key={rec.slot}
                onClick={() => {
                  toggle(rec.slot);
                }}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  padding: "8px 9px",
                  borderRadius: t.rMd,
                  cursor: "pointer",
                  background: on ? t.accentSoft : "transparent",
                }}
                onMouseEnter={(e) => {
                  if (!on) e.currentTarget.style.background = t.hover;
                }}
                onMouseLeave={(e) => {
                  if (!on) e.currentTarget.style.background = "transparent";
                }}
              >
                <Checkbox checked={on} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div
                    style={{
                      fontFamily: t.serif,
                      fontSize: t.fsName2,
                      color: t.ink,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {rec.name}
                  </div>
                  {rec.notes && (
                    <div
                      style={{
                        fontFamily: t.sans,
                        fontSize: t.fsData,
                        color: t.mutedInk,
                        marginTop: 1,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {rec.notes}
                    </div>
                  )}
                </div>
                <span
                  style={{
                    fontFamily: t.mono,
                    fontSize: t.fsMeta,
                    color: bpm != null ? t.mutedInk : t.faint,
                  }}
                >
                  {bpm != null ? `${String(bpm)} bpm` : DASH}
                </span>
              </div>
            );
          })}
          {filtered.length === 0 && (
            <div
              style={{
                padding: "26px 10px",
                textAlign: "center",
                fontFamily: t.sans,
                fontSize: t.fsControl,
                color: t.faint,
                lineHeight: 1.5,
              }}
            >
              {available.length === 0
                ? "Every song is already in this setlist."
                : "No songs match."}
            </div>
          )}
        </div>
        <div style={{ borderTop: `0.5px solid ${t.hairline}`, padding: 9 }}>
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
                justifyContent: "space-between",
                gap: 8,
              }}
            >
              <Button
                variant="ghost"
                small
                icon="plus"
                onClick={() => {
                  setCreating(true);
                }}
              >
                New song
              </Button>
              <Button
                variant="primary"
                small
                icon="check"
                disabled={sel.size === 0}
                onClick={add}
              >
                Add {sel.size || ""}
              </Button>
            </div>
          )}
        </div>
      </div>
    </Menu>
  );
}
