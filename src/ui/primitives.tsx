// src/ui/primitives.tsx — theme-aware reusable building blocks for every screen (look B).
//
// All primitives read the active token object via useTheme(); none hold a private
// palette. Sizes, radii and colours come from the look B tokens.
//
// This file is deliberately NOT split into one-component-per-file, unlike the rest
// of ui/. Modal renders Button, so splitting them reintroduces a circular import
// between the resulting modules (and Dialog.tsx stays button-free for the same
// reason: any confirm shell that carries buttons lives here). Keep them co-located.

import { useRef, useState } from "react";
import { createPortal } from "react-dom";
import type {
  ComponentPropsWithRef,
  CSSProperties,
  MouseEvent,
  PointerEvent,
  ReactNode,
} from "react";
import { Dialog, DialogBody, DialogFooter, DialogHeader } from "./Dialog";
import { Icon, type IconName } from "./Icon";
import { Spinner } from "./Spinner";
import { ProgressBar } from "./ProgressBar";
import { useTheme, useStyles } from "../theme/ThemeContext";
import { plainButton, plainInput, type ThemeTokens } from "../theme/tokens";

// ===========================================================================
// Button — look B kinds: `primary` (action fill, 600), `secondary` (white with a
// controlBorder edge, 500), `plain` (no edge, text colour), `link` (no edge, link
// colour, no side padding) and `danger` (danger fill, 600). Sizes `sm` 22, `md` 28,
// `lg` 32 high. `busy` keeps the words and adds a 12 px spinner before them (a busy
// button is disabled). Disabled filled buttons turn `track` with `textDisabled` text.
//
// Two more names: `ghost` is a permanent alias of `secondary` (the frozen
// ActiveSignalChainView renders it), and `bare` is NOT a look B kind but the
// native-button reset (no padding, no type scale) behind the clickable rows, cells,
// chips and glyphs.
//
// EVERY clickable control is a real <button>, so it is reachable by Tab and VoiceOver
// and activates on Space/Enter natively — the app adds no key handlers of its own
// (click-only). The rest of the native button props pass straight through (aria-*,
// title, ref, onMouseEnter, …), and `onClick` receives the event so a nested control
// can stopPropagation. An icon-only button needs an `aria-label`.
// ===========================================================================

export type ButtonVariant =
  "primary" | "secondary" | "plain" | "link" | "danger" | "ghost" | "bare";
export type ButtonSize = "sm" | "md" | "lg";

type ButtonKind = "primary" | "secondary" | "plain" | "link" | "danger";

const BUTTON_KIND: Record<Exclude<ButtonVariant, "bare">, ButtonKind> = {
  primary: "primary",
  secondary: "secondary",
  ghost: "secondary",
  plain: "plain",
  link: "link",
  danger: "danger",
};

export interface ButtonProps extends ComponentPropsWithRef<"button"> {
  variant?: ButtonVariant;
  /** `sm` 22 (inside rows), `md` 28 (default), `lg` 32 (the footer's main action).
   *  Ignored by the `bare` variant, which carries no padding or type scale. */
  size?: ButtonSize;
  /** Optional leading icon. */
  icon?: IconName;
  /** Working: keeps the words, adds a 12 px spinner before them, and disables the
   *  button. It never changes its label. */
  busy?: boolean;
}

function buttonMetrics(t: ThemeTokens, size: ButtonSize) {
  switch (size) {
    case "sm":
      return { height: t.controlSmall, font: t.fontCaption, padX: t.sp8 };
    case "md":
      return { height: t.control, font: t.fontBody, padX: t.sp12 };
    case "lg":
      return { height: t.controlLarge, font: t.fontBody, padX: t.sp16 };
  }
}

function buttonPaint(
  t: ThemeTokens,
  kind: ButtonKind,
  inert: boolean,
  pressed: boolean,
): CSSProperties {
  switch (kind) {
    case "primary":
      return {
        background: inert ? t.track : pressed ? t.actionPressed : t.action,
        color: inert ? t.textDisabled : t.onAction,
        border: "none",
        fontWeight: t.weightSemibold,
      };
    case "danger":
      return {
        background: inert ? t.track : t.danger,
        color: inert ? t.textDisabled : t.onAction,
        border: "none",
        fontWeight: t.weightSemibold,
      };
    case "secondary":
      return {
        background: t.surface,
        color: inert ? t.textDisabled : t.text,
        border: `1px solid ${t.controlBorder}`,
        fontWeight: t.weightMedium,
      };
    case "plain":
      return {
        background: "transparent",
        color: inert ? t.textDisabled : t.text,
        border: "none",
        fontWeight: t.weightMedium,
      };
    case "link":
      return {
        background: "transparent",
        color: inert ? t.textDisabled : t.link,
        border: "none",
        fontWeight: t.weightMedium,
      };
  }
}

