// src/views/level/useSetupRows.ts — the Set up page's state and what every row shows.
//
// State: each sound's target (unset = "by name"), its chosen knob (`handle`) and, for a
// footswitch, the scene it is leveled in; one instrument per PRESET; and the presets whose
// two amps are balanced first. Everything a row shows is derived from that state plus the
// data the backup scan already read (candidates, scene drives, footswitch scene
// suggestions), so every dropdown shows exactly what the run will use. A preset's
// instrument and a sound's target are resolved at render from the user's own pick, kept
// only while its dropdown still offers it, else the saved instrument (or the target by
// name), else the default — so they follow the app's store when it loads after the page
// opened, and a dropdown never shows one thing while the run gets another (danger.md).
// With no target configured nothing can run: Start waits (`noTargets`), and the run's
// choices are never built with a target that isn't configured.

import { useEffect, useMemo, useState, useSyncExternalStore } from "react";

import { getLibraryScan, subscribeLibraryScan } from "../../device/libraryScan";
import type { LevelParamCandidate } from "../../lib/types";
import type { SelectOption } from "../../ui/selectValue";
import type { BlockLevelHandle } from "./blockLevelGroups";
import {
  baseCandidatesFetch,
  enablingBlockHandle,
  footswitchCandidatesFetch,
  sceneCandidatesFetch,
} from "./setupCandidates";
import {
  controlModel,
  sceneDefault,
  targetByName,
  type ControlModel,
} from "./setupControls";
import { useSceneHandles } from "./useSceneHandles";
import { useLevelBlocks } from "./useLevelBlocks";
import { useFootswitchSceneContexts } from "./useFootswitchSceneContexts";
import {
  MAIN_SOUND,
  buildSetupChoices,
  runRank,
  setupRowHookKey,
  type SetupChoice,
  type SetupOption,
} from "./leveling";

/** One sound's own choices. `undefined` = not chosen: the target goes by name, the knob
 *  is the row's default (or its preselect), the footswitch scene is the suggested one. A
 *  target that is no longer configured counts as not chosen. */
interface RowState {
  target: string | undefined;
  handle: BlockLevelHandle | null | undefined;
  sceneContext: number | null | undefined;
}

/** The "Level it in scene" choice of a footswitch row. */
export interface SceneChoiceModel {
  /** `""` none chosen (the suggestion couldn't be read), `"base"` the main sound, or the
   *  scene's 0-based wire index. */
  value: string;
  options: SelectOption[];
  /** The suggestion is still being read. */
  busy: boolean;
}

/** What Start waits for on a row, and the footer link that points at it. */
export interface RowNeed {
  which: "scene" | "block";
  text: string;
}

export interface SetupRowModel {
  option: SetupOption;
  /** e2e identity (`setupRowHookKey`). */
  hookKey: string;
  /** "footswitch 3" beside a footswitch's name. */
  caption: string | null;
  /** The target the run uses. */
  target: string;
  control: ControlModel;
  /** Footswitch rows of presets with scenes only. */
  scene: SceneChoiceModel | null;
  /** Notes under the row, one per line. */
  notes: string[];
  need: RowNeed | null;
}

export interface SetupGroupModel {
  slot: number;
  name: string;
  /** The preset's instrument (profile id or "none"). */
  inst: string;
  /** "Balance its two amps first" applies: the backend can rebalance this preset and
   *  one of its scenes is in the run. */
  canRebalance: boolean;
  rows: SetupRowModel[];
}

/** A "for all" dropdown's value: one shared value, or rows that differ. */
export interface ForAllValue {
  value: string;
  mixed: boolean;
}

/** "Target for all"'s own values: "By name", or one target. */
export const TARGET_BY_NAME = "by-name";
export const targetForAllValue = (name: string): string => `t:${name}`;

export interface UseSetupRowsArgs {
  options: SetupOption[];
  /** "None" + each instrument (`value` = profile id). */
  instrumentOptions: readonly SelectOption[];
  /** The configured targets' names, in their order. */
  targetNames: readonly string[];
  /** The instrument saved for each preset (0-based list index → profile id or "none"). */
  savedInstruments: Partial<Record<number, string>>;
  /** A preset's instrument when nothing is saved for it. */
  defaultInst: string;
  /** Remember a preset's instrument (the app's own store, never the unit). */
  onInstrumentChange?: (slot: number, id: string) => void;
}

const footswitchNumber = (o: SetupOption): number =>
  (o.footswitch?.switchIndex ?? 0) + 1;

