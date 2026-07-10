// src/views/settings/InstrumentForm.tsx — inline add/edit card: border-only serif
// name input + Type chips + Pickup chips + Cancel/Save. Builds a topology_id from
// the picked chips. Split out of SettingsView.tsx (mechanical extraction).

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { plainInput, type ThemeTokens } from "../../theme/tokens";
import { Button } from "../../ui/primitives";
import type { Profile, TopologyInfo } from "../../lib/types";

// ===========================================================================
// InstrumentForm
// ===========================================================================

function chipStyle(t: ThemeTokens, on: boolean) {
  return {
    fontFamily: t.mono,
    fontSize: t.fsData2,
    letterSpacing: t.lsMeta,
    color: on ? t.onInk : t.ink2,
    background: on ? t.accent : "transparent",
    border: `0.5px solid ${on ? t.accent : t.hairlineStrong}`,
    borderRadius: t.rPill,
    padding: "3px 11px",
    cursor: "pointer",
    userSelect: "none" as const,
    whiteSpace: "nowrap" as const,
  };
}

interface InstrumentFormProps {
  initial?: Profile;
  types: string[];
  topologies: TopologyInfo[];
  onSave: (data: { name: string; topology_id: string }) => void;
  onCancel: () => void;
}

export function InstrumentForm({
  initial,
  types,
  topologies,
  onSave,
  onCancel,
}: InstrumentFormProps) {
  const { t } = useTheme();

  const initialTopo = initial
    ? (topologies.find((tp) => tp.id === initial.topology_id) ?? null)
    : null;
  const [name, setName] = useState(initial ? initial.name : "");
  const firstType: string | undefined = types.length > 0 ? types[0] : undefined;
  const [type, setType] = useState<string>(
    initialTopo?.instrument ?? firstType ?? "",
  );
  const [topologyId, setTopologyId] = useState<string>(
    initial?.topology_id ?? "",
  );

  // Pickups available for the selected type (chip label = topology.label).
  const pickups = topologies.filter((tp) => tp.instrument === type);

  // Pick a sensible default pickup when none is chosen / the type changes.
  function pickType(next: string) {
    setType(next);
    const stillValid = topologies.some(
      (tp) => tp.id === topologyId && tp.instrument === next,
    );
    if (!stillValid) {
      const first = topologies.find((tp) => tp.instrument === next);
      setTopologyId(first ? first.id : "");
    }
  }

  // Ensure topologyId is set for a fresh add once topologies/type resolve.
  const effectiveTopologyId = topologyId || pickups[0]?.id || "";
  const canSave = name.trim() !== "" && effectiveTopologyId !== "";

  function save() {
    const n = name.trim();
    if (!n || !effectiveTopologyId) return;
    onSave({ name: n, topology_id: effectiveTopologyId });
  }

  const fieldLab = (label: string) => (
    <div
      style={{
        fontFamily: t.mono,
        fontSize: t.fsMicro2,
        letterSpacing: t.lsWide,
        color: t.faint,
        textTransform: "uppercase",
      }}
    >
      {label}
    </div>
  );

  return (
    <div
      style={{
        border: `0.5px solid ${t.accent}`,
        borderRadius: t.rLg,
        padding: "12px 13px",
        marginBottom: 8,
        background: t.bg,
        display: "flex",
        flexDirection: "column",
        gap: 11,
      }}
    >
      <input
        autoFocus
        value={name}
        onChange={(e) => {
          setName(e.target.value);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") save();
          if (e.key === "Escape") onCancel();
        }}
        onFocus={(e) => (e.currentTarget.style.borderColor = t.accent)}
        onBlur={(e) => (e.currentTarget.style.borderColor = t.hairlineStrong)}
        placeholder="Instrument name (e.g. Telecaster)"
        style={plainInput(t, {
          border: `0.5px solid ${t.hairlineStrong}`,
          borderRadius: t.rMd,
          padding: "7px 9px",
          fontFamily: t.serif,
          fontSize: t.fsName,
        })}
      />

      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {fieldLab("Type")}
        <div style={{ display: "flex", gap: 5, flexWrap: "wrap" }}>
          {types.map((ty) => (
            <span
              key={ty}
              onClick={() => {
                pickType(ty);
              }}
              style={chipStyle(t, type === ty)}
            >
              {ty}
            </span>
          ))}
        </div>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {fieldLab("Pickup")}
        <div style={{ display: "flex", gap: 5, flexWrap: "wrap" }}>
          {pickups.map((tp) => (
            <span
              key={tp.id}
              onClick={() => {
                setTopologyId(tp.id);
              }}
              style={chipStyle(t, effectiveTopologyId === tp.id)}
            >
              {tp.label}
            </span>
          ))}
        </div>
      </div>

      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
        <Button variant="ghost" small onClick={onCancel}>
          Cancel
        </Button>
        <Button variant="primary" small onClick={save} disabled={!canSave}>
          {initial ? "Save" : "Add instrument"}
        </Button>
      </div>
    </div>
  );
}
