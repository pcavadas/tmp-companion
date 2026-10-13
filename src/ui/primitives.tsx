// src/ui/primitives.tsx — theme-aware reusable building blocks for every screen.
//
// All primitives read the active token object via useTheme(); none hold a private
// palette. Spacing / hairline / radius come from the theme tokens.
//
// This file is deliberately NOT split into one-component-per-file, unlike the rest
// of ui/. Modal renders Button, so splitting them reintroduces a circular import
// between the resulting modules. Keep them co-located.

import { useState } from "react";
import { createPortal } from "react-dom";
import type {
  ComponentPropsWithRef,
  CSSProperties,
  MouseEvent,
  ReactNode,
} from "react";
import { Dialog, DialogBody, DialogFooter } from "./Dialog";
import { Icon, type IconName } from "./Icon";
import { Spinner } from "./Spinner";
import { ProgressBar } from "./ProgressBar";
import { useTheme, useStyles } from "../theme/ThemeContext";
import { plainButton, plainInput, type ThemeTokens } from "../theme/tokens";

// ===========================================================================
// Button — primary (ink fill / bg text), ghost (bordered), warn, and `bare` (a
// native-button reset that keeps the look of the row / chip / glyph / inline link
// it replaces). EVERY clickable control is a real <button>, so it is reachable by
// Tab and VoiceOver and activates on Space/Enter natively — the app adds no key
// handlers of its own (click-only). The rest of the native button props pass
// straight through (aria-*, title, ref, onMouseEnter, …), and `onClick` receives
// the event so a nested control can stopPropagation.
// ===========================================================================

export type ButtonVariant = "primary" | "ghost" | "warn" | "bare";
export type ButtonSize = "sm" | "md" | "lg";

export interface ButtonProps extends ComponentPropsWithRef<"button"> {
  variant?: ButtonVariant;
  /** `sm` = toolbar / inline, `md` = default, `lg` = a hero call to action.
   *  Ignored by the `bare` variant, which carries no padding or type scale. */
  size?: ButtonSize;
  /** Optional leading icon. */
  icon?: IconName;
}

