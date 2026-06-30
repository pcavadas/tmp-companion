// src/ui/primitives.tsx — theme-aware reusable building blocks for every screen.
//
// All primitives read the active token object via useTheme(); none hold a private
// palette. Spacing / hairline / radius come from the theme tokens.

import { useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { Dialog, DialogBody, DialogFooter } from "./Dialog";
import { Icon, type IconName } from "./Icon";
import { useTheme, useStyles } from "../theme/ThemeContext";
import { microLabel, plainInput } from "../theme/tokens";

// ===========================================================================
// Button — primary (ink fill / bg text), inverse alias, ghost (bordered), warn.
// ===========================================================================

export type ButtonVariant = "primary" | "ghost" | "warn";

export interface ButtonProps {
  children: ReactNode;
  onClick?: () => void;
  variant?: ButtonVariant;
  disabled?: boolean;
  /** Optional leading icon. */
  icon?: IconName;
  type?: "button" | "submit";
  style?: CSSProperties;
  /** Smaller padding/font (toolbar / inline). */
  small?: boolean;
}

export function Button({
  children,
  onClick,
  variant = "primary",
  disabled,
  icon,
  type = "button",
  style,
  small,
}: ButtonProps) {
  const { t } = useTheme();
  const base: CSSProperties = {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: 6,
    fontFamily: t.sans,
    fontSize: small ? t.fsLabel : t.fsUi,
    fontWeight: 500,
    borderRadius: t.rMd,
    padding: small ? "7px 12px" : "10px 16px",
    cursor: disabled ? "default" : "pointer",
    opacity: disabled ? 0.45 : 1,
    ...style,
  };
  const variantStyle: CSSProperties =
    variant === "primary"
      ? { background: t.accent, color: t.onInk, border: 0 }
      : variant === "warn"
        ? {
            background: "transparent",
            color: t.warn,
            border: `0.5px solid ${t.warn}`,
          }
        : {
            background: "transparent",
            color: t.ink,
            border: `0.5px solid ${t.hairlineStrong}`,
          };

  return (
    <button
      type={type}
      onClick={disabled ? undefined : onClick}
      disabled={disabled}
      style={{ ...base, ...variantStyle }}
    >
      {icon && (
        <Icon
          name={icon}
          size={13}
          stroke={variant === "primary" ? t.onInk : "currentColor"}
        />
      )}
      {children}
    </button>
  );
}

// ===========================================================================
// Slider — labeled range + mono numeric readout (Level rail target/headroom).
// ===========================================================================

export interface SliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (v: number) => void;
  /** Format the mono readout (e.g. (v) => v.toFixed(1)). Defaults to String. */
  format?: (v: number) => string;
}

export function Slider({
  label,
  value,
  min,
  max,
  step = 0.1,
  onChange,
  format,
}: SliderProps) {
  const { t } = useTheme();
  const readout = (format ?? String)(value);
  const pct = ((value - min) / (max - min)) * 100;
  return (
    <div>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "baseline",
          marginBottom: 6,
        }}
      >
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsMicro,
            color: t.mutedInk,
            letterSpacing: t.lsLabel,
            textTransform: "uppercase",
          }}
        >
          {label}
        </span>
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsData,
            color: t.ink,
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {readout}
        </span>
      </div>
      <input
        className="ds-slider-input"
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => {
          onChange(parseFloat(e.target.value));
        }}
        style={{ width: "100%", "--fill": `${String(pct)}%` } as CSSProperties}
      />
    </div>
  );
}

// ===========================================================================
// Modal — destructive-confirm shell: warn kicker + serif headline + mono code
// block + Cancel/Apply. This IS the confirm (window.confirm no-ops in WKWebView).
// ===========================================================================

