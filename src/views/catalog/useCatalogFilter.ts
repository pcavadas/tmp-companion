// src/views/catalog/useCatalogFilter.ts — the Catalog's filter state and the wall it
// derives: the search (deferred, so each keystroke paints the input first), the sidebar
// scope (category / Effects subcategory), the effect type, the Sound (All / Mono /
// Stereo) and Convolution filters, the Type/CPU sort and its direction, and the chosen
// model. Filters compose: e.g. Effects → Stompboxes → Fuzz, or Effects + Stereo.

import { useDeferredValue, useMemo, useState } from "react";

import {
  MODELS,
  CAT_ORDER,
  SUB_ORDER,
  CAB_SIZE_ORDER,
  cabSize,
  etSort,
  etypesFor,
  displayName,
  type ModelRecord,
} from "../../models/catalog";
import { sentenceCase } from "./catalogLabels";

export type Routing = "all" | "mono" | "stereo";
export type Sort = "type" | "cpu";
export type CpuDir = "desc" | "asc";
export interface Sel {
  cat: string;
  sub: string | null;
}

/** One section of the wall, under its label. */
export interface Group {
  key: string;
  label: string;
  blocks: ModelRecord[];
}

/** A to Z by the name the tile shows. */
const byName = (a: ModelRecord, b: ModelRecord) =>
  displayName(a).localeCompare(displayName(b));

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

  // Choosing a sidebar scope resets the effect type, clears the chosen model, and drops
  // the Convolution filter unless still in a convolution-relevant scope.
  function choose(cat: string, sub?: string | null) {
    setSel({ cat, sub: sub ?? null });
    setEt(null);
    if (!(cat === "Effects" && (sub === "Reverb" || !sub))) setConvOnly(false);
    setPick(null);
  }

  // One pass over the category/subcategory scope, yielding both the scoped list
  // and whether it contains any stereo model (drives the Stereo segment's enabled
  // state). Both only change when `sel` changes — search/filters don't touch them.
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
    // CPU sort overrides grouping — one flat, ranked section ("Highest CPU first").
    // Uncosted models (mics / FX-loop markers, cpu null) sort to the bottom
    // regardless of direction, alphabetical among themselves.
    if (sort === "cpu") {
      const sorted = [...shown].sort((a, b) => {
        if (a.cpu == null && b.cpu == null) return byName(a, b);
        if (a.cpu == null) return 1;
        if (b.cpu == null) return -1;
        return cpuDir === "desc" ? b.cpu - a.cpu : a.cpu - b.cpu;
      });
      const label =
        cpuDir === "desc" ? "Highest CPU first" : "Lowest CPU first";
      return [{ key: "_cpu", label, blocks: sorted }];
    }
    if (sel.cat === "Effects" && sel.sub) {
      const types = [
        ...new Set(shown.map((r) => r.et).filter((x): x is string => !!x)),
      ].sort(etSort);
      const g: Group[] = types.map((ty) => ({
        key: ty,
        label: sentenceCase(ty),
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
        label: sentenceCase(c),
        blocks: shown.filter((r) => r.cat === c),
      })).filter((x) => x.blocks.length);
    }
    return [
      {
        key: sel.cat,
        label: sentenceCase(sel.cat),
        blocks: [...shown].sort(byName),
      },
    ];
  }, [shown, sel, sort, cpuDir]);

  // The Effect type dropdown, inside an Effects subcategory with more than one type.
  const etypes = sel.cat === "Effects" && sel.sub ? etypesFor(sel.sub) : [];
  const showEt = etypes.length > 1;
  const showConv = sel.cat === "Effects" && (sel.sub === "Reverb" || !sel.sub);
  // Any filter narrowing the chosen scope: the count then reads "{n} of {scope}".
  const filtered =
    deferredQ.trim() !== "" || routing !== "all" || convOnly || et !== null;
  const scopeTotal = inScope.length;

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
    etypes,
    showEt,
    showConv,
    filtered,
    scopeTotal,
  };
}
