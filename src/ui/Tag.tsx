// src/ui/Tag.tsx — the old chip, now a shim over the look B `Badge` so the screens
// that still render <Tag> compile and look B. Its old props have no Badge equivalent
// and map as follows: `warn` → caution, `neutralFill` → neutral, a custom `fg` colour
// → neutral (B badges carry no per-site colour), and `size` / `uppercase` are ignored
// (one size; sentence case — the child string is shown as written). Removed in the
// clean-up step, once every call site renders Badge (or caption text) directly.

import type { ReactNode } from "react";

import { Badge, type BadgeTone } from "./Badge";

export type TagTone = "accent" | "good" | "warn" | "neutral" | "neutralFill";
export type TagSize = "sm" | "md";

export interface TagProps {
  children: ReactNode;
  /** default "neutral". */
  tone?: TagTone;
  /** Ignored: a Badge has one size. */
  size?: TagSize;
  /** Ignored: look B never uppercases. */
  uppercase?: boolean;
  /** A custom colour has no Badge equivalent: the badge renders neutral. */
  fg?: string;
}

const TAG_TONE: Record<TagTone, BadgeTone> = {
  accent: "accent",
  good: "good",
  warn: "caution",
  neutral: "neutral",
  neutralFill: "neutral",
};

export function Tag({ children, tone = "neutral", fg }: TagProps) {
  return <Badge tone={fg ? "neutral" : TAG_TONE[tone]}>{children}</Badge>;
}
