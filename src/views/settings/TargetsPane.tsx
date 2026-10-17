// src/views/settings/TargetsPane.tsx — the "Loudness targets" page (look B): "Add
// target" in the page head, one Group with a TargetRow per user-owned target (rename,
// reorder by ⋯ Move up / Move down or drag, delete, and a slider with NO upper
// ceiling), and the helper line under it. The list operations live in `useTargets`
// (owned by SettingsView so a fresh row's rename mode survives a page switch). Nothing
// that edits the list is offered before the store is read: a save replaces the WHOLE
// list, so an add against the unread empty list would overwrite the stored one.

import { useRef } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { EmptyState } from "../../ui/EmptyState";
import { Group } from "../../ui/List";
import { Button } from "../../ui/primitives";
import { SettingsPage } from "./SettingsPage";
import { StoreNotices } from "./StoreNotices";
import { TargetRow } from "./TargetRow";
import type { SettingsStore } from "./useSettingsStore";
import type { TargetOps } from "./useTargets";

export interface TargetsPaneProps {
  store: Pick<
    SettingsStore,
    "load" | "ready" | "saveError" | "retryLoad" | "targets"
  >;
  ops: TargetOps;
}

export function TargetsPane({ store, ops }: TargetsPaneProps) {
  const { t } = useTheme();
  const { ready, targets } = store;
  // The drag source's uid while a row is grabbed.
  const dragUid = useRef<string | null>(null);
  return (
    <SettingsPage
      title="Loudness targets"
      actions={
        <Button
          variant="secondary"
          icon="plus"
          disabled={!ready}
          onClick={ops.addTarget}
        >
          Add target
        </Button>
      }
    >
      <StoreNotices store={store} />
      {ready && (
        <>
          <Group label="Loudness targets">
            {targets.length === 0 ? (
              <EmptyState
                icon="sliders"
                title="No targets yet"
                body="Leveling needs at least one target."
              />
            ) : (
              targets.map((row, i) => (
                <TargetRow
                  key={row.uid}
                  name={row.name}
                  lufs={row.lufs}
                  defaultEditing={row.uid === ops.justAdded}
                  onRename={(name) => {
                    ops.renameTarget(row.uid, name);
                  }}
                  onChange={(v) => {
                    ops.setTargetLevelLocal(row.uid, v);
                  }}
                  onCommit={(v) => {
                    ops.commitTargetLevel(row.uid, v);
                  }}
                  onDelete={() => {
                    ops.deleteTarget(row.uid);
                  }}
                  onMoveUp={
                    i > 0
                      ? () => {
                          ops.moveTarget(row.uid, -1);
                        }
                      : undefined
                  }
                  onMoveDown={
                    i + 1 < targets.length
                      ? () => {
                          ops.moveTarget(row.uid, 1);
                        }
                      : undefined
                  }
                  onGrab={() => {
                    dragUid.current = row.uid;
                  }}
                  onDropOn={() => {
                    ops.reorderTargets(dragUid.current, row.uid);
                    dragUid.current = null;
                  }}
                  onDragEnd={() => {
                    dragUid.current = null;
                  }}
                />
              ))
            )}
          </Group>
          {targets.length > 0 && (
            <span
              style={{
                fontSize: t.fontCaption,
                lineHeight: `${String(t.lineCaption)}px`,
                color: t.textSecondary,
              }}
            >
              Sounds named like a target get it on Set up.
            </span>
          )}
        </>
      )}
    </SettingsPage>
  );
}