export function useSetupRows({
  options,
  instrumentOptions,
  targetNames,
  savedInstruments,
  defaultInst,
  onInstrumentChange,
}: UseSetupRowsArgs) {
  const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan);

  // Presets in slot order, each with its sounds in run order (`chosenFrom`'s order).
  const presets = useMemo(() => {
    const by = new Map<
      number,
      { slot: number; name: string; opts: SetupOption[] }
    >();
    options.forEach((o) => {
      let g = by.get(o.slot);
      if (!g) {
        g = { slot: o.slot, name: o.presetName, opts: [] };
        by.set(o.slot, g);
      }
      g.opts.push(o);
    });
    return [...by.values()].sort((a, b) => a.slot - b.slot);
  }, [options]);

  const offersInst = (id: string | undefined): id is string =>
    id !== undefined && instrumentOptions.some((o) => o.value === id);

  const [rows, setRows] = useState<Partial<Record<string, RowState>>>(() => {
    const m: Partial<Record<string, RowState>> = {};
    options.forEach((o) => {
      m[o.key] = {
        target: undefined,
        handle:
          o.footswitch != null
            ? {
                groupId: o.footswitch.levGroupId,
                nodeId: o.footswitch.levNodeId,
                parameterId: o.footswitch.levParameterId,
              }
            : o.isBase
              ? (o.baseHandle ?? null)
              : (o.sceneHandle ?? undefined),
        // Unset: a footswitch resolves to the preset's suggested scene.
        sceneContext: undefined,
      };
    });
    return m;
  });
  const patchRow = (key: string, partial: Partial<RowState>) => {
    setRows((p) => {
      const cur = p[key];
      return cur ? { ...p, [key]: { ...cur, ...partial } } : p;
    });
  };

  // One instrument per preset: the user's pick, else the one saved for the preset, else
  // the default — only an instrument the dropdown still offers. Only the picks are state;
  // the rest is read at render, so the saved instruments apply once the store arrives.
  const [insts, setInsts] = useState<Partial<Record<number, string>>>({});
  const instOf = (slot: number): string => {
    const pick = insts[slot];
    if (offersInst(pick)) return pick;
    const saved = savedInstruments[slot];
    return offersInst(saved) ? saved : defaultInst;
  };

  const [rebalance, setRebalance] = useState<ReadonlySet<number>>(new Set());

  const {
    prefetch: fetchHandlesFor,
    candidatesFor,
    hasBackupData: sceneHasBackup,
  } = useSceneHandles();
  const {
    prefetch: fetchBlocksFor,
    blocksFor,
    hasBackupData: baseHasBackup,
  } = useLevelBlocks();
  const { prefetch: fetchFsContextFor, contextFor } =
    useFootswitchSceneContexts();

  // A footswitch's scenes and knobs ride its option (`chosenFrom`).
  const sceneNamesOf = (o: SetupOption): string[] => o.fsSceneNames ?? [];
  const levelParamsOf = (o: SetupOption): LevelParamCandidate[] =>
    o.levelParams ?? [];

  // Warm every preset's candidates the backup scan already holds (provably no device
  // read — `hasBackupData`), so the dropdowns open on their lists.
  useEffect(() => {
    presets.forEach((g) => {
      if (baseHasBackup(g.slot)) fetchBlocksFor(g.slot);
      if (sceneHasBackup(g.slot)) fetchHandlesFor(g.slot);
    });
  }, [presets, fetchBlocksFor, fetchHandlesFor, baseHasBackup, sceneHasBackup]);

  // A footswitch's default scene is the preset's suggested one, so resolve it for every
  // footswitch row of a preset with scenes as Set up opens — never on a dropdown click,
  // or the same selection could level a switch in different scenes. Answered off the
  // backup scan; a device read only for a preset the scan has no entry for.
  const fsSlots = useMemo(
    () => [
      ...new Set(
        options
          .filter(
            (o) => o.footswitch != null && (o.fsSceneNames ?? []).length > 0,
          )
          .map((o) => o.slot),
      ),
    ],
    [options],
  );
  useEffect(() => {
    fsSlots.forEach(fetchFsContextFor);
  }, [fsSlots, fetchFsContextFor]);

  /** The knob a row levels with: its choice, else its default — for a scene, the block
   *  the scene turns on (issue 5's preselect) before the amp. */
  const handleFor = (o: SetupOption): BlockLevelHandle | null => {
    const chosen = rows[o.key]?.handle;
    if (chosen !== undefined) return chosen;
    if (o.isBase) return o.baseHandle ?? null;
    if (o.footswitch == null && o.sceneSlot != null) {
      const st = candidatesFor(o.slot, o.sceneSlot);
      return (
        o.sceneHandle ??
        enablingBlockHandle(st, sceneCandidatesFetch(st)) ??
        null
      );
    }
    return null;
  };

  /** The suggested scene of a footswitch row, once read (`null` = the main sound). */
  const fsSuggestedFor = (o: SetupOption): number | null => {
    if (o.footswitch == null) return null;
    const st = contextFor(o.slot, o.footswitch.switchIndex);
    return st.status === "resolved" ? (st.row?.suggested ?? null) : null;
  };

  /** A row's own target pick while it is still configured (`undefined`: by name). */
  const targetPickOf = (o: SetupOption): string | undefined => {
    const pick = rows[o.key]?.target;
    return pick !== undefined && targetNames.includes(pick) ? pick : undefined;
  };
  /** The target a row runs at: its pick, else by name (`""` only when no target is
   *  configured). */
  const targetOf = (o: SetupOption): string =>
    targetPickOf(o) ?? targetByName(o.sceneName, targetNames);
  /** No target is configured, so no sound can run: Start waits for one. */
  const noTargets = targetNames.length === 0;

  const modelOf = (o: SetupOption): SetupRowModel => {
    const row = rows[o.key];
    const notes: string[] = [];
    let need: RowNeed | null = null;
    let scene: SceneChoiceModel | null = null;
    let control: ControlModel;
    const fsw = o.footswitch ?? null;

    if (fsw != null) {
      control = controlModel(
        footswitchCandidatesFetch(levelParamsOf(o)),
        handleFor(o),
        { kind: "none" },
      );
      const names = sceneNamesOf(o);
      if (names.length > 0) {
        const n = String(footswitchNumber(o));
        const labelOf = (i: number) => names[i] || `Scene ${String(i + 1)}`;
        const ctx = contextFor(o.slot, fsw.switchIndex);
        const chosen = row?.sceneContext;
        const reading =
          chosen === undefined &&
          (ctx.status === "unfetched" || ctx.status === "loading");
        const unread = chosen === undefined && ctx.status === "error";
        const value =
          chosen !== undefined
            ? chosen
            : ctx.status === "resolved"
              ? (ctx.row?.suggested ?? fsw.sceneContext)
              : undefined;
        scene = {
          value:
            value === undefined ? "" : value === null ? "base" : String(value),
          options: [
            { value: "base", label: MAIN_SOUND },
            ...names.map((_, i) => ({ value: String(i), label: labelOf(i) })),
          ],
          busy: reading,
        };
        if (unread) {
          notes.push(`Couldn't read which scene to level footswitch ${n} in.`);
          need = { which: "scene", text: `Choose a scene for footswitch ${n}` };
        }
        const enabling =
          ctx.status === "resolved" ? (ctx.row?.enablingScenes ?? null) : null;
        if (
          typeof value === "number" &&
          enabling != null &&
          !enabling.includes(value)
        )
          notes.push(
            `Footswitch ${n} doesn't turn its effect on in ${labelOf(value)}. Choose another scene for it.`,
          );
      }
    } else if (o.isBase) {
      control = controlModel(
        baseCandidatesFetch(blocksFor(o.slot)),
        handleFor(o),
        {
          kind: "preset",
        },
      );
    } else if (o.sceneSlot != null) {
      const def = sceneDefault(
        (lib.sceneDrivesByIndex.get(o.slot) ?? []).find(
          (d) => d.sceneSlot === o.sceneSlot,
        ),
      );
      control = controlModel(
        sceneCandidatesFetch(candidatesFor(o.slot, o.sceneSlot)),
        handleFor(o),
        def,
      );
      if (control.nothingToChoose)
        notes.push(
          "Nothing in this scene can be leveled, so its volume won't change.",
        );
      else if (control.needsBlock) {
        notes.push(
          def.kind === "none" && def.reason === "no_active_amp"
            ? "No amp found for this scene. Choose a block and knob for it."
            : "Couldn't confirm which amp to turn in this scene. Choose a block and knob for it.",
        );
        need = { which: "block", text: `Choose a block for ${o.sceneName}` };
      }
    } else {
      // A scene with no wire slot: nothing to level it with.
      control = controlModel({ status: "resolved", list: [] }, null, {
        kind: "none",
      });
    }
    if (control.stale)
      notes.unshift("This knob isn't in the preset any more. Choose another.");

    return {
      option: o,
      hookKey: setupRowHookKey(o),
      caption: fsw != null ? `footswitch ${String(footswitchNumber(o))}` : null,
      target: targetOf(o),
      control,
      scene,
      notes,
      need,
    };
  };

  // Rows in run order (`runRank`): a footswitch leveled in a scene runs after the scenes.
  // Ranked by the switch's DEFAULT scene (the suggestion), never by a later pick, so a row
  // never jumps away under the pointer.
  const rankOf = (o: SetupOption): number => {
    const fsw = o.footswitch;
    if (fsw == null) return runRank(o);
    const sceneContext =
      sceneNamesOf(o).length > 0
        ? (fsSuggestedFor(o) ?? fsw.sceneContext)
        : null;
    return runRank({ isBase: o.isBase, footswitch: { sceneContext } });
  };
  const groups: SetupGroupModel[] = presets.map((g) => ({
    slot: g.slot,
    name: g.name,
    inst: instOf(g.slot),
    canRebalance:
      lib.rebalanceableIndices.has(g.slot) &&
      g.opts.some((o) => o.footswitch == null && o.sceneSlot != null),
    rows: [...g.opts].sort((a, b) => rankOf(a) - rankOf(b)).map(modelOf),
  }));
  const allRows = groups.flatMap((g) => g.rows);

  /** A footswitch scene still being read: Start waits (usually instant). */
  const pending = allRows.some((r) => r.scene?.busy === true);
  /** The first row Start waits on, in run order. */
  const firstNeed = allRows.find((r) => r.need != null) ?? null;

  // "Target for all": "By name" while every row goes by name; one target when every row
  // lands on it; else "Mixed".
  const targetAll: ForAllValue = (() => {
    if (options.every((o) => targetPickOf(o) === undefined))
      return { value: TARGET_BY_NAME, mixed: false };
    const used = new Set(options.map(targetOf));
    if (used.size === 1) {
      const [only] = used;
      return { value: targetForAllValue(only), mixed: false };
    }
    return { value: "", mixed: true };
  })();
  const setAllTargets = (value: string) => {
    const name = value === TARGET_BY_NAME ? undefined : value.slice(2);
    setRows((p) => {
      const n = { ...p };
      options.forEach((o) => {
        const cur = n[o.key];
        if (cur) n[o.key] = { ...cur, target: name };
      });
      return n;
    });
  };

  // "Instrument for all": the presets' shared instrument, or "Mixed".
  const instAll: ForAllValue = (() => {
    const used = new Set(presets.map((g) => instOf(g.slot)));
    if (used.size === 1) {
      const [only] = used;
      return { value: only, mixed: false };
    }
    return { value: "", mixed: true };
  })();
  const setInst = (slot: number, id: string) => {
    setInsts((p) => ({ ...p, [slot]: id }));
    onInstrumentChange?.(slot, id);
  };
  const setAllInst = (id: string) => {
    setInsts((p) => {
      const n = { ...p };
      presets.forEach((g) => {
        n[g.slot] = id;
      });
      return n;
    });
    presets.forEach((g) => onInstrumentChange?.(g.slot, id));
  };

  const toggleRebalance = (slot: number) => {
    setRebalance((p) => {
      const n = new Set(p);
      if (n.has(slot)) n.delete(slot);
      else n.add(slot);
      return n;
    });
  };

  /** The run's choices from the rows as they stand (the Start commit), plus the presets
   *  whose two amps are balanced first. None at all while a row's target isn't a
   *  configured one: there would be no loudness to level it to. */
  const buildChoices = (): {
    choices: SetupChoice[];
    rebalance: number[];
  } => {
    if (options.some((o) => !targetNames.includes(targetOf(o))))
      return { choices: [], rebalance: [] };
    const choices = buildSetupChoices(
      options,
      (o) => ({
        inst: instOf(o.slot),
        target: targetOf(o),
        sceneContext: rows[o.key]?.sceneContext,
      }),
      handleFor,
      fsSuggestedFor,
    );
    return {
      choices,
      rebalance: groups
        .filter((g) => g.canRebalance && rebalance.has(g.slot))
        .map((g) => g.slot),
    };
  };

  return {
    groups,
    /** At least one chosen footswitch belongs to a preset with scenes. */
    sceneColumn: allRows.some((r) => r.scene != null),
    targetAll,
    instAll,
    rebalance,
    pending,
    firstNeed,
    noTargets,
    setTarget: (key: string, name: string) => {
      patchRow(key, { target: name });
    },
    setAllTargets,
    setInst,
    setAllInst,
    setHandle: (key: string, handle: BlockLevelHandle | null) => {
      patchRow(key, { handle });
    },
    setSceneContext: (key: string, value: number | null) => {
      patchRow(key, { sceneContext: value });
    },
    toggleRebalance,
    fetchBlocksFor,
    fetchHandlesFor,
    fetchFsContextFor,
    buildChoices,
  };
}