export function Button({
  children,
  variant = "primary",
  size = "md",
  disabled,
  icon,
  type = "button",
  style,
  ...rest
}: ButtonProps) {
  const { t } = useTheme();
  if (variant === "bare") {
    return (
      <button
        {...rest}
        type={type}
        disabled={disabled}
        style={plainButton({
          cursor: disabled ? "default" : "pointer",
          ...style,
        })}
      >
        {icon && <Icon name={icon} size={13} stroke="currentColor" />}
        {children}
      </button>
    );
  }
  const sizing = {
    sm: {
      fontSize: t.fsLabel,
      padding: `${String(t.space4)}px ${String(t.space6)}px`,
    },
    md: {
      fontSize: t.fsUi,
      padding: `${String(t.space5)}px ${String(t.space8)}px`,
    },
    lg: {
      fontSize: t.fsBody,
      padding: `${String(t.space6)}px ${String(t.space10)}px`,
    },
  }[size];
  const base: CSSProperties = {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: t.space3,
    fontFamily: t.sans,
    ...sizing,
    fontWeight: 500,
    borderRadius: t.rMd,
    cursor: disabled ? "default" : "pointer",
    opacity: disabled ? 0.45 : 1,
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
      {...rest}
      type={type}
      disabled={disabled}
      style={{ ...base, ...variantStyle, ...style }}
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
// Modal — destructive-confirm shell: warn kicker + serif headline + mono code
// block + Cancel/Apply. This IS the confirm (window.confirm no-ops in WKWebView).
// ===========================================================================

export interface ModalProps {
  open: boolean;
  /** Accessible name for the dialog — plain text, since `headline` may be markup. */
  label: string;
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
  label,
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
    <Dialog size="sm" onClose={onCancel} label={label}>
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
            margin: `${String(t.space3)}px 0 ${String(t.space6)}px`,
            letterSpacing: t.lsTight,
          }}
        >
          {headline}
        </h2>
        {body && (
          <div
            style={{
              fontFamily: t.serif,
              fontSize: t.fsName,
              color: t.mutedInk,
              margin: 0,
              lineHeight: 1.55,
            }}
          >
            {body}
          </div>
        )}
        {code != null && (
          <div
            style={{
              marginTop: t.space8,
              padding: t.space6,
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
        <Button variant="ghost" size="sm" onClick={onCancel}>
          {cancelLabel}
        </Button>
        <Button variant={applyVariant} size="sm" onClick={onApply}>
          {applyLabel}
        </Button>
      </DialogFooter>
    </Dialog>
  );
}

// ===========================================================================
// Toast — transient bottom-right notice. Auto-dismiss is the caller's job.
// Covers the update-notification lifecycle (available/downloading/success/error)
// plus the legacy ok/warn/err/info severity kind for back-compat call sites.
// ===========================================================================

export type ToastKind = "ok" | "warn" | "err" | "info";
export type ToastStatus = "available" | "downloading" | "success" | "error";

export interface ToastAction {
  label: string;
  onClick: () => void;
  /** Filled/high-emphasis button. Only one action per toast should be primary. */
  primary?: boolean;
}

export interface ToastProps {
  message?: ReactNode;
  /** Bold headline. Falls back to `message` when omitted (legacy kind-only callers). */
  title?: ReactNode;
  /** Legacy severity — mapped to `status` when `status` isn't given directly. */
  kind?: ToastKind;
  /** Drives icon, left-accent + label. Overrides the `kind` mapping when given. */
  status?: ToastStatus;
  /** 0–100. Only relevant when status === "downloading". */
  percent?: number;
  /** Show the live "NN%" readout next to the downloading title. Default true. */
  showPercent?: boolean;
  /** Rendered left to right. Max 2 recommended. */
  actions?: ToastAction[];
  onDismiss?: () => void;
}

/** Legacy `kind` isn't just a `status` alias: `warn` gets its own amber "notice"
 *  tone rather than the shared red `error` tone, since the app's one live `warn`
 *  caller (SongsView's BPM-didn't-stick message) is a partial SUCCESS, not a
 *  failure — folding it into "FAILED" would misrepresent it. `info`/`ok` map
 *  straight onto `available`/`success` (accepted: no call site passes them
 *  today, so they don't inherit the update-specific copy in practice). */
type ResolvedStatus = ToastStatus | "notice";

function resolveStatus(
  status: ToastStatus | undefined,
  kind: ToastKind,
): ResolvedStatus {
  if (status) return status;
  if (kind === "ok") return "success";
  if (kind === "err") return "error";
  if (kind === "warn") return "notice";
  return "available";
}

interface ToastTone {
  edge: string;
  chipBg: string;
  label: string | null;
  icon: IconName;
  spin?: boolean;
}

function toneFor(t: ThemeTokens, status: ResolvedStatus): ToastTone {
  switch (status) {
    case "available":
      return {
        edge: t.accent,
        chipBg: t.accentBadgeSoft,
        label: "UPDATE AVAILABLE",
        icon: "download",
      };
    case "downloading":
      return {
        edge: t.accent,
        chipBg: t.accentBadgeSoft,
        label: null,
        icon: "spinner",
        spin: true,
      };
    case "success":
      return {
        edge: t.good,
        chipBg: t.goodSoft,
        label: "READY",
        icon: "check",
      };
    case "error":
      return {
        edge: t.record,
        chipBg: t.recordSoft,
        label: "FAILED",
        icon: "warn-tri",
      };
    case "notice":
      return {
        edge: t.sevWarn,
        chipBg: t.sevWarnSoft,
        label: "NOTICE",
        icon: "warn-tri",
      };
  }
}

export function Toast({
  message,
  title,
  kind = "info",
  status,
  percent,
  showPercent = true,
  actions,
  onDismiss,
}: ToastProps) {
  const { t } = useTheme();
  const resolved = resolveStatus(status, kind);
  const tone = toneFor(t, resolved);
  const isDownloading = resolved === "downloading";
  const titleNode = title ?? message;
  const messageNode = title ? message : undefined;
  const pct =
    isDownloading && typeof percent === "number"
      ? Math.max(0, Math.min(100, percent))
      : undefined;

  // Portalled to <body> with position:fixed: an open Dialog marks the app
  // root `inert`, and a toast left inside it would swallow no clicks — its ×
  // would fall through to the dialog's backdrop and close the dialog.
  return createPortal(
    <div
      className="tmp-toast-in"
      role={resolved === "error" ? "alert" : "status"}
      style={{
        position: "fixed",
        right: 18,
        bottom: 18,
        width: 360,
        background: t.bg,
        color: t.ink,
        border: `0.5px solid ${t.hairlineStrong}`,
        borderLeft: `2.5px solid ${tone.edge}`,
        borderRadius: t.rLg,
        boxShadow: `0 24px 48px -18px ${t.shadow}`,
        padding: `${String(t.space6)}px ${String(t.space7)}px ${String(t.space7)}px ${String(t.space7)}px`,
        display: "flex",
        gap: t.space5,
        fontFamily: t.sans,
        zIndex: 60,
      }}
    >
      <div
        style={{
          flexShrink: 0,
          width: 26,
          height: 26,
          borderRadius: 7,
          background: tone.chipBg,
          color: tone.edge,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          marginTop: t.space1,
        }}
      >
        {tone.spin ? (
          <Spinner name={tone.icon} size={15} strokeWidth={2.4} />
        ) : (
          <Icon name={tone.icon} size={15} strokeWidth={2} />
        )}
      </div>
      <div
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          flexDirection: "column",
          gap: isDownloading ? t.space4 : t.space2,
          paddingRight: t.space7,
        }}
      >
        {tone.label && (
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMicro2,
              fontWeight: 600,
              letterSpacing: t.lsKicker,
              textTransform: "uppercase",
              color: tone.edge,
            }}
          >
            {tone.label}
          </span>
        )}
        <div
          style={{
            display: "flex",
            alignItems: "baseline",
            justifyContent: "space-between",
            gap: t.space4,
          }}
        >
          <span
            style={{
              fontSize: t.fsBody,
              fontWeight: 600,
              lineHeight: 1.35,
              color: t.ink,
            }}
          >
            {titleNode}
          </span>
          {pct !== undefined && showPercent && (
            <span
              style={{
                fontFamily: t.mono,
                fontSize: t.fsData,
                fontWeight: 600,
                color: t.ink2,
                flexShrink: 0,
              }}
            >
              {Math.round(pct)}%
            </span>
          )}
        </div>
        {messageNode && (
          <div
            style={{
              fontSize: t.fsControl,
              lineHeight: 1.45,
              color: t.mutedInk,
            }}
          >
            {messageNode}
          </div>
        )}
        {pct !== undefined && <ProgressBar percent={pct} height={6} />}
        {actions && actions.length > 0 && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: t.space3,
              marginTop: t.space4,
            }}
          >
            {actions.map((a) => (
              <Button
                key={a.label}
                size="sm"
                variant={a.primary ? "primary" : "ghost"}
                onClick={a.onClick}
                style={a.primary ? { background: tone.edge } : undefined}
              >
                {a.label}
              </Button>
            ))}
          </div>
        )}
      </div>
      {onDismiss && !isDownloading && (
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss"
          style={{
            position: "absolute",
            top: 9,
            right: 9,
            width: 22,
            height: 22,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: "transparent",
            border: 0,
            borderRadius: 5,
            color: t.mutedInk,
            cursor: "pointer",
          }}
        >
          <Icon name="x" size={13} strokeWidth={2} />
        </button>
      )}
    </div>,
    document.body,
  );
}