export function Button({
  children,
  variant = "primary",
  size = "md",
  disabled,
  icon,
  busy = false,
  type = "button",
  style,
  onPointerDown,
  onPointerUp,
  onPointerLeave,
  ...rest
}: ButtonProps) {
  const { t } = useTheme();
  // The primary button's pressed fill (`actionPressed`) — inline styles have no
  // :active, so the press is tracked here.
  const [pressed, setPressed] = useState(false);
  if (variant === "bare") {
    return (
      <button
        {...rest}
        type={type}
        disabled={disabled}
        onPointerDown={onPointerDown}
        onPointerUp={onPointerUp}
        onPointerLeave={onPointerLeave}
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
  const kind = BUTTON_KIND[variant];
  const inert = (disabled ?? false) || busy;
  const m = buttonMetrics(t, size);
  const iconOnly =
    icon != null && (children === undefined || children === null);
  const iconSize = size === "sm" ? 12 : 14;
  const iconStroke =
    kind === "plain" && !inert ? t.textSecondary : "currentColor";

  return (
    <button
      {...rest}
      type={type}
      disabled={inert}
      aria-busy={busy ? true : undefined}
      onPointerDown={(e) => {
        setPressed(true);
        onPointerDown?.(e);
      }}
      onPointerUp={(e) => {
        setPressed(false);
        onPointerUp?.(e);
      }}
      onPointerLeave={(e) => {
        setPressed(false);
        onPointerLeave?.(e);
      }}
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        gap: t.sp6,
        height: m.height,
        width: iconOnly ? m.height : undefined,
        boxSizing: "border-box",
        padding: iconOnly || kind === "link" ? 0 : `0 ${String(m.padX)}px`,
        borderRadius: t.radiusControl,
        fontFamily: t.fontSans,
        fontSize: m.font,
        whiteSpace: "nowrap",
        cursor: inert ? "default" : "pointer",
        ...buttonPaint(t, kind, inert, pressed && !inert),
        ...style,
      }}
    >
      {busy ? (
        <Spinner size={12} strokeWidth={2.2} />
      ) : (
        icon && (
          <Icon
            name={icon}
            size={iconSize}
            stroke={iconStroke}
            strokeWidth={1.8}
          />
        )
      )}
      {children}
    </button>
  );
}

// ===========================================================================
// Modal — the button-bearing confirm shell, in Dialog's look B: title 15/600, a
// 13 px body, buttons right-aligned (secondary Cancel, then the primary or danger
// action). It guards a write or a delete, so a backdrop click does nothing: only its
// own buttons close it. This IS the confirm (window.confirm no-ops in WKWebView).
// ===========================================================================

export interface ModalProps {
  open: boolean;
  /** Accessible name for the dialog — plain text, since `headline` may be markup. */
  label: string;
  /** The title: says what happens. */
  headline: ReactNode;
  /** Optional body: what's safe, what can't be undone. */
  body?: ReactNode;
  /** Apply / confirm label (default "Apply"). */
  applyLabel?: ReactNode;
  /** Cancel label (default "Cancel"). */
  cancelLabel?: ReactNode;
  onApply: () => void;
  onCancel: () => void;
  /** Apply button kind (default primary; "danger" for a delete). */
  applyVariant?: ButtonVariant;
}

export function Modal({
  open,
  label,
  headline,
  body,
  applyLabel = "Apply",
  cancelLabel = "Cancel",
  onApply,
  onCancel,
  applyVariant = "primary",
}: ModalProps) {
  const { t } = useTheme();
  if (!open) return null;
  return (
    // No onClose: the backdrop is inert, so a stray click can't cancel a confirm.
    <Dialog size="xs" label={label}>
      <DialogHeader>
        <h2
          style={{
            margin: 0,
            fontSize: t.fontLarge,
            lineHeight: `${String(t.lineLarge)}px`,
            fontWeight: t.weightSemibold,
            color: t.text,
          }}
        >
          {headline}
        </h2>
      </DialogHeader>
      <DialogBody>
        {body && (
          <div
            style={{
              fontSize: t.fontBody,
              lineHeight: `${String(t.lineBody)}px`,
              color: t.text,
            }}
          >
            {body}
          </div>
        )}
      </DialogBody>
      <DialogFooter>
        <Button variant="secondary" onClick={onCancel}>
          {cancelLabel}
        </Button>
        <Button variant={applyVariant} onClick={onApply}>
          {applyLabel}
        </Button>
      </DialogFooter>
    </Dialog>
  );
}

// ===========================================================================
// Toast — transient notice, bottom-centre: white, `shadowMenu`, 13 px, a tone icon,
// optional actions and a close button. Auto-dismiss is the caller's job. Covers the
// update-notification lifecycle (available/downloading/success/error) plus the legacy
// ok/warn/err/info severity kind. No uppercase status chip: the title says it, the
// icon's colour carries the tone.
// ===========================================================================