export interface ModalProps {
  open: boolean;
  /** Warn-red mono kicker, e.g. "DESTRUCTIVE · BACKUP WILL BE CREATED". */
  kicker?: string;
  /** Serif-24 headline (an italic accent <span> is allowed). */
  headline: ReactNode;
  /** Optional serif body prose (the revert promise). */
  body?: ReactNode;
  /** Mono code block content on bgAlt (run_id / scope / op). */
  code?: ReactNode;
  /** Apply / confirm label (default "Apply"). */
  applyLabel?: ReactNode;
  /** Cancel label (default "Cancel"). */
  cancelLabel?: ReactNode;
  onApply: () => void;
  onCancel: () => void;
  /** Apply button variant (default primary; pass "warn" for revert). */
  applyVariant?: ButtonVariant;
}

export function Modal({
  open,
  kicker,
  headline,
  body,
  code,
  applyLabel = "Apply",
  cancelLabel = "Cancel",
  onApply,
  onCancel,
  applyVariant = "primary",
}: ModalProps) {
  const { t } = useTheme();
  if (!open) return null;
  return (
    <Dialog size="sm" onClose={onCancel}>
      <DialogBody>
        {kicker && (
          <div
            style={{
              fontFamily: t.mono,
              fontSize: t.fsData2,
              letterSpacing: t.lsKicker,
              textTransform: "uppercase",
              color: t.warn,
            }}
          >
            {kicker}
          </div>
        )}
        <h2
          style={{
            fontFamily: t.serif,
            fontSize: t.fsTitle,
            fontWeight: 400,
            margin: "6px 0 12px",
            letterSpacing: t.lsTight,
          }}
        >
          {headline}
        </h2>
        {body && (
          <p
            style={{
              fontFamily: t.serif,
              fontSize: t.fsName,
              color: t.mutedInk,
              margin: 0,
              lineHeight: 1.55,
            }}
          >
            {body}
          </p>
        )}
        {code != null && (
          <div
            style={{
              marginTop: 18,
              padding: 12,
              background: t.bgAlt,
              borderRadius: t.rSm,
              fontFamily: t.mono,
              fontSize: t.fsData,
              color: t.ink,
              lineHeight: 1.6,
            }}
          >
            {code}
          </div>
        )}
      </DialogBody>
      <DialogFooter>
        <Button variant="ghost" small onClick={onCancel}>
          {cancelLabel}
        </Button>
        <Button variant={applyVariant} small onClick={onApply}>
          {applyLabel}
        </Button>
      </DialogFooter>
    </Dialog>
  );
}

// ===========================================================================
// Toast — transient bottom-right notice. Auto-dismiss is the caller's job.
// ===========================================================================

export type ToastKind = "ok" | "warn" | "err" | "info";

export interface ToastProps {
  message: ReactNode;
  kind?: ToastKind;
  onDismiss?: () => void;
}

export function Toast({ message, kind = "info", onDismiss }: ToastProps) {
  const { t } = useTheme();
  const edge =
    kind === "ok"
      ? t.accent
      : kind === "warn"
        ? t.sevWarn
        : kind === "err"
          ? t.warn
          : t.mutedInk;
  return (
    <div
      style={{
        position: "absolute",
        right: 18,
        bottom: 18,
        minWidth: 240,
        maxWidth: 380,
        background: t.bg,
        color: t.ink,
        border: `0.5px solid ${t.hairlineStrong}`,
        borderLeft: `2px solid ${edge}`,
        borderRadius: t.rSm,
        padding: "10px 14px",
        boxShadow: "0 24px 48px -16px rgba(0,0,0,0.35)",
        fontFamily: t.sans,
        fontSize: t.fsUi,
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 12,
        zIndex: 60,
      }}
    >
      <span>{message}</span>
      {onDismiss && (
        <button
          type="button"
          onClick={onDismiss}
          style={{
            background: "transparent",
            border: 0,
            color: t.mutedInk,
            cursor: "pointer",
            fontFamily: t.mono,
            fontSize: t.fsUi,
          }}
        >
          ×
        </button>
      )}
    </div>
  );
}