// ---- Checkbox (per-row + select-all; supports indeterminate) ---------------
// The VISUAL box only (aria-hidden). Every checkbox sits inside a clickable cell
// or row, and THAT element is the control: a real <button role="checkbox"> with
// `aria-checked` ("mixed" when indeterminate; `aria-disabled` when inert), so the role
// is never nested inside another interactive element.
export interface CheckboxProps {
  checked?: boolean;
  indeterminate?: boolean;
  /** Not selectable at all (e.g. a footswitch with no level control to adjust —
   *  BUG 1) — renders inert/faint regardless of `checked`/`indeterminate`. The
   *  enclosing row owns the click no-op; this is display-only. */
  disabled?: boolean;
}

export function Checkbox({ checked, indeterminate, disabled }: CheckboxProps) {
  const { t } = useTheme();
  const on = !disabled && ((checked ?? false) || (indeterminate ?? false));
  return (
    <span
      aria-hidden
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
        opacity: disabled ? 0.45 : 1,
      }}
    >
      {disabled ? null : indeterminate ? (
        <span
          style={{
            width: 7,
            height: 2,
            borderRadius: 1,
            background: t.onAccent,
          }}
        />
      ) : checked ? (
        <Icon name="check" size={10} stroke={t.onAccent} />
      ) : null}
    </span>
  );
}

// ---- Toggle (pill switch) — a run-wide MODE control, distinct from the per-row
//      selection Checkboxes. 38×22 track, 17×17 sliding knob. Interactive, it is a
//      real <button role="switch"> and MUST be named (`label`); without `onClick`
//      it is the display-only pill inside a caller's own labelled switch button
//      (SetupPage's "Even out parallel amps" row). -----------------------------
interface ToggleSwitchProps {
  on: boolean;
  /** Receives the event so the caller can stopPropagation (e.g. when the whole
   *  enclosing row is also clickable). */
  onClick: (e: MouseEvent<HTMLButtonElement>) => void;
  /** Accessible name — the switch has no visible text of its own. */
  label: string;
}