export type ToastKind = "ok" | "warn" | "err" | "info";
export type ToastStatus = "available" | "downloading" | "success" | "error";

export interface ToastAction {
  label: string;
  onClick: () => void;
  /** The emphasised action (drawn as a link). Only one per toast should be primary. */
  primary?: boolean;
}

export interface ToastProps {
  message?: ReactNode;
  /** Bold headline. Falls back to `message` when omitted (legacy kind-only callers). */
  title?: ReactNode;
  /** Legacy severity — mapped to `status` when `status` isn't given directly. */
  kind?: ToastKind;
  /** Drives the icon and the role. Overrides the `kind` mapping when given. */
  status?: ToastStatus;
  /** 0–100. Only relevant when status === "downloading". */
  percent?: number;
  /** Show the live "NN%" readout next to the downloading title. Default true. */
  showPercent?: boolean;
  /** Rendered left to right. Max 2 recommended. */
  actions?: ToastAction[];
  onDismiss?: () => void;
  /** The close button's accessible name (what closing means). Default "Dismiss". */
  dismissLabel?: string;
}

/** Legacy `kind` isn't just a `status` alias: `warn` gets its own caution "notice"
 *  tone rather than the shared danger `error` tone, since its callers, Songs'
 *  partial-success toasts in useSongsDevice.ts (a song name already taken, a tempo
 *  that didn't stick), report a save that landed, not a failure. `info`/`ok` map
 *  straight onto `available`/`success`. */
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

function toastIcon(
  t: ThemeTokens,
  status: ResolvedStatus,
): { icon: IconName; color: string } {
  switch (status) {
    case "available":
      return { icon: "download", color: t.textSecondary };
    case "downloading":
      return { icon: "spinner", color: t.textSecondary };
    case "success":
      return { icon: "check", color: t.good };
    case "error":
      return { icon: "alert", color: t.danger };
    case "notice":
      return { icon: "warn-tri", color: t.caution };
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
  dismissLabel = "Dismiss",
}: ToastProps) {
  const { t } = useTheme();
  const resolved = resolveStatus(status, kind);
  const tone = toastIcon(t, resolved);
  const isDownloading = resolved === "downloading";
  const titleNode = title ?? message;
  const messageNode = title ? message : undefined;
  const pct =
    isDownloading && typeof percent === "number"
      ? Math.max(0, Math.min(100, percent))
      : undefined;

  // Portalled to <body> with position:fixed: an open Dialog marks the app root
  // `inert`, and a toast left inside it would swallow no clicks — its × would fall
  // through to the dialog's backdrop. The full-width strip centres the card and lets
  // clicks beside it through.
  return createPortal(
    <div
      style={{
        position: "fixed",
        left: 0,
        right: 0,
        bottom: t.sp16,
        display: "flex",
        justifyContent: "center",
        pointerEvents: "none",
        zIndex: 60,
      }}
    >
      <div
        className="tmp-toast-in"
        role={resolved === "error" ? "alert" : "status"}
        style={{
          pointerEvents: "auto",
          display: "flex",
          alignItems: "flex-start",
          gap: t.sp8,
          width: "max-content",
          maxWidth: "min(440px, calc(100vw - 32px))",
          boxSizing: "border-box",
          padding: `${String(t.sp8)}px ${String(t.sp8)}px ${String(t.sp8)}px ${String(t.sp12)}px`,
          background: t.surface,
          color: t.text,
          borderRadius: t.radiusPanel,
          boxShadow: t.shadowMenu,
          fontFamily: t.fontSans,
          fontSize: t.fontBody,
          lineHeight: `${String(t.lineBody)}px`,
        }}
      >
        <span style={{ display: "flex", paddingTop: t.sp2, flexShrink: 0 }}>
          {tone.icon === "spinner" ? (
            <Spinner size={15} stroke={tone.color} strokeWidth={2} />
          ) : (
            <Icon
              name={tone.icon}
              size={15}
              stroke={tone.color}
              strokeWidth={2}
            />
          )}
        </span>
        <div
          style={{
            flex: 1,
            minWidth: 0,
            display: "flex",
            flexDirection: "column",
            gap: t.sp4,
            padding: `${String(t.sp2)}px 0`,
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "baseline",
              justifyContent: "space-between",
              gap: t.sp8,
            }}
          >
            <span style={{ fontWeight: t.weightSemibold }}>{titleNode}</span>
            {pct !== undefined && showPercent && (
              <span
                style={{
                  color: t.textSecondary,
                  fontVariantNumeric: "tabular-nums",
                  flexShrink: 0,
                }}
              >
                {Math.round(pct)}%
              </span>
            )}
          </div>
          {messageNode && (
            <div style={{ color: t.textSecondary }}>{messageNode}</div>
          )}
          {pct !== undefined && <ProgressBar percent={pct} />}
          {actions && actions.length > 0 && (
            <div style={{ display: "flex", alignItems: "center", gap: t.sp12 }}>
              {actions.map((a) => (
                <Button
                  key={a.label}
                  size="sm"
                  variant={a.primary ? "link" : "plain"}
                  onClick={a.onClick}
                  style={a.primary ? undefined : { padding: 0 }}
                >
                  {a.label}
                </Button>
              ))}
            </div>
          )}
        </div>
        {onDismiss && !isDownloading && (
          <Button
            variant="plain"
            size="sm"
            icon="x"
            aria-label={dismissLabel}
            onClick={onDismiss}
          />
        )}
      </div>
    </div>,
    document.body,
  );
}

