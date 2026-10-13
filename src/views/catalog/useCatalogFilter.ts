// src/views/catalog/useCatalogFilter.ts — the Models tab's filter state and the wall it
// derives: the search (deferred, so each keystroke paints the input first), the rail
// scope (category / Effects subcategory), the effect-type chip, the Mono/Stereo and
// Convolution facets, the Type/CPU sort, and the picked model. Filters compose: e.g.
// Effects → Stompboxes → Fuzz, or Effects + Stereo.

import { useDeferredValue, useMemo, useState } from "react";

import {
  MODELS,
  CAT_ORDER,
  SUB_ORDER,
  CAB_SIZE_ORDER,
  cabSize,
  etSort,
  etypesFor,
  type ModelRecord,
} from "../../models/catalog";

export type Routing = "all" | "mono" | "stereo";
export type Sort = "type" | "cpu";
export type CpuDir = "desc" | "asc";
export interface Sel {
  cat: string;
  sub: string | null;
}

/** One sticky section of the wall. */
export interface Group {
  key: string;
  label: string;
  blocks: ModelRecord[];
}

export function useCatalogFilter() {
  const [q, setQ] = useState("");
  // The wall filters on the DEFERRED query, so each keystroke paints the input
  // first and the (expensive) card wall catches up at transition priority.
  const deferredQ = useDeferredValue(q);
  const [sel, setSel] = useState<Sel>({ cat: "all", sub: null });
  const [et, setEt] = useState<string | null>(null);
  const [routing, setRouting] = useState<Routing>("all");
  const [convOnly, setConvOnly] = useState(false);
  const [sort, setSort] = useState<Sort>("type");
  const [cpuDir, setCpuDir] = useState<CpuDir>("desc");
  const [pick, setPick] = useState<ModelRecord | null>(null);

  // Selecting a rail category resets the chip, clears the pick, and drops the
  // convolution facet unless still in a convolution-relevant scope.
  function choose(cat: string, sub?: string | null) {
    setSel({ cat, sub: sub ?? null });
    setEt(null);
    if (!(cat === "Effects" && (sub === "Reverb" || !sub))) setConvOnly(false);
    setPick(null);
  }

  // One pass over the category/subcategory scope, yielding both the scoped list
  // and whether it contains any stereo model (drives the Stereo facet's enabled
  // state). Both only change when `sel` changes — search/facets don't touch them.
  const { inScope, hasStereo } = useMemo(() => {
    const scope = MODELS.filter((r) => {
      if (sel.cat !== "all" && r.cat !== sel.cat) return false;
      if (sel.cat === "Effects" && sel.sub && r.sub !== sel.sub) return false;
      return true;
    });
    return { inScope: scope, hasStereo: scope.some((r) => r.ch === "stereo") };
  }, [sel]);

  const shown = useMemo(() => {
    const needle = deferredQ.trim().toLowerCase();
    return inScope.filter((r) => {
      if (et && r.et !== et) return false;
      if (routing === "mono" && r.ch !== "mono") return false;
      if (routing === "stereo" && r.ch !== "stereo") return false;
      if (convOnly && !r.conv) return false;
      if (needle && !r.search.includes(needle)) return false;
      return true;
    });
  }, [inScope, et, routing, convOnly, deferredQ]);

  const groups: Group[] = useMemo(() => {
    // CPU sort overrides grouping — one flat, ranked "By CPU usage" wall.
    // Uncosted models (mics / FX-loop markers, cpu null) sort to the bottom
    // regardless of direction, alphabetical among themselves.
    if (sort === "cpu") {
      const sorted = [...shown].sort((a, b) => {
        if (a.cpu == null && b.cpu == null) return a.name.localeCompare(b.name);
        if (a.cpu == null) return 1;
        if (b.cpu == null) return -1;
        return cpuDir === "desc" ? b.cpu - a.cpu : a.cpu - b.cpu;
      });
      return [{ key: "_cpu", label: "By CPU usage", blocks: sorted }];
    }
    if (sel.cat === "Effects" && sel.sub) {
      const types = [
        ...new Set(shown.map((r) => r.et).filter((x): x is string => !!x)),
      ].sort(etSort);
      const g: Group[] = types.map((ty) => ({
        key: ty,
        label: ty,
        blocks: shown.filter((r) => r.et === ty),
      }));
      const none = shown.filter((r) => !r.et);
      if (none.length) g.push({ key: "_", label: sel.sub, blocks: none });
      return g;
    }
    if (sel.cat === "Effects" && !sel.sub) {
      return SUB_ORDER.map((s) => ({
        key: s,
        label: s,
        blocks: shown.filter((r) => r.sub === s),
      })).filter((x) => x.blocks.length);
    }
    if (sel.cat === "Cabinets") {
      const map = new Map<string, ModelRecord[]>();
      shown.forEach((r) => {
        const s = cabSize(r.name);
        let bucket = map.get(s);
        if (!bucket) {
          bucket = [];
          map.set(s, bucket);
        }
        bucket.push(r);
      });
      const rank = (k: string) => {
        const i = CAB_SIZE_ORDER.indexOf(k);
        return i === -1 ? 999 : i;
      };
      const keys = [...map.keys()].sort(
        (a, b) => rank(a) - rank(b) || a.localeCompare(b),
      );
      return keys.map((k) => ({
        key: k,
        label: k.replace("X", "×"),
        blocks: map.get(k) ?? [],
      }));
    }
    if (sel.cat === "all") {
      return CAT_ORDER.map((c) => ({
        key: c,
        label: c,
        blocks: shown.filter((r) => r.cat === c),
      })).filter((x) => x.blocks.length);
    }
    return [
      {
        key: sel.cat,
        label: sel.cat,
        blocks: [...shown].sort((a, b) => a.name.localeCompare(b.name)),
      },
    ];
  }, [shown, sel, sort, cpuDir]);

  const etChips = sel.cat === "Effects" && sel.sub ? etypesFor(sel.sub) : null;
  const showConv = sel.cat === "Effects" && (sel.sub === "Reverb" || !sel.sub);

  return {
    q,
    setQ,
    deferredQ,
    sel,
    choose,
    et,
    setEt,
    routing,
    setRouting,
    convOnly,
    setConvOnly,
    sort,
    setSort,
    cpuDir,
    setCpuDir,
    pick,
    setPick,
    hasStereo,
    shown,
    groups,
    etChips,
    showConv,
  };
}