interface ToggleDisplayProps {
  on: boolean;
  onClick?: undefined;
  label?: undefined;
}

export type ToggleProps = ToggleSwitchProps | ToggleDisplayProps;

export function Toggle({ on, onClick, label }: ToggleProps) {
  const { t } = useTheme();
  const track: CSSProperties = {
    display: "inline-block",
    width: 38,
    height: 22,
    boxSizing: "content-box",
    borderRadius: t.rPill,
    flexShrink: 0,
    background: on ? t.accent : t.toggleTrack,
    border: `0.5px solid ${on ? t.accent : t.hairlineStrong}`,
    position: "relative",
    cursor: "pointer",
    transition: "background 0.16s",
  };
  const knob = (
    <span
      style={{
        position: "absolute",
        top: 2.5,
        left: on ? 18 : 2,
        width: 17,
        height: 17,
        borderRadius: t.rPill,
        background: t.knob,
        boxShadow: t.shadowKnob,
        transition: "left 0.16s",
      }}
    />
  );
  if (!onClick) return <span style={track}>{knob}</span>;
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      onClick={onClick}
      style={plainButton(track)}
    >
      {knob}
    </button>
  );
}

// ---- MenuItem (hover-tinted popover row) — radius 5 matches the prototype's
//      SMenuItem (songs.jsx) / mItem (screens.jsx). A full-width real <button>. ---
export interface MenuItemProps {
  label: string;
  onClick: () => void;
  danger?: boolean;
}

export function MenuItem({ label, onClick, danger }: MenuItemProps) {
  const { t } = useTheme();
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      style={plainButton({
        display: "block",
        width: "100%",
        textAlign: "left",
        fontFamily: t.sans,
        fontSize: t.fsControl,
        color: danger ? t.warn : t.ink2,
        padding: `${String(t.space4)}px ${String(t.space5)}px`,
        borderRadius: t.rMenuItem,
        whiteSpace: "nowrap",
      })}
      onMouseEnter={(e) => (e.currentTarget.style.background = t.hover)}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      {label}
    </button>
  );
}

/** Hairline separator between groups of MenuItems (inside a <Menu>). */
export function MenuDivider() {
  const { t } = useTheme();
  return (
    <div
      style={{
        height: 1,
        background: t.hairline,
        margin: `${String(t.space2)}px ${String(t.space3)}px`,
      }}
    />
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
        <button
          type="button"
          aria-label="Clear search"
          onClick={() => {
            onChange("");
          }}
          style={plainButton({ display: "flex" })}
        >
          <Icon name="x" size={13} stroke={t.faint} />
        </button>
      )}
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
        padding: `${String(t.space7)}px ${String(t.space8)}px`,
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
  /** `sm` shrinks the filled variant (shorter, smaller type) — e.g. an inline
   *  header filter. Defaults to the standard size. */
  size?: "sm" | "md";
}

export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  ariaLabel,
  variant = "filled",
  size = "md",
}: SegmentedControlProps<T>) {
  const { t } = useTheme();
  const [hover, setHover] = useState<T | null>(null);
  const light = variant === "light";
  const sm = size === "sm";

  // Each variant is a complete named design (palette + typography + casing +
  // shadow), so resolve the full per-variant style once rather than threading a
  // `light ? a : b` ternary through every style property.
  const v = light
    ? {
        trackGap: t.space2,
        fontSize: t.fsMicro,
        letterSpacing: "0.07em",
        textTransform: "uppercase" as const,
        padding: `${String(t.space3)}px ${String(t.space4)}px`,
        height: undefined as number | undefined,
        onBg: t.bgAlt,
        onFg: t.ink,
        offFg: t.faint,
        onIcon: t.accentDeep,
        offIcon: t.faint,
        shadow: "0 1px 2px rgba(15,17,21,0.10)",
      }
    : {
        trackGap: t.space1,
        fontSize: t.fsData,
        letterSpacing: "0.03em",
        textTransform: "none" as const,
        padding: `0 ${String(t.space4)}px`,
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
              gap: t.space3,
              borderRadius: t.rMd,
              fontFamily: t.mono,
              fontSize: sm ? t.fsData2 : v.fontSize,
              letterSpacing: v.letterSpacing,
              textTransform: v.textTransform,
              whiteSpace: "nowrap",
              padding: sm ? `0 ${String(t.space5)}px` : v.padding,
              height: sm && !light ? 24 : v.height,
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
