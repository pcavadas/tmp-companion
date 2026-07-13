// src/views/overlays/Pick.tsx — the leveling wizard's click-only dropdown.
//
// The menu PORTALS into the wizard card (via DialogCardCtx) and positions itself in the
// card's own coordinate space, flipping ABOVE the trigger when it would overflow the
// fixed-height frame — so a picker on a bottom row never clips past the card (the
// behaviour the design prototype specs). Falls back to a trigger-anchored inline menu
// when rendered outside a wizard card. The two menu deliveries are dedicated components
// (PickPortalMenu / PickInlineMenu); Pick owns the trigger, state, and the option rows.

import { useContext, useLayoutEffect, useRef, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { DialogCardCtx } from "./wizardContext";
import { PickPortalMenu } from "./PickPortalMenu";
import { PickInlineMenu } from "./PickInlineMenu";

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
   *  test can open a specific row's picker without relying on portal layout. */
  tid?: string;
}

interface Anchor {
  left: number;
  below: number;
  above: number;
  width: number;
  cardW: number;
  cardH: number;
}

export function Pick({
  value,
  options,
  onChange,
  grow,
  title,
  muted,
  tid,
}: PickProps) {
  const { t } = useTheme();
  const [open, setOpen] = useState(false);
  const [anchor, setAnchor] = useState<Anchor | null>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  // The portal target card element, captured from the context ref in `openMenu` (an
  // event handler) so render never reads `ref.current` — `open`/`anchor`/`cardEl` are
  // all set together when the menu opens, so the portal renders in the same pass.
  const [cardEl, setCardEl] = useState<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const cardRef = useContext(DialogCardCtx);
  // `options[0]` is `PickOption | undefined` at runtime (the array can be empty), but
  // this tsconfig has no `noUncheckedIndexedAccess` — spell the real type out so the
  // empty-list fallback below isn't seen as redundant.
  const first: PickOption | undefined =
    options.length > 0 ? options[0] : undefined;
  const cur = options.find((o) => o.id === value) ?? first;

  const openMenu = (e: React.MouseEvent) => {
    e.stopPropagation();
    const card = cardRef?.current;
    if (!card || !triggerRef.current) {
      // No card context (Pick used outside a wizard) → trigger-anchored inline menu.
      setAnchor(null);
      setOpen((o) => !o);
      return;
    }
    const tr = triggerRef.current.getBoundingClientRect();
    const cr = card.getBoundingClientRect();
    setAnchor({
      left: tr.left - cr.left,
      below: tr.bottom - cr.top + 4,
      above: tr.top - cr.top,
      width: tr.width,
      cardW: cr.width,
      cardH: cr.height,
    });
    setCardEl(card);
    setPos(null);
    setOpen(true);
  };

  // Two-pass: render hidden to measure, then place (clamped horizontally, flipped above
  // when it would overflow the card bottom).
  useLayoutEffect(() => {
    if (!open || !anchor || !menuRef.current) return;
    const mw = menuRef.current.offsetWidth;
    const mh = menuRef.current.offsetHeight;
    const left = Math.min(
      Math.max(8, anchor.left),
      Math.max(8, anchor.cardW - mw - 8),
    );
    let top = anchor.below;
    if (top + mh > anchor.cardH - 8) top = Math.max(8, anchor.above - mh - 4);
    setPos({ left, top });
  }, [open, anchor]);

  const close = () => {
    setOpen(false);
    setPos(null);
  };
  const pick = (id: string) => {
    close();
    onChange(id);
  };

  const optionRows = options.map((o) => {
    const on = o.id === value;
    return (
      <div
        key={o.id}
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
        title={title}
        data-pick={tid}
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space3,
          height: 26,
          padding: `0 ${String(t.space3)}px 0 ${String(t.space4)}px`,
          border: `0.5px solid ${open ? t.accent : t.hairlineStrong}`,
          borderRadius: 6,
          background: t.bg,
          cursor: "pointer",
          whiteSpace: "nowrap",
        }}
      >
        <span
          style={{
            fontFamily: t.mono,
            fontSize: 10.5,
            color: muted ? t.faint : t.ink,
            flex: 1,
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {cur ? cur.label : "—"}
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
