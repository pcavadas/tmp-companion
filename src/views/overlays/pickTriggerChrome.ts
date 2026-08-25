// src/views/overlays/pickTriggerChrome.ts — the one bit of trigger CHROME shared
// byte-for-byte across `Pick`/`FsParamPick`/`SceneLevelPick`: the 3-way border
// color (open > warn > default). Each picker's own WARN PREDICATE stays local
// (they operate on different shapes — `Pick`'s "stored value not in options",
// `FsParamPick`'s "no valid selection yet", `SceneLevelPick`'s "stored handle not
// in the fetched candidates" — unifying those would hide real per-picker meaning
// behind a shared name). Only the resulting border rule is identical, so only it
// is shared.

import type { ThemeTokens } from "../../theme/tokens";

/** The trigger's 0.5px border color: open beats warn beats the resting hairline. */
export function pickTriggerBorder(
  t: ThemeTokens,
  { open, warn }: { open: boolean; warn: boolean },
): string {
  return `0.5px solid ${open ? t.accent : warn ? t.sevWarn : t.hairlineStrong}`;
}
