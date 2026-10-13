// src/views/settings/TargetsPane.tsx — the "Loudness targets" pane: one TargetRow per
// user-owned target (create / rename / reorder via ⋯ Move up/down or drag / delete +
// a draggable slider with NO upper ceiling), plus "Add target". The list operations
// live in `useTargets` (owned by SettingsView so a fresh row's rename mode survives a
// category switch).

import { useRef } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { EmptyHint } from "./EmptyHint";
import { TargetRow } from "./TargetRow";
import type { UiTarget } from "./useSettingsStore";
import type { TargetOps } from "./useTargets";

export interface TargetsPaneProps {
  targets: UiTarget[];
  ops: TargetOps;
}

export function TargetsPane({ targets, ops }: TargetsPaneProps) {
  const { t } = useTheme();
  // The drag source's uid while a row is grabbed.
  const dragUid = useRef<string | null>(null);
  return (
    <div style={{ maxWidth: 560 }}>
      {targets.map((row, i) => (
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
      ))}

      {targets.length === 0 && (
        <EmptyHint>No targets yet — add one to start leveling.</EmptyHint>
      )}

      <Button
        variant="ghost"
        size="sm"
        icon="plus"
        onClick={ops.addTarget}
        style={{ marginTop: t.space6 }}
      >
        Add target
      </Button>
    </div>
  );
}