// ===========================================================================
// Checkbox — look B: 14 px, `radiusSmall`. Off: white with a `controlBorder` edge.
// On: `action` fill, white tick. Mixed: `action` fill, white dash.
//
// `Checkbox` is a real control (<button role="checkbox"> with an accessible name);
// with a visible label, the box and the words sit in one <label>, so a click on the
// words ticks it too. `CheckboxMark` is the same box as a display-only, aria-hidden
// mark for a row whose whole surface is the <button role="checkbox"> control (the
// Level list's SceneRow), since no control may nest inside another.
// ===========================================================================

function checkboxPaint(
  t: ThemeTokens,
  on: boolean,
  disabled: boolean,
): CSSProperties {
  const fill = disabled ? t.textDisabled : t.action;
  return {
    width: 14,
    height: 14,
    boxSizing: "border-box",
    flexShrink: 0,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    padding: 0,
    margin: 0,
    borderRadius: t.radiusSmall,
    border: `1px solid ${on ? fill : disabled ? t.textDisabled : t.controlBorder}`,
    background: on ? fill : t.surface,
  };
}

function CheckGlyph({ mixed }: { mixed: boolean }) {
  const { t } = useTheme();
  return (
    <Icon
      name={mixed ? "minus" : "check"}
      size={12}
      stroke={t.onAction}
      strokeWidth={2.6}
    />
  );
}

export interface CheckboxProps {
  checked: boolean;
  /** Some of what it stands for is chosen: `aria-checked="mixed"`, a dash. */
  indeterminate?: boolean;
  disabled?: boolean;
  /** Receives the next state (a mixed box turns on) and the click event, so a
   *  control inside a clickable surface can stopPropagation. */
  onChange: (checked: boolean, e: MouseEvent<HTMLButtonElement>) => void;
  /** The accessible name. Drawn beside the box unless `labelHidden`. */
  label: string;
  /** Name the box without drawing the words (a row that shows the name itself). */
  labelHidden?: boolean;
  /** Layout extras for the <label> wrapper (visible-label form only). */
  style?: CSSProperties;
}

export function Checkbox({
  checked,
  indeterminate = false,
  disabled = false,
  onChange,
  label,
  labelHidden = false,
  style,
}: CheckboxProps) {
  const { t } = useTheme();
  const on = checked || indeterminate;
  const box = (
    <button
      type="button"
      role="checkbox"
      aria-checked={indeterminate ? "mixed" : checked}
      aria-label={label}
      disabled={disabled}
      onClick={(e) => {
        onChange(indeterminate ? true : !checked, e);
      }}
      style={{
        ...checkboxPaint(t, on, disabled),
        cursor: disabled ? "default" : "pointer",
      }}
    >
      {on && <CheckGlyph mixed={indeterminate} />}
    </button>
  );
  if (labelHidden) return box;
  return (
    <label
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: t.sp8,
        fontFamily: t.fontSans,
        fontSize: t.fontBody,
        color: disabled ? t.textDisabled : t.text,
        cursor: disabled ? "default" : "pointer",
        userSelect: "none",
        ...style,
      }}
    >
      {box}
      {label}
    </label>
  );
}

export interface CheckboxMarkProps {
  checked?: boolean;
  indeterminate?: boolean;
  /** Not selectable at all (e.g. a footswitch with no level control to adjust —
   *  BUG 1) — an empty, greyed box regardless of `checked`/`indeterminate`. The
   *  enclosing row owns the click no-op; this is display-only. */
  disabled?: boolean;
}

/** The display-only box for a row whose own <button role="checkbox"> is the control. */
export function CheckboxMark({
  checked = false,
  indeterminate = false,
  disabled = false,
}: CheckboxMarkProps) {
  const { t } = useTheme();
  const on = !disabled && (checked || indeterminate);
  return (
    <span aria-hidden style={checkboxPaint(t, on, disabled)}>
      {on && <CheckGlyph mixed={indeterminate} />}
    </span>
  );
}

// ---- Radio (single choice) — 14 px circle, filled `action` with a white dot when
//      on. A real <button role="radio">; with a visible label, wrapped in one
//      <label>. ------
export interface RadioProps {
  checked: boolean;
  /** Receives the click event, so a control inside a clickable surface can
   *  stopPropagation. */
  onSelect: (e: MouseEvent<HTMLButtonElement>) => void;
  /** The accessible name. Drawn beside the circle unless `labelHidden`. */
  label: string;
  labelHidden?: boolean;
  disabled?: boolean;
  /** Layout extras for the <label> wrapper (visible-label form only). */
  style?: CSSProperties;
}

