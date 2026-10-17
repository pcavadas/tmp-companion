// src/views/settings/InstrumentsPane.tsx — the "Instruments" page (look B): "Add
// instrument" in the page head, one helper line, a Notice while the unit is away
// (recording listens through its input; while a save has failed, its sentence joins
// the failed-save Notice instead, so the page shows one Notice), then one Group with an
// InstrumentRow per profile — or its edit form in place — and the add form as a card
// under the list. The caller keeps it MOUNTED and toggles `visible`, so a page switch
// mid-recording doesn't unmount an InstrumentRow and drop an in-flight ~8 s device
// capture.

import { useMemo } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { EmptyState } from "../../ui/EmptyState";
import { Group } from "../../ui/List";
import { Button, Notice } from "../../ui/primitives";
import type { TopologyInfo } from "../../lib/types";
import { InstrumentForm } from "./InstrumentForm";
import { InstrumentRow } from "./InstrumentRow";
import { SettingsPage } from "./SettingsPage";
import { StoreNotices } from "./StoreNotices";
import { useProfiles } from "./useProfiles";
import type { SettingsStore } from "./useSettingsStore";

export interface InstrumentsPaneProps {
  /** Shown only while Instruments is the chosen page. */
  visible: boolean;
  connected: boolean;
  store: Pick<
    SettingsStore,
    | "load"
    | "ready"
    | "saveError"
    | "retryLoad"
    | "profiles"
    | "topologies"
    | "persistProfiles"
    | "refreshFromStore"
  >;
}

/** Why every Record button is off while the unit is away. */
const RECORDING_NEEDS_UNIT = "Recording needs your Tone Master Pro connected.";

export function InstrumentsPane({
  visible,
  connected,
  store,
}: InstrumentsPaneProps) {
  const { t } = useTheme();
  const { ready, profiles, topologies } = store;
  const ops = useProfiles({
    profiles,
    persistProfiles: store.persistProfiles,
  });

  // The instrument types (guitar / bass / bass-vi), in the order the backend lists them.
  const types = useMemo(() => {
    const seen: string[] = [];
    for (const tp of topologies)
      if (!seen.includes(tp.instrument)) seen.push(tp.instrument);
    return seen;
  }, [topologies]);

  // topology_id → its TopologyInfo, for each row's "{type} · {pickup}" caption.
  const topoById = useMemo(() => {
    const m = new Map<string, TopologyInfo>();
    for (const tp of topologies) m.set(tp.id, tp);
    return m;
  }, [topologies]);

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: visible ? "flex" : "none",
        flexDirection: "column",
      }}
    >
      <SettingsPage
        title="Instruments"
        actions={
          <Button
            variant="secondary"
            icon="plus"
            disabled={!ready || topologies.length === 0 || ops.adding}
            onClick={() => {
              ops.setEditingId(null);
              ops.setAdding(true);
            }}
          >
            Add instrument
          </Button>
        }
      >
        <StoreNotices
          store={store}
          alsoOnSaveError={
            ready && !connected ? RECORDING_NEEDS_UNIT : undefined
          }
        />
        {ready && (
          <>
            <span
              style={{
                fontSize: t.fontBody,
                lineHeight: `${String(t.lineBody)}px`,
                color: t.textSecondary,
              }}
            >
              Record each instrument once, about 8 seconds. Play the way you
              gig: chords and lead, pick and fingers.
            </span>
            {!connected && store.saveError == null && (
              <Notice>{RECORDING_NEEDS_UNIT}</Notice>
            )}
            {profiles.length === 0 ? (
              !ops.adding && (
                <Group>
                  <EmptyState
                    icon="cable"
                    title="No instruments yet"
                    body="Add one, then record it."
                  />
                </Group>
              )
            ) : (
              <Group label="Instruments">
                {profiles.map((p, i) =>
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
                        void store.refreshFromStore();
                      }}
                      onEdit={() => {
                        ops.setAdding(false);
                        ops.setEditingId(p.id);
                      }}
                      onDelete={() => {
                        ops.deleteProfile(p.id);
                      }}
                      onMoveUp={
                        i > 0
                          ? () => {
                              ops.moveProfile(p.id, -1);
                            }
                          : undefined
                      }
                      onMoveDown={
                        i + 1 < profiles.length
                          ? () => {
                              ops.moveProfile(p.id, 1);
                            }
                          : undefined
                      }
                    />
                  ),
                )}
              </Group>
            )}
            {ops.adding && (
              <InstrumentForm
                card
                types={types}
                topologies={topologies}
                onSave={ops.addInstrument}
                onCancel={() => {
                  ops.setAdding(false);
                }}
              />
            )}
          </>
        )}
      </SettingsPage>
    </div>
  );
}
