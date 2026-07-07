// src/views/level/LevelView.tsx — the Level view (scene-tree selection).
//
// Layout: hero signal-path strip (live) → preset list (instant) + a background
// scene-scan strip (the whole library via one ~22 s device backup) → selection
// footer → the leveling flow (setup → run → summary).
//
// The list is a SCENE TREE: each preset row is a parent of its scenes (Base + each
// footswitch scene). The row checkbox selects the whole preset, the caret reveals the
// scenes for individual selection, and clicking the row body toggles that preset's
// selection (it does NOT recall the preset on the unit — app-driven recall was removed;
// recall is owned by Pro Control / the footswitches). The setup dialog only configures
// (instrument + target) what the list picked;
// its footer's backup acknowledgment gates the "Level" commit.
//
// HARD RULES: every device WRITE (leveling save) fires only after the backup
// acknowledgment, in the run. Reads (list / store / the backup scene read) + recalls
// are the only other device touches. No fabricated data; click-only.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { LoadErrorPane } from "../LoadErrorPane";
import { usePresetData } from "./usePresetData";
import { useLiveDevice } from "./useLiveDevice";
import { useLevelingFlow } from "./useLevelingFlow";
import { ContextFooter } from "./ContextFooter";
import { ActiveSignalChainView, type SceneTag } from "../ActiveSignalChainView";
import { PresetList } from "../PresetList";
import { EmptyState, UsbC } from "../EmptyState";
import { LevelingWizard } from "../overlays";
import { HowLevelingSheet, LevelingInfoRow } from "./HowLevelingSheet";
import { instrumentOptions, instrumentName } from "./leveling";
import type { PickOption } from "../overlays/Pick";
import type { ActiveGraph } from "../../lib/types";
import { currentGraph, readActivePreset } from "../../lib/invoke";

// Grace window after a recall for the signal-chain push to land before the UI
// concludes the picture is stale (the diagram-fail state).
const DIAGRAM_GRACE_MS = 1400;
// A scene event's graph may arrive BEFORE the scene push (the unit's tap burst is
// field-3 → SceneLoaded); a graph no older than this slack at scene-event time
// counts as the recall's redraw, so only a genuinely older picture trips the
// diagram-fail watchdog.
const GRAPH_FRESH_SLACK_MS = 1000;

export interface LevelViewProps {
  connected: boolean;
  onScan?: () => void;
  initialGraph?: ActiveGraph | null;
}