// ---- Scrim (outside-click catcher behind a floating popover) ---------------
// A full-viewport transparent click-catcher. `zIndex` is per-context (popover
// stacks differ by surface). The popover card itself is composed at the call
// site from `useStyles().popoverCard`.
export interface ScrimProps {
  onClose: () => void;
  zIndex?: number;
}

export function Scrim({ onClose, zIndex = 30 }: ScrimProps) {
  return (
    <div
      onClick={onClose}
      style={{ position: "fixed", inset: 0, zIndex, background: "transparent" }}
    />
  );
}

// ---- Checkbox (per-row + select-all; supports indeterminate) ---------------
export interface CheckboxProps {
  checked?: boolean;
  indeterminate?: boolean;
}

export function Checkbox({ checked, indeterminate }: CheckboxProps) {
  const { t } = useTheme();
  const on = (checked ?? false) || (indeterminate ?? false);
  return (
    <span
      style={{
        width: 14,
        height: 14,
        borderRadius: t.rSm,
        border: `1px solid ${on ? t.accent : t.hairlineStrong}`,
        background: on ? t.accent : "transparent",
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
        boxSizing: "border-box",
      }}
    >
      {indeterminate ? (
        <span
          style={{ width: 7, height: 2, borderRadius: 1, background: "#fff" }}
        />
      ) : checked ? (
        <Icon name="check" size={10} stroke="#fff" />
      ) : null}
    </span>
  );
}

// ---- Toggle (pill switch) — a run-wide MODE control, distinct from the per-row
//      selection Checkboxes. 38×22 track, 17×17 sliding knob. -----------------
export interface ToggleProps {
  on: boolean;
  /** Receives the event so the caller can stopPropagation (e.g. when the whole
   *  enclosing row is also clickable). */
  onClick?: (e: React.MouseEvent) => void;
}

export function Toggle({ on, onClick }: ToggleProps) {
  const { t } = useTheme();
  return (
    <span
      onClick={onClick}
      style={{
        display: "inline-block",
        width: 38,
        height: 22,
        borderRadius: t.rPill,
        flexShrink: 0,
        background: on ? t.accent : "rgba(15,17,21,0.14)",
        border: `0.5px solid ${on ? t.accent : t.hairlineStrong}`,
        position: "relative",
        cursor: "pointer",
        transition: "background 0.16s",
      }}
    >
      <span
        style={{
          position: "absolute",
          top: 2.5,
          left: on ? 18 : 2,
          width: 17,
          height: 17,
          borderRadius: t.rPill,
          background: "#fff",
          boxShadow: "0 1px 3px rgba(0,0,0,0.25)",
          transition: "left 0.16s",
        }}
      />
    </span>
  );
}

// ---- MenuItem (hover-tinted popover row) — radius 5 matches the prototype's
//      SMenuItem (songs.jsx) / mItem (screens.jsx). -------------------------
export interface MenuItemProps {
  label: string;
  onClick: () => void;
  danger?: boolean;
}

export function MenuItem({ label, onClick, danger }: MenuItemProps) {
  const { t } = useTheme();
  return (
    <div
      onClick={onClick}
      style={{
        fontFamily: t.sans,
        fontSize: t.fsControl,
        color: danger ? t.warn : t.ink2,
        padding: "7px 10px",
        borderRadius: t.rMenuItem,
        cursor: "pointer",
        whiteSpace: "nowrap",
      }}
      onMouseEnter={(e) => (e.currentTarget.style.background = t.hover)}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      {label}
    </div>
  );
}

/** Hairline separator between groups of MenuItems (inside a <Menu>). */
export function MenuDivider() {
  const { t } = useTheme();
  return (
    <div style={{ height: 1, background: t.hairline, margin: "4px 6px" }} />
  );
}

// ---- SearchInput -----------------------------------------------------------
export interface SearchInputProps {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
  disabled?: boolean;
  /** Show a clear (×) affordance when there's a value. */
  clearable?: boolean;
  /** Outer frame overrides (e.g. `{ flex: 1 }`, a loading `opacity`). */
  style?: CSSProperties;
}

