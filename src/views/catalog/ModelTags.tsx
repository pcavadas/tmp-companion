// src/views/catalog/ModelTags.tsx — a catalog record's STEREO / CONV badges.

import type { ThemeTokens } from "../../theme/tokens";
import { Tag } from "../../ui/Tag";
import type { ModelRecord } from "../../models/catalog";

/** The STEREO / CONV badges for a record — shown on cards and in the detail bar. */
export interface ModelTagsProps {
  r: ModelRecord;
  t: ThemeTokens;
}

export function ModelTags({ r, t }: ModelTagsProps) {
  return (
    <>
      {r.ch === "stereo" && <Tag fg={t.badgeStereo}>STEREO</Tag>}
      {r.conv && <Tag fg={t.badgeConv}>CONV</Tag>}
    </>
  );
}
