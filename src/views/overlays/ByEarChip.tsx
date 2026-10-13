// src/views/overlays/ByEarChip.tsx — the "by ear" chip, shared by the leveling wizard.
//
// One consistent chip (the cause distinction lives in the engine + the Summary
// footnote, never in the chip wording). Shown in the Set-up Run-option caveat, on
// flagged Summary rows, and in the Summary's reason-aware footnote.

import { Tag } from "../../ui/Tag";

export function ByEarChip() {
  return <Tag tone="accent">by ear</Tag>;
}

export default ByEarChip;