/** Icon + transparent input (+ optional clear) in the shared `searchBox` frame — the one
 *  filter/search field used across the Presets list, Catalog, and the add-songs popover. */
export function SearchInput({
  value,
  onChange,
  placeholder,
  autoFocus,
  disabled,
  clearable,
  style,
}: SearchInputProps) {
  const { t } = useTheme();
  const s = useStyles();
  return (
    <div style={{ ...s.searchBox, ...style }}>
      <Icon name="search" size={13} stroke={t.faint} />
      <input
        type="text"
        value={value}
        autoFocus={autoFocus}
        disabled={disabled}
        placeholder={placeholder}
        onChange={(e) => {
          onChange(e.target.value);
        }}
        style={plainInput(t, {
          flex: 1,
          minWidth: 0,
          fontFamily: t.sans,
          fontSize: t.fsControl,
        })}
      />
      {clearable && value && (
        <span
          onClick={() => {
            onChange("");
          }}
          style={{ cursor: "pointer", display: "flex" }}
        >
          <Icon name="x" size={13} stroke={t.faint} />
        </span>
      )}
    </div>
  );
}

// ---- Select ----------------------------------------------------------------
export interface SelectProps {
  options: string[];
  value?: string;
  onChange?: (v: string) => void;
  style?: CSSProperties;
}

export function Select({ options, value, onChange, style }: SelectProps) {
  const { t } = useTheme();
  return (
    <div style={{ position: "relative", ...style }}>
      <select
        value={value}
        onChange={(e) => onChange?.(e.target.value)}
        style={{
          width: "100%",
          appearance: "none",
          WebkitAppearance: "none",
          padding: "7px 28px 7px 10px",
          background: t.bg,
          color: t.ink,
          border: `0.5px solid ${t.hairline}`,
          borderRadius: t.rMd,
          fontFamily: t.sans,
          fontSize: t.fsUi,
          cursor: "pointer",
          outline: "none",
        }}
      >
        {options.map((o) => (
          <option key={o} value={o}>
            {o}
          </option>
        ))}
      </select>
      <span
        style={{
          position: "absolute",
          right: 9,
          top: "50%",
          transform: "translateY(-50%)",
          pointerEvents: "none",
          color: t.mutedInk,
          display: "inline-flex",
        }}
      >
        <Icon name="arrow-down" size={12} stroke="currentColor" />
      </span>
    </div>
  );
}

// ---- Tag -------------------------------------------------------------------
export interface TagProps {
  children: ReactNode;
  tone?: "muted" | "accent" | "warn";
}

export function Tag({ children, tone = "muted" }: TagProps) {
  const { t } = useTheme();
  const color =
    tone === "accent" ? t.accent : tone === "warn" ? t.warn : t.mutedInk;
  return (
    <span style={{ ...microLabel(t), letterSpacing: t.lsTag, color }}>
      {children}
    </span>
  );
}

// ---- Panel -----------------------------------------------------------------
export interface PanelProps {
  title?: ReactNode;
  children: ReactNode;
  padded?: boolean;
  style?: CSSProperties;
}

export function Panel({ title, children, padded = true, style }: PanelProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        border: `0.5px solid ${t.hairline}`,
        borderRadius: t.rMd,
        overflow: "hidden",
        ...style,
      }}
    >
      {title && (
        <div style={{ ...microLabel(t), padding: "10px 12px 0" }}>{title}</div>
      )}
      <div style={{ padding: padded ? 12 : 0 }}>{children}</div>
    </div>
  );
}

// ===========================================================================
// AlertBanner — the one `role="alert"` warn banner: terracotta hairline + a 2px
// left rule on accentSoft. Used for connect errors (App) and load errors (the
// device views). Defaults suit an inline banner; pass `style` for the
// per-surface deltas (margin / padding / type scale).
// ===========================================================================

export interface AlertBannerProps {
  children: ReactNode;
  style?: CSSProperties;
}

