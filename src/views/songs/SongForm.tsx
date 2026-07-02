// src/views/songs/SongForm.tsx — inline create / edit form: name + optional notes + BPM.
import { useState } from "react";
import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { plainInput } from "../../theme/tokens";
import type { SongDraft } from "./shared";

interface SongFormProps {
  initial?: SongDraft;
  onSave: (d: SongDraft) => void;
  onCancel: () => void;
}

export function SongForm({ initial, onSave, onCancel }: SongFormProps) {
  const { t } = useTheme();
  const [name, setName] = useState(initial ? initial.name : "");
  const [notes, setNotes] = useState(initial?.notes ?? "");
  const [bpm, setBpm] = useState(
    initial?.bpm != null ? String(initial.bpm) : "",
  );

  const save = () => {
    const n = name.trim();
    if (!n) return;
    const b =
      bpm.trim() === ""
        ? null
        : Math.max(20, Math.min(400, parseInt(bpm, 10) || 0));
    onSave({ name: n, notes: notes.trim(), bpm: b });
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") save();
    if (e.key === "Escape") onCancel();
  };

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 8,
        border: `0.5px solid ${t.accent}`,
        borderRadius: t.rLg,
        padding: "9px 9px 9px 12px",
        background: t.bg,
        boxShadow: "0 1px 0 rgba(217,119,87,0.08)",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <input
          autoFocus
          value={name}
          onChange={(e) => {
            setName(e.target.value);
          }}
          onKeyDown={onKey}
          placeholder="Song name"
          style={plainInput(t, {
            flex: 1,
            minWidth: 0,
            fontFamily: t.serif,
            fontSize: t.fsName,
          })}
        />
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 5,
            borderLeft: `0.5px solid ${t.hairline}`,
            paddingLeft: 11,
            marginLeft: 3,
          }}
        >
          <input
            value={bpm}
            onChange={(e) => {
              setBpm(e.target.value.replace(/[^0-9]/g, ""));
            }}
            onKeyDown={onKey}
            placeholder="—"
            inputMode="numeric"
            style={plainInput(t, {
              width: 40,
              fontFamily: t.mono,
              fontSize: t.fsControl,
              textAlign: "right",
            })}
          />
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMicro2,
              letterSpacing: t.lsWide,
              color: t.faint,
            }}
          >
            BPM
          </span>
        </div>
        <span
          role="button"
          aria-label="Save"
          onClick={save}
          title="Save"
          style={{ cursor: "pointer", display: "flex", padding: 3 }}
        >
          <Icon name="check" size={15} stroke={t.accentDeep} />
        </span>
        <span
          role="button"
          aria-label="Cancel"
          onClick={onCancel}
          title="Cancel"
          style={{ cursor: "pointer", display: "flex", padding: 3 }}
        >
          <Icon name="x" size={13} stroke={t.faint} />
        </span>
      </div>
      <input
        value={notes}
        onChange={(e) => {
          setNotes(e.target.value);
        }}
        onKeyDown={onKey}
        placeholder="Notes (optional) — e.g. capo 2 · lead at 2nd chorus"
        style={plainInput(t, {
          borderTop: `0.5px solid ${t.hairline}`,
          paddingTop: 8,
          fontFamily: t.sans,
          fontSize: t.fsControl,
          color: t.ink2,
        })}
      />
    </div>
  );
}
