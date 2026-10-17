// src/ui/InlineEdit.tsx — the ONE inline text editor (#314): Songs' setlist-title
// rename, Settings' target-name rename, and Songs' new-setlist field.
//
// It owns its draft (seeded from `initial` on mount — callers mount it only while
// editing, so a remount is a fresh edit). Enter commits, blur commits unless the
// caller opts out (`commitOnBlur={false}`: the new-setlist field, whose ✓/✕
// buttons — `commitLabel` — sit beside it), and Escape cancels. Enter/Escape here
// are the CLAUDE.md text-field carve-out: they act only while THIS input has
// focus, on THIS field's own edit — never a shortcut. The first commit or cancel ends the edit, so a blur
// that follows an Enter/Escape (the input unmounting) can't fire a second one.
//
// Validation stays with the caller: `onCommit` receives the TRIMMED draft and the
// caller decides (an empty or unchanged name just closes the editor).

import { useRef, useState, type CSSProperties } from "react";

import { useTheme } from "../theme/ThemeContext";
import { plainInput } from "../theme/tokens";
import { Icon } from "./Icon";
import { Button } from "./primitives";

export interface InlineEditProps {
  /** The draft's starting text (the current name; "" for a new item). */
  initial: string;
  /** Receives the trimmed draft on Enter, on blur (unless opted out), or on ✓. */
  onCommit: (value: string) => void;
  /** Escape, or ✕. */
  onCancel: () => void;
  /** Commit when the field loses focus. Default true. */
  commitOnBlur?: boolean;
  /** Select the whole draft on focus (rename a fresh row by just typing). */
  selectOnFocus?: boolean;
  placeholder?: string;
  ariaLabel?: string;
  /** The input's own style, over the `plainInput` reset. */
  style?: CSSProperties;
  /** Adds ✓ / ✕ buttons after the input — the field's click path — with this as
   *  the ✓'s accessible name (e.g. "Create"); the ✕ is "Cancel". With them, the
   *  input and the buttons share one wrapper styled by `wrapStyle`. */
  commitLabel?: string;
  wrapStyle?: CSSProperties;
}

export function InlineEdit({
  initial,
  onCommit,
  onCancel,
  commitOnBlur = true,
  selectOnFocus = false,
  placeholder,
  ariaLabel,
  style,
  commitLabel,
  wrapStyle,
}: InlineEditProps) {
  const { t } = useTheme();
  const [draft, setDraft] = useState(initial);
  // Set by the first commit/cancel; read only in event handlers.
  const ended = useRef(false);

  const commit = () => {
    if (ended.current) return;
    ended.current = true;
    onCommit(draft.trim());
  };
  const cancel = () => {
    if (ended.current) return;
    ended.current = true;
    onCancel();
  };

  const input = (
    <input
      autoFocus
      value={draft}
      placeholder={placeholder}
      aria-label={ariaLabel}
      onChange={(e) => {
        setDraft(e.target.value);
      }}
      onFocus={
        selectOnFocus
          ? (e) => {
              e.currentTarget.select();
            }
          : undefined
      }
      onBlur={commitOnBlur ? commit : undefined}
      onKeyDown={(e) => {
        // An IME's Enter confirms a candidate, not the field.
        if (e.nativeEvent.isComposing) return;
        if (e.key === "Enter") {
          e.preventDefault();
          commit();
        } else if (e.key === "Escape") {
          e.preventDefault();
          cancel();
        }
      }}
      style={plainInput(t, style)}
    />
  );

  if (commitLabel == null) return input;
  return (
    <div style={wrapStyle}>
      {input}
      <Button
        variant="bare"
        aria-label={commitLabel}
        onClick={commit}
        style={{ display: "flex" }}
      >
        <Icon name="check" size={14} stroke={t.link} strokeWidth={2} />
      </Button>
      <Button
        variant="bare"
        aria-label="Cancel"
        onClick={cancel}
        style={{ display: "flex" }}
      >
        <Icon name="x" size={13} stroke={t.textSecondary} strokeWidth={2} />
      </Button>
    </div>
  );
}