export function AlertBanner({ children, style }: AlertBannerProps) {
  const { t } = useTheme();
  return (
    <div
      role="alert"
      style={{
        border: `0.5px solid ${t.warn}`,
        borderLeft: `2px solid ${t.warn}`,
        borderRadius: t.rMd,
        background: t.accentSoft,
        color: t.warn,
        fontFamily: t.sans,
        fontSize: t.fsBody,
        padding: "14px 16px",
        ...style,
      }}
    >
      {children}
    </div>
  );
}

// ===========================================================================
// SegmentedControl — connected single-choice radio group (segmented pills).
// `filled` = accent fill + hover (data chrome); `light` = soft pill + optional
// per-option icon (uppercase micro). Set the active fill via the backgroundColor
// LONGHAND with NO transition: the `background` shorthand + transition combo was
// observed to leave the computed fill stuck on the previously selected segment.
// ===========================================================================

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  /** Optional leading icon (rendered before the label; used by the `light` variant). */
  icon?: IconName;
}

export interface SegmentedControlProps<T extends string> {
  options: SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  /** Required: a role="radiogroup" needs an accessible name. */
  ariaLabel: string;
  variant?: "filled" | "light";
}

export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  ariaLabel,
  variant = "filled",
}: SegmentedControlProps<T>) {
  const { t } = useTheme();
  const [hover, setHover] = useState<T | null>(null);
  const light = variant === "light";

  // Each variant is a complete named design (palette + typography + casing +
  // shadow), so resolve the full per-variant style once rather than threading a
  // `light ? a : b` ternary through every style property.
  const v = light
    ? {
        trackGap: 3,
        fontSize: t.fsMicro,
        letterSpacing: "0.07em",
        textTransform: "uppercase" as const,
        padding: "6px 8px",
        height: undefined as number | undefined,
        onBg: t.bgAlt,
        onFg: t.ink,
        offFg: t.faint,
        onIcon: t.accentDeep,
        offIcon: t.faint,
        shadow: "0 1px 2px rgba(15,17,21,0.10)",
      }
    : {
        trackGap: 2,
        fontSize: t.fsData,
        letterSpacing: "0.03em",
        textTransform: "none" as const,
        padding: "0 8px",
        height: 30 as number | undefined,
        onBg: t.accent,
        onFg: t.onInk,
        offFg: t.ink2,
        onIcon: t.onInk,
        offIcon: t.ink2,
        shadow: "0 1px 2px rgba(15,17,21,0.18)",
      };

  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      style={{
        display: "flex",
        gap: v.trackGap,
        padding: v.trackGap,
        borderRadius: t.rLg,
        border: `0.5px solid ${t.hairlineStrong}`,
        background: t.bg,
      }}
    >
      {options.map((o) => {
        const on = o.value === value;
        // Hover is filled-only; the light variant has no hover affordance.
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={on}
            onClick={() => {
              onChange(o.value);
            }}
            onMouseEnter={
              light
                ? undefined
                : () => {
                    setHover(o.value);
                  }
            }
            onMouseLeave={
              light
                ? undefined
                : () => {
                    setHover(null);
                  }
            }
            style={{
              flex: 1,
              appearance: "none",
              border: "none",
              cursor: "pointer",
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              gap: 6,
              borderRadius: t.rMd,
              fontFamily: t.mono,
              fontSize: v.fontSize,
              letterSpacing: v.letterSpacing,
              textTransform: v.textTransform,
              whiteSpace: "nowrap",
              padding: v.padding,
              height: v.height,
              color: on ? v.onFg : v.offFg,
              backgroundColor: on
                ? v.onBg
                : hover === o.value
                  ? t.hover
                  : "transparent",
              boxShadow: on ? v.shadow : "none",
            }}
          >
            {o.icon ? (
              <Icon
                name={o.icon}
                size={12}
                stroke={on ? v.onIcon : v.offIcon}
              />
            ) : null}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
