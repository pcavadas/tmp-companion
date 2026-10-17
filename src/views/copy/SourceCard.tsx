// src/views/copy/SourceCard.tsx — the "Copy from" card at the top of Place blocks
// (look B).
//
// A white panel: "Copy from <name>" with its slot (and "Loaded" when it is the preset on
// your Tone Master Pro), the source strip, then one line. A click on a tile chooses that
// block to copy (the ring); a click on the chosen tile clears it. The line says what is
// chosen ("Greenbox 8 is ready to copy." and, when its cost is known, "Adds 4.2% CPU"
// under it), or "Choose a block to copy." while nothing is.

import { useCallback, useMemo } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { cpuForBid } from "../../models/cpu";
import { Badge } from "../../ui/Badge";
import { SlotLabel } from "../../ui/SlotLabel";
import { CopyPath } from "./CopyPath";
import {
  COPY_TEXT,
  addsCpu,
  blockName,
  copyFromName,
  readyToCopy,
} from "./copyText";
import { editGraphFromActive, findBlock } from "./copyModel";
import type { PickedBlock } from "./copyFlow";
import type { CopyPreset } from "./useCopyLibrary";

export interface SourceCardProps {
  from: CopyPreset;
  /** The block chosen to copy, or null. */
  pick: PickedBlock | null;
  /** Choose block `uid` (or clear it, when it is the chosen one). */
  onPick: (uid: string, model: string) => void;
}

export function SourceCard({ from, pick, onPick }: SourceCardProps) {
  const { t } = useTheme();
  const graph = useMemo(() => editGraphFromActive(from.graph), [from.graph]);
  const onTap = useCallback(
    (uid: string) => {
      const b = findBlock(graph, uid);
      if (b) onPick(uid, b.model);
    },
    [graph, onPick],
  );
  const cpu = pick ? cpuForBid(pick.model) : null;

  return (
    <section
      data-source-card=""
      aria-label={copyFromName(from.name)}
      style={{
        display: "flex",
        flexDirection: "column",
        gap: t.sp8 + t.sp2,
        padding: `${String(t.sp12 + t.sp2)}px ${String(t.sp16)}px`,
        background: t.surface,
        border: `1px solid ${t.border}`,
        borderRadius: t.radiusGroup,
        fontFamily: t.fontSans,
        color: t.text,
        flexShrink: 0,
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: t.sp8 }}>
        <span style={{ fontSize: t.fontBody, fontWeight: t.weightSemibold }}>
          {copyFromName(from.name)}
        </span>
        <SlotLabel index={from.slot} />
        {from.onUnit && (
          <span style={{ alignSelf: "center" }}>
            <Badge>{COPY_TEXT.loaded}</Badge>
          </span>
        )}
      </div>
      <CopyPath graph={graph} onTap={onTap} selectedUid={pick?.uid ?? null} />
      <div style={{ display: "flex", flexDirection: "column", gap: t.sp2 }}>
        <span
          style={{
            fontSize: t.fontBody,
            lineHeight: `${String(t.lineBody)}px`,
          }}
        >
          {pick ? readyToCopy(blockName(pick.model)) : COPY_TEXT.chooseBlock}
        </span>
        {cpu != null && (
          <span
            style={{
              fontSize: t.fontCaption,
              lineHeight: `${String(t.lineCaption)}px`,
              color: t.textSecondary,
            }}
          >
            {addsCpu(cpu)}
          </span>
        )}
      </div>
    </section>
  );
}
