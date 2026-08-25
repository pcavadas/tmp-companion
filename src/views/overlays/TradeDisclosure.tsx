// src/views/overlays/TradeDisclosure.tsx — the headroom-trade disclosure block
// (D4/D5). When a leveling batch traded headroom (raised the base amp / presetLevel
// to buy a clamped sibling more room), EVERY row of that batch carries the SAME
// `TradeSummary` (`LevelResult.trade`) — this renders it ONCE per traded preset:
// the presetLevel move, each base-amp knob move, and which sounds the raise was
// bought for. Never hidden: `applied:false` is a PREVIEW that did NOT execute the
// trade ("would trade…"); a clamped row still shows its own clamp message alongside
// this (see `CLAMP_MESSAGES`). One compact block — not a modal.

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { signedDb } from "../../lib/format";
import { paramLabel, type RunItem } from "../level/leveling";
import type { TradeSummary, SoundId } from "../../lib/types";

/** A linear amplitude value (`presetLevel` / `outputLevel`), never a dB delta —
 *  three decimals matches the wire's own working precision. */
const fmtLinear = (v: number): string => v.toFixed(3);

/** Resolve a `benefiting` sound id to its display name off the batch's own sibling
 *  RunItems (same slot). A scene id with no matching sibling (shouldn't happen — the
 *  trade and the row it benefited come from the SAME run) falls back to a positional
 *  "Scene N" label rather than dropping the name silently. */
function soundName(id: SoundId, siblings: RunItem[]): string {
  const match = siblings.find(
    (it) =>
      !it.isBase && it.footswitch == null && it.sceneSlot === id.sceneSlot,
  );
  return match ? match.sceneName : `Scene ${String(id.sceneSlot + 1)}`;
}

export interface TradeDisclosureProps {
  trade: TradeSummary;
  /** This trade's own preset slot. */
  slot: number;
  /** Every run item from the SAME run (any slot) — filtered here to `slot` to resolve
   *  `benefiting` scene ids to their display names. */
  items: RunItem[];
}

export function TradeDisclosure({ trade, slot, items }: TradeDisclosureProps) {
  const { t } = useTheme();
  const siblings = items.filter((it) => it.slot === slot);
  const names = trade.benefiting.map((id) => soundName(id, siblings));
  const capNote =
    trade.cap === "preset_level_max"
      ? "capped at the preset level’s maximum"
      : trade.cap === "base_fader_floor"
        ? "capped by the base amp’s floor"
        : null;
  return (
    <div
      style={{
        display: "flex",
        gap: t.space5,
        padding: `${String(t.space5)}px ${String(t.space6)}px`,
        borderRadius: 9,
        background: t.bgAlt,
        border: `0.5px solid ${t.hairlineStrong}`,
      }}
    >
      <span style={{ flexShrink: 0, paddingTop: t.space1 }}>
        <Icon name="gauge" size={15} stroke={t.accentDeep} strokeWidth={1.7} />
      </span>
      <div
        style={{
          minWidth: 0,
          display: "flex",
          flexDirection: "column",
          gap: t.space2,
        }}
      >
        <div
          style={{
            fontFamily: t.sans,
            fontSize: 12.5,
            fontWeight: 500,
            color: t.ink,
          }}
        >
          {trade.applied ? "Headroom traded" : "Would trade headroom"} — preset
          level {signedDb(trade.raise_db)} (
          {fmtLinear(trade.previous_preset_level)}
          {" → "}
          {fmtLinear(trade.preset_level)})
        </div>
        {trade.base_amps.map((a) => (
          <div
            key={`${a.group_id}:${a.node_id}:${a.parameter_id}`}
            style={{ fontFamily: t.mono, fontSize: 10.5, color: t.mutedInk }}
          >
            {paramLabel(a.parameter_id)} {fmtLinear(a.previous_value)}
            {" → "}
            {a.value != null ? fmtLinear(a.value) : "?"}
          </div>
        ))}
        <div style={{ fontFamily: t.sans, fontSize: 11.5, color: t.mutedInk }}>
          for {names.join(", ")}
          {capNote ? ` — ${capNote}` : ""}
        </div>
      </div>
    </div>
  );
}

export default TradeDisclosure;