export function Radio({
  checked,
  onSelect,
  label,
  labelHidden = false,
  disabled = false,
  style,
}: RadioProps) {
  const { t } = useTheme();
  const edge = disabled ? t.textDisabled : checked ? t.action : t.controlBorder;
  const circle = (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={onSelect}
      style={{
        width: 14,
        height: 14,
        boxSizing: "border-box",
        flexShrink: 0,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        padding: 0,
        margin: 0,
        borderRadius: t.radiusPill,
        border: `1px solid ${edge}`,
        // On: `action` fill around a white dot, like a ticked Checkbox.
        background: checked && !disabled ? t.action : t.surface,
        cursor: disabled ? "default" : "pointer",
      }}
    >
      {checked && (
        <span
          style={{
            width: 6,
            height: 6,
            borderRadius: t.radiusPill,
            background: disabled ? t.textDisabled : t.onAction,
          }}
        />
      )}
    </button>
  );
  if (labelHidden) return circle;
  return (
    <label
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: t.sp8,
        fontFamily: t.fontSans,
        fontSize: t.fontBody,
        color: disabled ? t.textDisabled : t.text,
        cursor: disabled ? "default" : "pointer",
        userSelect: "none",
        ...style,
      }}
    >
      {circle}
      {label}
    </label>
  );
}

// ---- Toggle (switch) — only for a setting that takes effect straight away. A
//      26×16 pill: `switchOff` when off, `action` when on. Interactive, it is a real
//      <button role="switch"> and MUST be named (`label`); without `onClick` it is the
//      display-only pill inside a caller's own labelled switch button. -------------
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
    width: 26,
    height: 16,
    boxSizing: "border-box",
    borderRadius: t.radiusPill,
    flexShrink: 0,
    background: on ? t.action : t.switchOff,
    position: "relative",
    cursor: "pointer",
    transition: "background 0.16s",
  };
  const knob = (
    <span
      style={{
        position: "absolute",
        top: 2,
        left: on ? 12 : 2,
        width: 12,
        height: 12,
        borderRadius: t.radiusPill,
        background: t.surface,
        boxShadow: t.shadowSegment,
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

// ---- MenuItem — a 28-high popover row: 13 px after a 12 px check column,
//      `radiusSmall`, `rowHover` on hover, `danger` items in the danger colour. A
//      full-width real <button>. -----------
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
        display: "flex",
        alignItems: "center",
        width: "100%",
        height: t.control,
        textAlign: "left",
        fontFamily: t.fontSans,
        fontSize: t.fontBody,
        color: danger ? t.danger : t.text,
        gap: t.sp6,
        padding: `0 ${String(t.sp8)}px`,
        borderRadius: t.radiusSmall,
        whiteSpace: "nowrap",
      })}
      onMouseEnter={(e) => (e.currentTarget.style.background = t.rowHover)}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      {/* The 12 px check column every menu keeps, as a Select's list does. */}
      <span
        aria-hidden
        data-menu-gutter=""
        style={{ width: 12, flexShrink: 0 }}
      />
      {label}
    </button>
  );
}

/** A `separator` line between groups of MenuItems (inside a <Menu>). */
export function MenuDivider() {
  const { t } = useTheme();
  return (
    <div
      role="separator"
      style={{
        height: 1,
        background: t.separator,
        margin: `${String(t.sp4)}px ${String(t.sp6)}px`,
      }}
    />
  );
}

// ---- SearchInput -----------------------------------------------------------
export interface SearchInputProps {
  value: string;
  onChange: (v: string) => void;
  /** Says what you can type ("Filter by name or slot"); also the accessible name
   *  unless `ariaLabel` is given. */
  placeholder?: string;
  ariaLabel?: string;
  autoFocus?: boolean;
  disabled?: boolean;
  /** Show a clear (×) affordance when there's a value. */
  clearable?: boolean;
  /** Outer frame overrides (e.g. `{ flex: 1 }`, a fixed width). */
  style?: CSSProperties;
}

/** Icon + transparent input (+ optional clear) in the shared `searchBox` frame (28 high,
 *  `controlBorder` edge) — the one filter/search field used across the app. The frame's
 *  `tmp-search` class (index.html) rings it with the `focus` colour while the field has
 *  focus; the field itself keeps plainInput's outline reset. */
