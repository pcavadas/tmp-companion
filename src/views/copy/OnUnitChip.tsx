// src/views/copy/OnUnitChip.tsx — the green "ON UNIT" chip of the Copy steps.
// A private feature-shared module (NOT in the barrel; never imports a sub-component).

import { Tag } from "../../ui/Tag";

/** The green "ON UNIT" chip — the preset currently loaded on the device. */
export function OnUnitChip() {
  return <Tag tone="good">ON UNIT</Tag>;
}
