// src/views/overlays/Pick.tsx — the leveling wizard's click-only dropdown.
//
// The menu PORTALS into the wizard card (via DialogCardCtx) and positions itself in the
// card's own coordinate space, flipping ABOVE the trigger when it would overflow the
// fixed-height frame — so a picker on a bottom row never clips past the card (the
// behaviour the design prototype specs). Falls back to a trigger-anchored inline menu
// when rendered outside a wizard card. The two menu deliveries are dedicated components
// (PickPortalMenu / PickInlineMenu); Pick owns the trigger, state, and the option rows.

import { useContext } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { DialogCardCtx } from "./wizardContext";
import { PickPortalMenu } from "./PickPortalMenu";
import { PickInlineMenu } from "./PickInlineMenu";
import { usePickAnchor } from "./usePickAnchor";
import { pickTriggerBorder } from "./pickTriggerChrome";

export interface PickOption {
  id: string;
  label: string;
  sub?: string;
  /** Instrument options only: true ⇒ this profile carries a stored calibration.
   *  Drives the Set up step's instrument nudge (the menu itself ignores it). */
  calibrated?: boolean;
}

export interface PickProps {
  value: string;
  options: PickOption[];
  onChange: (id: string) => void;
  grow?: boolean;
  title?: string;
  /** Render the trigger label faint — used for a per-scene picker that is FOLLOWING
   *  the apply-to-all value (not yet overridden). */
  muted?: boolean;
  /** e2e hook: stable `data-pick` selector on the trigger (e.g. `target:E2E P400`) so a
   *  test can open a specific row's picker without relying on portal layout. Each option
   *  row also carries `data-pick-option="<tid>:<option.id>"` once the menu is open — a
   *  text-based option match (`getByText(label,{exact:true})`) breaks once TWO rows are
   *  bound to option text that's ALSO the trigger's own closed-state label (e.g. two
   *  presets both already showing "Lead"); the attribute selector has no such collision
   *  and doesn't depend on DOM append order. */
  tid?: string;
  /** Fired once the menu has opened — a lazy per-preset fetch trigger (e.g. the
   *  footswitch scene-context picker's `list_footswitch_scene_contexts`). Idempotent,
   *  safe to call on every open. Omit for a Pick whose options need no device read. */
  onOpen?: () => void;
}

export function Pick({
  value,
  options,
  onChange,
  grow,
  title,
  muted,
  tid,
  onOpen,
}: PickProps) {
  const { t } = useTheme();
  const cardRef = useContext(DialogCardCtx);
  const { open, anchor, pos, cardEl, menuRef, triggerRef, openMenu, close } =
    usePickAnchor(cardRef, { onOpen });
  // `options[0]` is `PickOption | undefined` at runtime (the array can be empty), but
  // this tsconfig has no `noUncheckedIndexedAccess` — spell the real type out so the
  // empty-list fallback below isn't seen as redundant.
  const first: PickOption | undefined =
    options.length > 0 ? options[0] : undefined;
  const found = options.find((o) => o.id === value);
  // DANGER-rule guard (danger.md's Pick/BlockPick trap): a non-empty `value` the
  // current `options` set doesn't contain must NEVER silently fall back to
  // `options[0]` — a store without a "Crunch" target once displayed "Rhythm" while
  // the run submitted "crunch", a silent wrong-target write. Show the raw stored id
  // in a warning state instead: honest (states what's ACTUALLY selected), even
  // though it isn't as pretty as a resolved label.
  const stale = value !== "" && found === undefined;
  const cur = found ?? (stale ? undefined : first);

  const pick = (id: string) => {
    close();
    onChange(id);
  };

  const optionRows = options.map((o) => {
    const on = o.id === value;
    return (
      <div
        key={o.id}
        data-pick-option={tid ? `${tid}:${o.id}` : undefined}
        onClick={(e) => {
          e.stopPropagation();
          pick(o.id);
        }}
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space5,
          padding: `${String(t.space3)}px ${String(t.space4)}px`,
          borderRadius: 5,
          cursor: "pointer",
          background: on ? t.accentSoft : "transparent",
        }}
        onMouseEnter={(e) => {
          if (!on) e.currentTarget.style.background = t.hover;
        }}
        onMouseLeave={(e) => {
          if (!on) e.currentTarget.style.background = "transparent";
        }}
      >
        <span
          style={{
            fontFamily: t.mono,
            fontSize: 11,
            color: on ? t.accentDeep : t.ink2,
          }}
        >
          {o.label}
        </span>
        {o.sub && (
          <span
            style={{
              fontFamily: t.mono,
              fontSize: 9.5,
              color: t.faint,
              marginLeft: "auto",
            }}
          >
            {o.sub}
          </span>
        )}
      </div>
    );
  });

  return (
    <div
      ref={triggerRef}
      style={{
        position: "relative",
        width: grow ? "100%" : undefined,
        minWidth: grow ? 0 : undefined,
      }}
    >
      <div
        onClick={openMenu}
        title={stale ? `"${value}" is no longer offered` : title}
        data-pick={tid}
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space3,
          height: 26,
          padding: `0 ${String(t.space3)}px 0 ${String(t.space4)}px`,
          border: pickTriggerBorder(t, { open, warn: stale }),
          borderRadius: 6,
          background: t.bg,
          cursor: "pointer",
          whiteSpace: "nowrap",
        }}
      >
        {stale && (
          <Icon
            name="warn-tri"
            size={11}
            stroke={t.sevWarn}
            strokeWidth={1.7}
          />
        )}
        <span
          style={{
            fontFamily: t.mono,
            fontSize: 10.5,
            color: stale ? t.sevWarn : muted ? t.faint : t.ink,
            flex: 1,
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {stale ? value : cur ? cur.label : "—"}
        </span>
        <Icon
          name="chev-down"
          size={11}
          stroke={open ? t.accentDeep : t.faint}
        />
      </div>

      {open && anchor && cardEl && (
        <PickPortalMenu
          cardEl={cardEl}
          menuRef={menuRef}
          left={pos ? pos.left : anchor.left}
          top={pos ? pos.top : anchor.below}
          visible={pos != null}
          minWidth={Math.max(anchor.width, 172)}
          onClose={close}
        >
          {optionRows}
        </PickPortalMenu>
      )}
      {open && !anchor && (
        <PickInlineMenu onClose={close}>{optionRows}</PickInlineMenu>
      )}
    </div>
  );
}