export function SearchInput({
  value,
  onChange,
  placeholder,
  ariaLabel,
  autoFocus,
  disabled,
  clearable,
  style,
}: SearchInputProps) {
  const { t } = useTheme();
  const s = useStyles();
  return (
    <div className="tmp-search" style={{ ...s.searchBox, ...style }}>
      <Icon name="search" size={13} stroke={t.textSecondary} strokeWidth={2} />
      <input
        type="text"
        value={value}
        autoFocus={autoFocus}
        disabled={disabled}
        placeholder={placeholder}
        aria-label={ariaLabel ?? placeholder}
        onChange={(e) => {
          onChange(e.target.value);
        }}
        style={plainInput(t, {
          flex: 1,
          minWidth: 0,
          fontFamily: t.fontSans,
          fontSize: t.fontBody,
          color: disabled ? t.textDisabled : t.text,
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
          <Icon name="x" size={12} stroke={t.textSecondary} strokeWidth={2} />
        </button>
      )}
    </div>
  );
}

// ---- TextField / TextArea — a label above (12 px semibold, secondary) and a 28-high
//      field with a `controlBorder` edge (TextArea: free height). The <label> wraps
//      both, so the words name the field. Enter/Escape are left to the browser: no
//      key handling here. -------------------------------------------------------
function fieldStyle(t: ThemeTokens, disabled: boolean): CSSProperties {
  return {
    boxSizing: "border-box",
    width: "100%",
    minWidth: 0,
    margin: 0,
    border: `1px solid ${t.controlBorder}`,
    borderRadius: t.radiusControl,
    background: t.surface,
    fontFamily: t.fontSans,
    fontSize: t.fontBody,
    color: disabled ? t.textDisabled : t.text,
  };
}

function fieldLabelStyle(t: ThemeTokens): CSSProperties {
  return {
    fontSize: t.fontCaption,
    lineHeight: `${String(t.lineCaption)}px`,
    fontWeight: t.weightSemibold,
    color: t.textSecondary,
  };
}

export interface TextFieldProps extends Omit<
  ComponentPropsWithRef<"input">,
  "value" | "onChange" | "style" | "size"
> {
  /** The visible label above the field (and its accessible name). */
  label: string;
  value: string;
  onChange: (value: string) => void;
  /** Layout extras for the wrapper (width, flex). */
  style?: CSSProperties;
}

export function TextField({
  label,
  value,
  onChange,
  style,
  disabled,
  type = "text",
  ...rest
}: TextFieldProps) {
  const { t } = useTheme();
  return (
    <label
      style={{
        display: "flex",
        flexDirection: "column",
        gap: t.sp4,
        fontFamily: t.fontSans,
        ...style,
      }}
    >
      <span style={fieldLabelStyle(t)}>{label}</span>
      <input
        {...rest}
        type={type}
        value={value}
        disabled={disabled}
        onChange={(e) => {
          onChange(e.target.value);
        }}
        style={{
          ...fieldStyle(t, disabled ?? false),
          height: t.control,
          padding: `0 ${String(t.sp8)}px`,
        }}
      />
    </label>
  );
}

export interface TextAreaProps extends Omit<
  ComponentPropsWithRef<"textarea">,
  "value" | "onChange" | "style"
> {
  /** The visible label above the field (and its accessible name). */
  label: string;
  value: string;
  onChange: (value: string) => void;
  /** Field height in px. Default 96. */
  height?: number;
  /** Layout extras for the wrapper (width, flex). */
  style?: CSSProperties;
}

export function TextArea({
  label,
  value,
  onChange,
  height = 96,
  style,
  disabled,
  ...rest
}: TextAreaProps) {
  const { t } = useTheme();
  return (
    <label
      style={{
        display: "flex",
        flexDirection: "column",
        gap: t.sp4,
        fontFamily: t.fontSans,
        ...style,
      }}
    >
      <span style={fieldLabelStyle(t)}>{label}</span>
      <textarea
        {...rest}
        value={value}
        disabled={disabled}
        onChange={(e) => {
          onChange(e.target.value);
        }}
        style={{
          ...fieldStyle(t, disabled ?? false),
          height,
          padding: `${String(t.sp6)}px ${String(t.sp8)}px`,
          lineHeight: `${String(t.lineBody)}px`,
          resize: "none",
        }}
      />
    </label>
  );
}

// ===========================================================================
// Notice — the one inline message: a tinted block in the content (info grey with a
// `border` edge, caution `cautionFill`, danger `dangerFill`), `radiusPanel`, a 15 px
// tone icon, 13 px text with an optional bold first sentence, at most one action
// (a link or a secondary `sm` Button) and an optional close. One per screen at most.
// ===========================================================================

export type NoticeTone = "info" | "caution" | "danger";

export interface NoticeProps {
  /** Default "info". */
  tone?: NoticeTone;
  /** Optional bold first sentence. */
  title?: ReactNode;
  children?: ReactNode;
  /** The one action (a link Button or a secondary `sm` Button). */
  action?: ReactNode;
  /** Adds a close (×) button. */
  onClose?: () => void;
  /** Outer layout (margins). */
  style?: CSSProperties;
}

function noticePaint(
  t: ThemeTokens,
  tone: NoticeTone,
): { bg: string; edge: string; icon: IconName; iconColor: string } {
  switch (tone) {
    case "info":
      return {
        bg: t.groupHeader,
        edge: t.border,
        icon: "info",
        iconColor: t.textSecondary,
      };
    case "caution":
      return {
        bg: t.cautionFill,
        edge: "transparent",
        icon: "warn-tri",
        iconColor: t.caution,
      };
    case "danger":
      return {
        bg: t.dangerFill,
        edge: "transparent",
        icon: "alert",
        iconColor: t.danger,
      };
  }
}

export function Notice({
  tone = "info",
  title,
  children,
  action,
  onClose,
  style,
}: NoticeProps) {
  const { t } = useTheme();
  const p = noticePaint(t, tone);
  return (
    <div
      role={tone === "danger" ? "alert" : "status"}
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: t.sp8,
        padding: `${String(t.sp8)}px ${String(t.sp12)}px`,
        borderRadius: t.radiusPanel,
        background: p.bg,
        border: `1px solid ${p.edge}`,
        boxSizing: "border-box",
        fontFamily: t.fontSans,
        fontSize: t.fontBody,
        lineHeight: `${String(t.lineBody)}px`,
        color: t.text,
        ...style,
      }}
    >
      <span style={{ display: "flex", paddingTop: t.sp2, flexShrink: 0 }}>
        <Icon name={p.icon} size={15} stroke={p.iconColor} strokeWidth={2} />
      </span>
      <div
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          flexDirection: "column",
          gap: t.sp2,
        }}
      >
        {title != null && (
          <span style={{ fontWeight: t.weightSemibold }}>{title}</span>
        )}
        {children != null && <span>{children}</span>}
      </div>
      {action != null && (
        <span style={{ display: "flex", flexShrink: 0 }}>{action}</span>
      )}
      {onClose && (
        <Button
          variant="plain"
          size="sm"
          icon="x"
          aria-label="Close"
          onClick={onClose}
        />
      )}
    </div>
  );
}