export function LevelView({ connected, onScan, initialGraph }: LevelViewProps) {
  const { t } = useTheme();

  const {
    phase,
    rows,
    store,
    sel,
    pendingWhole,
    expanded,
    ready,
    presetCount,
    sceneCount,
    sceneInfo,
    footswitchInfo,
    ampCandidates,
    blocksByIndex,
    graphByIndex,
    scan,
    togglePreset,
    toggleKey,
    toggleExpand,
    deselectKeys,
    toggleAll,
    targetLufsByName,
    refresh,
  } = usePresetData(connected);

  const live = useLiveDevice(connected);

  const [filter, setFilter] = useState("");
  const [liveScene, setLiveScene] = useState<"base" | number | null>(null);
  const [diagError, setDiagError] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false); // "How leveling works" sheet
  // Prefer the LIVE graph over the connect-time seed: on a tab-switch remount the
  // live store still holds the unit's CURRENT preset, while `initialGraph` is the
  // stale handshake graph — seeding from it would revert the hero to that preset.
  const [heroGraph, setHeroGraph] = useState<ActiveGraph | null>(
    live.graph ?? initialGraph ?? null,
  );
  const lastGraphAtRef = useRef(0);

  // The active slot derives from the live snapshot (falling back to the connect-
  // time handshake graph's slot) — purely device-driven, so no mirror state.
  const activeSlot =
    live.presetNonce > 0 ? live.activeListIndex : (initialGraph?.slot ?? null);

  // ── leveling flow (setup → run → summary) ──────────────────────────────────
  const flow = useLevelingFlow({
    rows,
    store,
    sceneInfo,
    footswitchInfo,
    ampCandidates,
    blocksByIndex,
    targetLufsByName,
    deselectKeys,
    refresh,
  });

  // ── map live pushes → hero state ───────────────────────────────────────────
  // Hero graph: synced from the connect-time handshake graph and the live
  // signal-chain push via React's "adjust state during render when an input
  // changes" pattern (it's also set imperatively by the retry-diagram handler, so
  // it stays real state). Each block guards on a prev-value compare, so it
  // converges in one pass instead of looping.
  const [prevLiveGraph, setPrevLiveGraph] = useState(live.graph);
  if (live.graph !== prevLiveGraph) {
    setPrevLiveGraph(live.graph);
    if (live.graph != null) {
      setHeroGraph(live.graph);
      setDiagError(false);
    }
  }
  const seedGraph = initialGraph ?? null;
  const [prevSeedGraph, setPrevSeedGraph] = useState(seedGraph);
  if (seedGraph !== prevSeedGraph) {
    setPrevSeedGraph(seedGraph);
    if (seedGraph != null) setHeroGraph(seedGraph);
  }
  // Record graph-arrival time for the diagram-staleness watchdog (a ref write —
  // in an effect, never during render).
  useEffect(() => {
    if (heroGraph != null) lastGraphAtRef.current = Date.now();
  }, [heroGraph]);

  // Fallback seed: a graphless connect (handshake graph=none → initialGraph null)
  // plus an idle device (no tmp://signal-chain push since this mount) leaves the
  // hero with nothing to show. Read the monitor's ALREADY-CURRENT cached graph
  // once to self-heal — cheap (no device I/O), instead of waiting for a push an
  // idle unit never sends. A null cache leaves the empty state; the manual recall
  // / diagram-Retry paths still recover it.
  useEffect(() => {
    if (!connected || heroGraph != null) return;
    let cancelled = false;
    void currentGraph()
      .then((g) => {
        if (!cancelled && g != null) setHeroGraph(g);
      })
      .catch(() => {
        /* leave the empty state */
      });
    return () => {
      cancelled = true;
    };
  }, [connected, heroGraph]);

  // tmp://live-preset → reset to the BASE scene (render-phase).
  const [prevPresetNonce, setPrevPresetNonce] = useState(live.presetNonce);
  if (live.presetNonce !== prevPresetNonce) {
    setPrevPresetNonce(live.presetNonce);
    if (live.presetNonce !== 0) {
      setLiveScene("base"); // loading a preset activates BASE
      setDiagError(false);
    }
  }

  // tmp://live-scene → the live scene within the active preset (render-phase),
  // plus the diagram-staleness watchdog (an effect — it owns a timer).
  const [prevSceneNonce, setPrevSceneNonce] = useState(live.sceneNonce);
  const ls = live.liveScene;
  if (live.sceneNonce !== prevSceneNonce) {
    setPrevSceneNonce(live.sceneNonce);
    if (live.sceneNonce !== 0 && ls != null) {
      const k = ls.key;
      const named =
        k !== "base" && ls.name != null
          ? live.scenes.findIndex((sc) => sc.name === ls.name)
          : -1;
      setLiveScene(named >= 0 ? named : k);
      setDiagError(false);
    }
  }
  useEffect(() => {
    if (live.sceneNonce === 0 || live.liveScene == null) return;
    const sceneAt = Date.now();
    const id = window.setTimeout(() => {
      if (lastGraphAtRef.current < sceneAt - GRAPH_FRESH_SLACK_MS)
        setDiagError(true);
    }, DIAGRAM_GRACE_MS);
    return () => {
      window.clearTimeout(id);
    };
  }, [live.sceneNonce, live.liveScene]);

  const handleRetryDiagram = useCallback(async () => {
    try {
      const g = await readActivePreset();
      setHeroGraph(g);
      setDiagError(false);
    } catch {
      setDiagError(true);
    }
  }, []);

  // Stable identity so the sheet's Escape-key effect (deps [onClose]) subscribes
  // once, not on every LevelView render (live-device pushes re-render frequently).
  const closeHelp = useCallback(() => {
    setHelpOpen(false);
  }, []);

  // ── derived (declared BEFORE the phase early-returns — constant hook count) ─
  const instOptions = useMemo<PickOption[]>(
    () => instrumentOptions(store?.profiles),
    [store?.profiles],
  );
  const targetOptions = useMemo<PickOption[]>(
    () =>
      (store?.targets ?? []).map((tg) => ({
        id: tg.name,
        label: `${tg.name} ${tg.lufs < 0 ? "−" : ""}${Math.abs(tg.lufs).toFixed(0)}`,
      })),
    [store?.targets],
  );
  const defaultInst = store?.profiles[0]?.id ?? "";
  const defaultTarget = store?.targets[0]?.name ?? "";
  // Resolve an instrument profile id → its display name (the run-row instrument chip).
  const instName = useCallback(
    (id: string): string => instrumentName(store?.profiles, id),
    [store?.profiles],
  );

  const activeHasScenes = activeSlot != null && live.scenes.length > 0;
  const liveSceneName = useMemo(() => {
    const lsv = live.liveScene;
    if (lsv == null) return null;
    if (lsv.key === "base") return "BASE";
    if (lsv.name != null) return lsv.name;
    // The FS-scene index may be out of range for the current scenes list, so the
    // lookup is genuinely possibly-undefined (an array index type doesn't say so).
    const sc = lsv.key < live.scenes.length ? live.scenes[lsv.key] : undefined;
    return sc?.name ?? null;
  }, [live.liveScene, live.scenes]);

  const sceneTag = useMemo<SceneTag | null>(() => {
    if (!activeHasScenes) return null;
    if (live.syncing) return { text: "—", tone: t.faint, neutral: true };
    return {
      text: liveScene === "base" ? "BASE" : (liveSceneName ?? "").toUpperCase(),
      tone: t.accentDeep,
    };
  }, [activeHasScenes, live.syncing, liveScene, liveSceneName, t]);

  const handleLevel = useCallback(() => {
    if (sel.size === 0 && pendingWhole.size === 0) return;
    flow.openFlow(sel);
  }, [sel, pendingWhole, flow]);

  // ── render ──────────────────────────────────────────────────────────────────
  if (!connected) {
    return (
      <EmptyState
        title="Presets live on the Tone Master Pro"
        body={
          <>
            Connect your unit over <UsbC /> and power it on to list your presets
            — it will connect automatically.
          </>
        }
        onScan={onScan}
      />
    );
  }

  if (phase.kind === "error") {
    return (
      <LoadErrorPane message={phase.message} onRetry={() => void refresh()} />
    );
  }

  const presetsLoading = phase.kind === "loading";
  const heroDiagramLoading =
    presetsLoading || (live.syncing && heroGraph == null);
  const heroPresetLoading = heroDiagramLoading && heroGraph == null;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        flex: 1,
        minHeight: 0,
        position: "relative",
      }}
    >
      <ActiveSignalChainView
        graph={heroGraph}
        slot={activeSlot}
        presetLoading={heroPresetLoading}
        diagramLoading={heroDiagramLoading}
        sceneTag={sceneTag}
        diagramError={diagError}
        onRetryDiagram={() => {
          void handleRetryDiagram();
        }}
      />

      <LevelingInfoRow
        onOpen={() => {
          setHelpOpen(true);
        }}
      />

      <div style={{ flex: 1, minHeight: 0 }}>
        <PresetList
          rows={rows}
          sel={sel}
          pendingWhole={pendingWhole}
          expanded={expanded}
          ready={ready}
          activeSlot={activeSlot}
          filter={filter}
          loading={presetsLoading}
          scan={scan}
          sceneInfo={sceneInfo}
          footswitchInfo={footswitchInfo}
          graphByIndex={graphByIndex}
          onFilterChange={setFilter}
          onTogglePreset={togglePreset}
          onToggleExpand={toggleExpand}
          onToggleKey={toggleKey}
          onToggleAll={toggleAll}
        />
      </div>

      <ContextFooter
        store={store}
        presetCount={presetCount}
        sceneCount={sceneCount}
        ready={ready}
        onLevel={handleLevel}
      />

      {/* ── leveling flow — one persistent wizard, body swaps per stage ── */}
      {flow.stage !== "closed" && (
        <LevelingWizard
          stage={flow.stage}
          chosen={flow.chosen}
          flowPresetCount={flow.flowPresetCount}
          isRelevel={flow.isRelevel}
          instrumentOptions={instOptions}
          targetOptions={targetOptions}
          defaultInst={defaultInst}
          defaultTarget={defaultTarget}
          instrumentName={instName}
          runItems={flow.run.items}
          runCurrentIndex={flow.run.currentIndex}
          runTotal={flow.run.total}
          runDone={flow.run.done}
          runStopped={flow.run.stopped}
          runStopping={flow.run.stopping}
          liveLufs={flow.liveLufs}
          liveTrace={flow.liveTrace}
          onCancel={flow.onCancel}
          onStart={flow.onSetupStart}
          onRunCancel={flow.onRunCancel}
          onRunComplete={flow.onRunComplete}
          onAccept={flow.onAccept}
          onRelevel={flow.onRelevel}
          onRebalanceChange={flow.setRebalance}
        />
      )}

      {helpOpen && <HowLevelingSheet onClose={closeHelp} />}
    </div>
  );
}

export default LevelView;
