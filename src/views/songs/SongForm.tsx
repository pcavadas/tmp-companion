// src/views/songs/SongForm.tsx — the create / edit song form (look B): "Song name" and
// "Tempo" side by side (name flexible, tempo 110 px), "Notes" under them, then
// right-aligned "Cancel" and "Save song". Save is disabled while the name is empty.
// Enter in a field saves and Escape cancels: the text-field carve-out, acting only on
// this form's own edit while one of its fields has focus.
//
// `card` draws it as a white card with a control edge (New song, above the list);
// without it, it is just the fields, for a caller that already gives it a surface (in
// place of a song's row, in the Add songs picker).

import { useState } from "react";
import type { KeyboardEvent } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Button, TextField } from "../../ui/primitives";
import type { SongDraft } from "./songUtil";

export interface SongFormProps {
  initial?: SongDraft;
  onSave: (d: SongDraft) => void;
  onCancel: () => void;
  card?: boolean;
}

export function SongForm({
  initial,
  onSave,
  onCancel,
  card = false,
}: SongFormProps) {
  const { t } = useTheme();
  const [name, setName] = useState(initial ? initial.name : "");
  const [notes, setNotes] = useState(initial?.notes ?? "");
  const [bpm, setBpm] = useState(
    initial?.bpm != null ? String(initial.bpm) : "",
  );
  const canSave = name.trim() !== "";

  const save = () => {
    const n = name.trim();
    if (!n) return;
    const b =
      bpm.trim() === ""
        ? null
        : Math.max(20, Math.min(400, parseInt(bpm, 10) || 0));
    onSave({ name: n, notes: notes.trim(), bpm: b });
  };
  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    // An IME's Enter confirms a candidate, not the form.
    if (e.nativeEvent.isComposing) return;
    if (e.key === "Enter") {
      e.preventDefault();
      save();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  };

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: t.sp8 + 2,
        padding: t.sp12 + 2,
        ...(card
          ? {
              background: t.surface,
              border: `1px solid ${t.controlBorder}`,
              borderRadius: t.radiusGroup,
            }
          : {}),
      }}
    >
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0, 1fr) 110px",
          gap: t.sp12,
        }}
      >
        <TextField
          label="Song name"
          autoFocus
          value={name}
          onChange={setName}
          onKeyDown={onKey}
        />
        <TextField
          label="Tempo"
          placeholder="Optional"
          inputMode="numeric"
          maxLength={3}
          value={bpm}
          onChange={(v) => {
            setBpm(v.replace(/[^0-9]/g, ""));
          }}
          onKeyDown={onKey}
        />
      </div>
      <TextField
        label="Notes"
        placeholder="Optional, for example: capo 2"
        value={notes}
        onChange={setNotes}
        onKeyDown={onKey}
      />
      <div style={{ display: "flex", justifyContent: "flex-end", gap: t.sp8 }}>
        <Button variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
        <Button variant="primary" disabled={!canSave} onClick={save}>
          Save song
        </Button>
      </div>
    </div>
  );
}
