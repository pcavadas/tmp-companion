// src/views/level/ContextFooter.tsx — the Level list's Footer, look B.
//
// Left: "How it works" (a plain button that opens the How leveling works dialog). Right:
// a caption, then the one main action, which is ALWAYS there and counts SOUNDS:
//   • nothing chosen (or the list still loading) — "Choose presets or sounds to level"
//     beside a greyed "Level";
//   • the backup scan still reading — "You can start when reading finishes" beside a busy,
//     greyed "Level", whatever is already chosen;
//   • sounds chosen — "2 presets" beside "Level 6 sounds".
// No loudness numbers here: targets live in Settings.

import { Button } from "../../ui/primitives";
import { Footer } from "../../ui/Footer";
import { useTheme } from "../../theme/ThemeContext";

export interface ContextFooterProps {
  /** The preset list itself is still arriving. */
  loading: boolean;
  /** Background scene load settled — until then "Level" waits, busy. */
  ready: boolean;
  /** Distinct presets with any chosen sound. */
  presetCount: number;
  /** Chosen sounds (the main sound, each scene, each footswitch). */
  soundCount: number;
  onLevel: () => void;
  /** Opens the "How leveling works" dialog. */
  onHelp: () => void;
}

export function ContextFooter({
  loading,
  ready,
  presetCount,
  soundCount,
  onLevel,
  onHelp,
}: ContextFooterProps) {
  const { t } = useTheme();
  const captionStyle = {
    fontSize: t.fontCaption,
    color: t.textSecondary,
    whiteSpace: "nowrap",
  } as const;

  let right;
  if (!loading && !ready) {
    right = (
      <>
        <span style={captionStyle}>You can start when reading finishes</span>
        <Button variant="secondary" size="lg" busy>
          Level
        </Button>
      </>
    );
  } else if (loading || soundCount === 0) {
    right = (
      <>
        <span style={captionStyle}>Choose presets or sounds to level</span>
        <Button variant="primary" size="lg" disabled>
          Level
        </Button>
      </>
    );
  } else {
    right = (
      <>
        <span style={captionStyle}>
          {`${String(presetCount)} ${presetCount === 1 ? "preset" : "presets"}`}
        </span>
        <Button variant="primary" size="lg" onClick={onLevel}>
          {`Level ${String(soundCount)} ${soundCount === 1 ? "sound" : "sounds"}`}
        </Button>
      </>
    );
  }

  return (
    <Footer
      left={
        <Button variant="plain" onClick={onHelp}>
          How it works
        </Button>
      }
      right={right}
    />
  );
}