// ===========================================================================
// SegmentedControl — one choice among two to six, all visible: a `segmentTrack`
// track with 2 px padding (`radiusPanel`); the selected segment is white with
// `shadowSegment` and semibold text. `sm` is the compact 12 px form (a sort control).
// A segment can be disabled (greyed, not clickable). A connected single-choice radio
// group. The fill is set via the backgroundColor LONGHAND with NO transition: the
// `background` shorthand + transition combo was observed to leave the computed fill
// stuck on the previously selected segment.
// ===========================================================================

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  /** Optional leading icon. */
  icon?: IconName;
  /** Can't be chosen right now (Catalog's "Stereo" in a scope with no stereo model):
   *  greyed text, and a click does nothing. A selected segment is never disabled by
   *  its caller, so a chosen filter can always be changed back. */
  disabled?: boolean;
}

export interface SegmentedControlProps<T extends string> {
  options: SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  /** Required: a role="radiogroup" needs an accessible name. */
  ariaLabel: string;
  /** `sm` = 12 px segments (a sort or filter control). Default `md` (13 px). */
  size?: "sm" | "md";
}

export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  ariaLabel,
  size = "md",
}: SegmentedControlProps<T>) {
  const { t } = useTheme();
  const sm = size === "sm";
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      style={{
        display: "inline-flex",
        alignSelf: "flex-start",
        gap: t.sp2,
        padding: t.sp2,
        borderRadius: t.radiusPanel,
        background: t.segmentTrack,
        maxWidth: "100%",
        boxSizing: "border-box",
      }}
    >
      {options.map((o) => {
        const on = o.value === value;
        const off = o.disabled === true;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={on}
            disabled={off}
            onClick={() => {
              onChange(o.value);
            }}
            style={{
              appearance: "none",
              border: "none",
              margin: 0,
              cursor: off ? "default" : "pointer",
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              gap: t.sp6,
              borderRadius: t.radiusControl,
              fontFamily: t.fontSans,
              fontSize: sm ? t.fontCaption : t.fontBody,
              lineHeight: `${String(sm ? t.lineCaption : t.lineBody)}px`,
              fontWeight: on ? t.weightSemibold : t.weightMedium,
              whiteSpace: "nowrap",
              padding: sm
                ? `${String(t.sp2 + 1)}px ${String(t.sp8 + 2)}px`
                : `${String(t.sp4)}px ${String(t.sp12)}px`,
              color: off ? t.textDisabled : t.text,
              backgroundColor: on ? t.surface : "transparent",
              boxShadow: on ? t.shadowSegment : "none",
            }}
          >
            {o.icon ? (
              <Icon
                name={o.icon}
                size={12}
                stroke={off ? t.textDisabled : on ? t.text : t.textSecondary}
              />
            ) : null}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

// ===========================================================================
// Slider — a 4 px `track` with an `accent` fill and an 18 px white thumb, "Quieter"
// and "Louder" at the ends, the value on the right (Settings → Targets). Pointer-driven
// (click-only app: no key handling), exposed as role="slider" with min/max/now. The
// track captures its pointer: moves and the release land on it, and a pointercancel
// (or a lost capture) ends the drag WITHOUT committing, restoring the start value.
// ===========================================================================

export interface SliderProps {
  value: number;
  min: number;
  max: number;
  /** Value resolution. Default 0.1. */
  step?: number;
  /** Accessible name ("Rhythm target"). */
  label: string;
  /** The value as words, drawn on the right and given as `aria-valuetext`. */
  valueText: string;
  /** Live value during a drag (does NOT persist). */
  onChange: (value: number) => void;
  /** Final value on release (persist once). */
  onCommit: (value: number) => void;
  /** Track width in px; it shrinks down to 160 when the row is narrow. Default 250. */
  width?: number;
  /** End captions. Default "Quieter" / "Louder". */
  minLabel?: string;
  maxLabel?: string;
}

export function Slider({
  value,
  min,
  max,
  step = 0.1,
  label,
  valueText,
  onChange,
  onCommit,
  width = 250,
  minLabel = "Quieter",
  maxLabel = "Louder",
}: SliderProps) {
  const { t } = useTheme();
  const trackRef = useRef<HTMLSpanElement | null>(null);
  // The in-progress drag: which pointer owns it and the value it started from
  // (restored on a cancel). null = no drag.
  const dragRef = useRef<{ pointerId: number; start: number } | null>(null);
  const span = max - min;
  const pct =
    span > 0 ? Math.min(100, Math.max(0, ((value - min) / span) * 100)) : 0;

  function valueAt(clientX: number): number {
    const el = trackRef.current;
    if (!el) return value;
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0) return value;
    const ratio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    const raw = min + ratio * span;
    // Snap to the step, then drop float noise (−23.000000004 → −23).
    return Math.round(Math.round(raw / step) * step * 1e6) / 1e6;
  }

  function onDown(e: PointerEvent<HTMLSpanElement>) {
    e.preventDefault();
    dragRef.current = { pointerId: e.pointerId, start: value };
    // Absent in jsdom, hence the guard.
    if ("setPointerCapture" in e.currentTarget)
      e.currentTarget.setPointerCapture(e.pointerId);
    onChange(valueAt(e.clientX));
  }
  function onMove(e: PointerEvent<HTMLSpanElement>) {
    if (dragRef.current?.pointerId !== e.pointerId) return;
    onChange(valueAt(e.clientX));
  }
  function onUp(e: PointerEvent<HTMLSpanElement>) {
    if (dragRef.current?.pointerId !== e.pointerId) return;
    dragRef.current = null;
    onCommit(valueAt(e.clientX));
  }
  function onCancel(e: PointerEvent<HTMLSpanElement>) {
    const drag = dragRef.current;
    if (drag?.pointerId !== e.pointerId) return;
    dragRef.current = null;
    onChange(drag.start);
  }

  const caption: CSSProperties = {
    fontSize: t.fontCaption,
    color: t.textSecondary,
    whiteSpace: "nowrap",
  };
  return (
    <span
      style={{
        display: "flex",
        alignItems: "center",
        gap: t.sp8 + 2,
        minWidth: 0,
        fontFamily: t.fontSans,
      }}
    >
      <span style={caption}>{minLabel}</span>
      <span
        ref={trackRef}
        role="slider"
        aria-label={label}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={value}
        aria-valuetext={valueText}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={onCancel}
        onLostPointerCapture={onCancel}
        style={{
          position: "relative",
          display: "block",
          width,
          minWidth: 160,
          flexShrink: 1,
          height: 18,
          cursor: "pointer",
          touchAction: "none",
        }}
      >
        <span
          style={{
            position: "absolute",
            left: 0,
            right: 0,
            top: 7,
            height: 4,
            borderRadius: 2,
            background: t.track,
          }}
        />
        <span
          style={{
            position: "absolute",
            left: 0,
            width: `${String(pct)}%`,
            top: 7,
            height: 4,
            borderRadius: 2,
            background: t.accent,
          }}
        />
        <span
          style={{
            position: "absolute",
            left: `calc(${String(pct)}% - 9px)`,
            top: 0,
            width: 18,
            height: 18,
            boxSizing: "border-box",
            borderRadius: t.radiusPill,
            background: t.surface,
            border: `1px solid ${t.controlBorder}`,
            boxShadow: t.shadowSegment,
          }}
        />
      </span>
      <span style={caption}>{maxLabel}</span>
      <span
        style={{
          width: 56,
          flexShrink: 0,
          textAlign: "right",
          fontSize: t.fontBody,
          color: t.text,
          fontVariantNumeric: "tabular-nums",
          whiteSpace: "nowrap",
        }}
      >
        {valueText}
      </span>
    </span>
  );
}
