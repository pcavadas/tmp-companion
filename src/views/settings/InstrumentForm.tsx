// src/views/settings/InstrumentForm.tsx — the add / edit instrument form (look B): a
// "Name" TextField, then "Type" (Guitar / Bass / Bass VI, from the backend's pickup
// topologies) and "Pickup" (a Select of that type's pickups) side by side, then
// right-aligned "Cancel" and "Add instrument" / "Save instrument". Save waits for a name
// and a pickup. Enter in the name saves and Escape cancels: the text-field carve-out,
// acting only on this form's own edit while its field has focus.
//
// The Pickup dropdown always shows the pickup that will be saved (danger.md: a dropdown
// must display the value that will be used). A new instrument starts on its type's first pickup, and a type
// change moves to the new type's first pickup — both shown as chosen. A saved pickup
// the backend no longer offers is NOT replaced by another one behind the user's back:
// the dropdown is empty in the attention look, a note says why, and Save waits for a
// choice.
//
// `card` draws it as a white card with a control edge (a new instrument, under the
// list); without it, it is just the fields, for an edit in place of the row inside the
// Group.

import { useState } from "react";
import type { KeyboardEvent } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import { Icon } from "../../ui/Icon";
import { Select } from "../../ui/Select";
import { Button, SegmentedControl, TextField } from "../../ui/primitives";
import { instrumentTypeLabel } from "../../lib/instrumentTypes";
import type { Profile, TopologyInfo } from "../../lib/types";
import type { ProfileDraft } from "./useProfiles";

export interface InstrumentFormProps {
  initial?: Profile;
  /** The instrument types, in the order the backend lists them. */
  types: string[];
  topologies: TopologyInfo[];
  onSave: (data: ProfileDraft) => void;
  onCancel: () => void;
  card?: boolean;
}

/** The first pickup of `type`, or "" when it has none. */
function firstPickup(topologies: TopologyInfo[], type: string): string {
  return topologies.find((tp) => tp.instrument === type)?.id ?? "";
}

export function InstrumentForm({
  initial,
  types,
  topologies,
  onSave,
  onCancel,
  card = false,
}: InstrumentFormProps) {
  const { t } = useTheme();

  const initialTopo = initial
    ? (topologies.find((tp) => tp.id === initial.topology_id) ?? null)
    : null;
  // An edit whose saved pickup the backend no longer offers.
  const savedGone = initial != null && initialTopo == null;
  const startType =
    initialTopo?.instrument ?? (types.length > 0 ? types[0] : "");

  const [name, setName] = useState(initial ? initial.name : "");
  const [type, setType] = useState(startType);
  const [topologyId, setTopologyId] = useState<string>(
    initialTopo?.id ?? (savedGone ? "" : firstPickup(topologies, startType)),
  );

  const pickups = topologies.filter((tp) => tp.instrument === type);
  const canSave = name.trim() !== "" && topologyId !== "";

  function pickType(next: string) {
    setType(next);
    if (
      !topologies.some((tp) => tp.id === topologyId && tp.instrument === next)
    )
      setTopologyId(firstPickup(topologies, next));
  }

  function save() {
    const n = name.trim();
    if (!n || !topologyId) return;
    onSave({ name: n, topology_id: topologyId });
  }

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

  const field = {
    display: "flex",
    flexDirection: "column",
    gap: t.sp4,
  } as const;

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
          : { borderTop: `1px solid ${t.separator}` }),
      }}
    >
      <TextField
        label="Name"
        placeholder="For example, Telecaster"
        autoFocus
        value={name}
        onChange={setName}
        onKeyDown={onKey}
      />
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          alignItems: "flex-start",
          gap: t.sp12,
        }}
      >
        <div style={field}>
          <span style={sectionLabel(t)}>Type</span>
          <SegmentedControl
            ariaLabel="Type"
            value={type}
            onChange={pickType}
            options={types.map((ty) => ({
              value: ty,
              label: instrumentTypeLabel(ty),
            }))}
          />
        </div>
        <div style={{ ...field, minWidth: 0 }}>
          <span style={sectionLabel(t)}>Pickup</span>
          <Select
            label="Pickup"
            value={topologyId}
            onChange={setTopologyId}
            options={pickups.map((tp) => ({ value: tp.id, label: tp.label }))}
            width={200}
            tid="pickup"
          />
          {savedGone && topologyId === "" && (
            <span
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: t.sp6,
                fontSize: t.fontCaption,
                lineHeight: `${String(t.lineCaption)}px`,
                color: t.text,
              }}
            >
              <span style={{ display: "flex", paddingTop: 1.5, flexShrink: 0 }}>
                <Icon
                  name="warn-tri"
                  size={13}
                  stroke={t.caution}
                  strokeWidth={2}
                />
              </span>
              The saved pickup isn&rsquo;t offered any more. Choose one.
            </span>
          )}
        </div>
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: t.sp8 }}>
        <Button variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
        <Button variant="primary" disabled={!canSave} onClick={save}>
          {initial ? "Save instrument" : "Add instrument"}
        </Button>
      </div>
    </div>
  );
}
