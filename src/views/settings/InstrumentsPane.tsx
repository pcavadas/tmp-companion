// src/views/settings/InstrumentsPane.tsx — the "Instruments" pane: the
// connect-to-calibrate note, one InstrumentRow per profile (or its edit form), and the
// "Add instrument" form. The caller keeps it MOUNTED and toggles `visible`, so a rail
// switch mid-calibration doesn't unmount an InstrumentRow and discard an in-flight
// ~8 s device capture.

import { useMemo } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button } from "../../ui/primitives";
import type { TopologyInfo } from "../../lib/types";
import { EmptyHint } from "./EmptyHint";
import { InstrumentForm } from "./InstrumentForm";
import { InstrumentRow } from "./InstrumentRow";
import { useProfiles } from "./useProfiles";
import type { SettingsStore } from "./useSettingsStore";

export interface InstrumentsPaneProps {
  /** Shown only on the Instruments category once the store is ready. */
  visible: boolean;
  ready: boolean;
  connected: boolean;
  profiles: SettingsStore["profiles"];
  topologies: TopologyInfo[];
  persistProfiles: SettingsStore["persistProfiles"];
  /** Re-read the store after a calibration resolves. */
  onRefresh: SettingsStore["refreshFromStore"];
}

export function InstrumentsPane({
  visible,
  ready,
  connected,
  profiles,
  topologies,
  persistProfiles,
  onRefresh,
}: InstrumentsPaneProps) {
  const { t } = useTheme();
  const ops = useProfiles({ profiles, persistProfiles });

  // Distinct instrument types (e.g. Guitar / Bass / Acoustic), in first-seen order.
  const types = useMemo(() => {
    const seen: string[] = [];
    for (const tp of topologies)
      if (!seen.includes(tp.instrument)) seen.push(tp.instrument);
    return seen;
  }, [topologies]);

  // topology_id → its TopologyInfo, for the per-row "Type · Pickup" caption.
  const topoById = useMemo(() => {
    const m = new Map<string, TopologyInfo>();
    for (const tp of topologies) m.set(tp.id, tp);
    return m;
  }, [topologies]);

  return (
    <div
      style={{
        maxWidth: 620,
        display: visible ? "block" : "none",
      }}
    >
      {!connected && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: t.space4,
            padding: `${String(t.space4)}px ${String(t.space5)}px`,
            marginBottom: t.space6,
            borderRadius: t.rCard,
            border: `0.5px solid ${t.hairlineStrong}`,
            background: t.bgAlt,
          }}
        >
          <Icon name="cable" size={15} stroke={t.sevWarn} strokeWidth={1.5} />
          <span
            style={{
              fontFamily: t.sans,
              fontSize: t.fsUi,
              color: t.ink2,
              lineHeight: 1.45,
            }}
          >
            Your instruments and offsets are saved on this Mac.{" "}
            <strong style={{ color: t.ink }}>
              Calibrating needs the unit connected
            </strong>
            , because it listens through the device input.
          </span>
        </div>
      )}

      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsMeta,
          color: t.mutedInk,
          lineHeight: 1.45,
          marginBottom: t.space6,
        }}
      >
        Play the way you gig: mix chords and lead, pick and fingers. Include
        EBow if you use one. Record it all in one take.
      </div>

      {profiles.map((p) =>
        ops.editingId === p.id ? (
          <InstrumentForm
            key={p.id}
            initial={p}
            types={types}
            topologies={topologies}
            onSave={(d) => {
              ops.saveEdit(p.id, d);
            }}
            onCancel={() => {
              ops.setEditingId(null);
            }}
          />
        ) : (
          <InstrumentRow
            key={p.id}
            profile={p}
            topology={topoById.get(p.topology_id) ?? null}
            connected={connected}
            onCalibrated={() => {
              void onRefresh();
            }}
            onEdit={() => {
              ops.setEditingId(p.id);
            }}
            onDelete={() => {
              ops.deleteProfile(p.id);
            }}
            onMove={(dir) => {
              ops.moveProfile(p.id, dir);
            }}
          />
        ),
      )}

      {profiles.length === 0 && (
        <EmptyHint>No instruments yet — add one to calibrate.</EmptyHint>
      )}

      {ops.adding ? (
        <InstrumentForm
          types={types}
          topologies={topologies}
          onSave={ops.addInstrument}
          onCancel={() => {
            ops.setAdding(false);
          }}
        />
      ) : (
        <Button
          variant="ghost"
          size="sm"
          icon="plus"
          onClick={() => {
            ops.setAdding(true);
          }}
          disabled={!ready || topologies.length === 0}
          style={{ marginTop: t.space1 }}
        >
          Add instrument
        </Button>
      )}
    </div>
  );
}
