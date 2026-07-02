// src/views/copy/BlockEditor.tsx — the inline block editor.
//
// Opens INSIDE a target card (replacing the hint line) when a target tile is tapped.
// Header: the tapped block's art + name, a Remove action, and a close ×. Body: a 3-way
// segmented control (Replace · Insert before · Insert after, default Replace) over a
// wrapping row of ORIGIN chips — one per distinct block in the reference preset. Tapping
// a chip applies the current mode with that model and closes the editor. There is NO
// auto-matching: the user always chooses both the block and the placement.

import { useState } from "react";
import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { BlockArt } from "../../ui/BlockArt";
import { SegmentedControl } from "../../ui/primitives";
import { resolveBlockArt, shortFallback } from "../../models/blockArt";
import { cpuStr } from "../../models/cpu";
import { type EditBlock, type OriginBlock } from "./copyModel";
import { checkOp, REASON_COPY, type BaseCounts } from "./validateBlockEdit";

export type EditMode = "replace" | "before" | "after";

export interface BlockEditorProps {
  block: EditBlock;
  /** The target preset's cap standing BEFORE this op — combined with the current
   *  `mode` (below, local state) to grey out any candidate that would violate a
   *  firmware cap. `block` doubles as the anchor (its model + dual-cab flag are
   *  what a `replace` would free). */
  counts: BaseCounts;
  /** The reference preset's name (chip-row preamble). */
  fromName: string;
  origin: OriginBlock[];
  onRemove: () => void;
  onApply: (mode: EditMode, model: string) => void;
  onClose: () => void;
}

export function BlockEditor({
  block,
  counts,
  fromName,
  origin,
  onRemove,
  onApply,
  onClose,
}: BlockEditorProps) {
  const { t } = useTheme();
  const [mode, setMode] = useState<EditMode>("replace");
  const headArt = resolveBlockArt(block.model);
  const headName = headArt?.name ?? shortFallback(block.model);
  const anchor = { model: block.model, dualCab: block.cabSim2Enabled };

  return (
    <div
      style={{
        marginTop: 8,
        border: `1px solid ${t.accent}`,
        borderRadius: t.rPopover,
        background: t.bgAlt,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "10px 13px",
          borderBottom: `0.5px solid ${t.hairline}`,
        }}
      >
        <BlockArt
          icon={headArt?.icon}
          tone={headArt?.tone}
          lab={headArt?.short}
          footswitch={headArt?.footswitch}
          bodyColor={headArt?.body}
          accentColor={headArt?.accent}
          panelColor={headArt?.panel}
          size={26}
          label={false}
        />
        <span style={{ fontFamily: t.serif, fontSize: t.fsName, color: t.ink }}>
          {headName}
        </span>
        <span style={{ flex: 1 }} />
        <span
          role="button"
          onClick={onRemove}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            fontFamily: t.sans,
            fontSize: t.fsUi,
            color: t.warn,
            border: `0.5px solid ${t.warnBorder}`,
            background: t.warnSoft,
            borderRadius: t.rMd,
            padding: "6px 11px",
            cursor: "pointer",
          }}
        >
          <Icon name="trash" size={13} stroke={t.warn} />
          Remove
        </span>
        <span
          role="button"
          onClick={onClose}
          style={{ display: "inline-flex", cursor: "pointer", padding: 3 }}
        >
          <Icon name="x" size={15} stroke={t.faint} />
        </span>
      </div>
      <div style={{ padding: "11px 13px" }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 9,
            marginBottom: 10,
            flexWrap: "wrap",
          }}
        >
          <span
            style={{
              fontFamily: t.sans,
              fontSize: t.fsBody2,
              color: t.mutedInk,
            }}
          >
            Use a block from{" "}
            <strong style={{ color: t.ink2 }}>{fromName}</strong>:
          </span>
          <SegmentedControl
            variant="filled"
            ariaLabel="Edit mode"
            value={mode}
            onChange={setMode}
            options={[
              { value: "replace", label: "Replace" },
              { value: "before", label: "Insert before" },
              { value: "after", label: "Insert after" },
            ]}
          />
        </div>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
          {origin.map((o) => {
            // Mode-aware pre-flight: would placing `o.model` via the CURRENT mode
            // violate a firmware cap? UX only — greys out + explains, doesn't
            // enforce (the Rust `copy_apply` guard is the real checkpoint).
            const reason = checkOp(counts, o.model, mode, { anchor });
            const blocked = reason != null;
            return (
              <span
                key={o.model}
                role="button"
                aria-disabled={blocked}
                // e2e hook: pick a candidate by its MODEL id (matches the tile's
                // `data-block-tile` model exactly; the display label differs between them).
                data-candidate={o.model}
                title={blocked ? REASON_COPY[reason] : undefined}
                onClick={
                  blocked
                    ? undefined
                    : () => {
                        onApply(mode, o.model);
                      }
                }
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "6px 11px 6px 7px",
                  borderRadius: t.rPill,
                  border: `0.5px solid ${t.hairlineStrong}`,
                  background: t.bg,
                  cursor: blocked ? "not-allowed" : "pointer",
                  opacity: blocked ? 0.4 : 1,
                }}
              >
                <BlockArt
                  icon={o.icon}
                  tone={o.tone}
                  lab={o.lab}
                  footswitch={o.footswitch}
                  bodyColor={o.body}
                  accentColor={o.accent}
                  panelColor={o.panel}
                  size={22}
                  label={false}
                />
                <span
                  style={{
                    fontFamily: t.serif,
                    fontSize: t.fsName2,
                    color: t.ink,
                    whiteSpace: "nowrap",
                  }}
                >
                  {o.name}
                </span>
                <span
                  style={{
                    fontFamily: t.mono,
                    fontSize: t.fsMicro,
                    color: t.faint,
                  }}
                >
                  {o.cpu == null ? "" : `+${cpuStr(o.cpu, "")}`}
                </span>
              </span>
            );
          })}
        </div>
      </div>
    </div>
  );
}

export default BlockEditor;
