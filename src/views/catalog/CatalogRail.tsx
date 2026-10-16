// src/views/catalog/CatalogRail.tsx — the Catalog's sidebar (look B): "All models", then
// every category in the unit's order with its count, the nine Effects subcategories
// always listed under Effects (indented), labels in sentence case. Choosing an item
// scopes the wall (`useCatalogFilter.choose`).

import {
  CAT_COUNT,
  CAT_ORDER,
  SUB_COUNT,
  SUB_ORDER,
  TOTAL,
} from "../../models/catalog";
import { RailItem } from "../../ui/Rail";
import { sentenceCase } from "./catalogLabels";
import type { Sel } from "./useCatalogFilter";

// A category or subcategory with no available model has no entry.
const catCount = CAT_COUNT as Partial<Record<string, number>>;
const subCount = SUB_COUNT as Partial<Record<string, number>>;

export interface CatalogRailProps {
  sel: Sel;
  choose: (cat: string, sub?: string | null) => void;
}

export function CatalogRail({ sel, choose }: CatalogRailProps) {
  return (
    <>
      <RailItem
        label="All models"
        meta={TOTAL}
        active={sel.cat === "all"}
        onClick={() => {
          choose("all");
        }}
      />
      {CAT_ORDER.map((c) => (
        <CategoryItems key={c} cat={c} sel={sel} choose={choose} />
      ))}
    </>
  );
}

interface CategoryItemsProps extends CatalogRailProps {
  cat: string;
}

function CategoryItems({ cat, sel, choose }: CategoryItemsProps) {
  const item = (
    <RailItem
      label={sentenceCase(cat)}
      meta={catCount[cat] ?? 0}
      active={sel.cat === cat && sel.sub === null}
      onClick={() => {
        choose(cat);
      }}
    />
  );
  if (cat !== "Effects") return item;
  return (
    <>
      {item}
      {SUB_ORDER.map((s) => (
        <RailItem
          key={s}
          indent
          label={s}
          meta={subCount[s] ?? 0}
          active={sel.cat === "Effects" && sel.sub === s}
          onClick={() => {
            choose("Effects", s);
          }}
        />
      ))}
    </>
  );
}
