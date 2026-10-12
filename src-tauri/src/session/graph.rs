//! The audio-graph model (blocks, stages, lanes, route graph) and its pure extraction
//! from preset JSON: level blocks, doctor params and the active graph. No I/O.

use super::*;

/// A level-type block control discoverable from a preset's `audioGraph` — a
/// candidate leveling knob. `group_id`/`node_id`/`parameter_id` are the
/// `ChangeParameter` coordinates; `model_id` is the stable Fender block id for
/// catalog classification; `value` is the current normalized (0..1) value.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LevelBlock {
    pub group_id: String,
    pub node_id: String,
    pub model_id: String,
    pub parameter_id: String,
    pub value: f32,
}

/// One block (DSP node) in the active preset's signal chain, for the
/// active-preset signal chain rendering. `model` is the node's `FenderId` (`ACD_*` factory / `USR_*` user IR)
/// — the identity the frontend maps to a pedal icon. `bypassed` reads
/// `dspUnitParameters.bypass`. Routing (which group, series vs parallel) is carried
/// by [`ActiveGraph`].
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphNode {
    pub group_id: String,
    pub node_id: String,
    pub model: String,
    pub bypassed: bool,
    /// For a CabSim block: its primary cabinet id (`dspUnitParameters.cabsimid`,
    /// e.g. `Mar1960aV30Alt`) — lets the strip NAME the cab instead of the generic
    /// "CAB IR", and expand a dual-cab into two parallel tiles. `None` for non-cab
    /// nodes (omitted from the wire via `skip_serializing_if`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cab_sim_id: Option<String>,
    /// The SECOND cabinet id of a dual-cab block (`cab2simid`), set when
    /// `cab_sim2_enabled`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cab_sim_id2: Option<String>,
    /// Whether this CabSim runs two cabinets in parallel (`cabsim2enabled`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cab_sim2_enabled: Option<bool>,
    /// ALLOWLISTED current `dspUnitParameters` values for Doctor's value-aware
    /// prescriptions: the reverb wet/dry mix names (`mix`, `wetdrymix`), the cab
    /// low/high cut (`hpf`, `lpf`), and the EQ-10 `gain*hz` band gains. Empty for
    /// every other param/node — the full param map would bloat every snapshot/
    /// backup row for nothing.
    pub params: std::collections::HashMap<String, f64>,
}

/// One ordered stage of the signal chain: a `series` run of blocks, or a `split`
/// with two parallel lanes (each a series run) joined by a mix. A chain is a
/// `Vec<Stage>` walked left→right, so any number of sequential splits with series
/// segments before/between/after them is representable (the device's 7 guitar
/// slots allow up to two). Serializes to the frontend's discriminated union:
/// `{kind:"series",blocks:[…]}` / `{kind:"split",a:[…],b:[…]}`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Stage {
    Series {
        blocks: Vec<GraphNode>,
    },
    Split {
        a: Vec<GraphNode>,
        b: Vec<GraphNode>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InputLane {
    #[serde(rename = "type")]
    pub kind: String,
    pub blocks: Vec<GraphNode>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OutputLane {
    #[serde(rename = "type")]
    pub kind: String,
    pub blocks: Vec<GraphNode>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InputPair {
    pub a: InputLane,
    pub b: InputLane,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OutputPair {
    pub a: OutputLane,
    pub b: OutputLane,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IndependentLane {
    pub input: String,
    pub output: String,
    pub blocks: Vec<GraphNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RouteGraph {
    pub(super) input_type: Option<String>,
    pub(super) output_type: Option<String>,
    pub(super) inputs: Option<InputPair>,
    pub(super) outputs: Option<OutputPair>,
    pub(super) lanes: Option<Vec<IndependentLane>>,
    pub(super) stages: Vec<Stage>,
}

/// The active preset's signal-chain topology, read live from the field-3 partial
/// (the only reliable live preset read). `template` gives the active routing
/// (one of 12 signal-path templates); `split_mix` carries the available split/mix
/// controls, including inactive ones. `nodes` are
/// the blocks per group (`G1..G7` guitar, `M1..M4` mic), in stable group+array
/// order. `name`/`slot` are `None` when the device-truncated partial doesn't carry
/// preset `info` (the honest no-fabricate state — the partial truncates at
/// `"scenes"`, and `info` sits past that, so these are usually `None`).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ActiveGraph {
    pub name: Option<String>,
    pub slot: Option<ListIndex>,
    pub template: Option<String>,
    pub split_mix: Option<serde_json::Value>,
    /// Flat block list in stable group+array order (every block, all groups).
    pub nodes: Vec<GraphNode>,
    /// Single-input source label for renderers that need to distinguish guitar
    /// vs mic/line paths. Omitted for dual-input or independent-lane templates.
    pub input_type: Option<String>,
    /// Single-output sink label for renderers that need to distinguish one
    /// summed output from split output 1/2 paths.
    pub output_type: Option<String>,
    /// Dual-input templates: each input lane may carry pre-join blocks, then
    /// the two lanes converge at a JOIN node before `stages`.
    pub inputs: Option<InputPair>,
    /// Split-output templates: `stages` feed a SPLIT node, then the two output
    /// lanes terminate independently at OUT 1 / OUT 2.
    pub outputs: Option<OutputPair>,
    /// Fully independent dual-rail templates such as `gtrMicParallel`.
    pub lanes: Option<Vec<IndependentLane>>,
    /// Routing-aware view: the ordered series/split stages the strip renders.
    /// Derived from the group slots + routing template id.
    pub stages: Vec<Stage>,
}

/// A control name is a leveling candidate if it reads as a level/volume/output
/// control. Deliberately EXCLUDES `gain`/drive (changes tone, not clean level)
/// and EQ — the scoping decision was "level-type controls only".
///
/// NOT the leveling PICKER's gate any more — that is
/// [`crate::param_class::classify`], via [`extract_level_candidates`]. This
/// name-substring rule survives only inside [`extract_level_blocks`], whose
/// remaining callers all look up ONE known control (the amp's `outputLevel`) or
/// dump a probe diagnostic, so the two rules can no longer disagree about what
/// the user is OFFERED.
pub(super) fn is_level_param(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.contains("gain") || n.contains("drive") {
        return false;
    }
    n.contains("level") || n.contains("volume") || n == "vol" || n.contains("master")
}

/// Walk decoded preset JSON and collect every level-type block control under
/// `audioGraph.guitarNodes.{G1..G7}[].dspUnitParameters`. Numeric params only;
/// names filtered by `is_level_param`. Each node's id is its `nodeId` (falling
/// back to `FenderId`). Tolerant of missing branches.
///
/// A VALUE LOOKUP, not a picker: every remaining caller either narrows to one
/// known control (the amp's `outputLevel` — `scene_jobs::classify_scene_knobs`,
/// `probe_api::overlay_ab`, `probe_api::level::filter_amp_candidates`) or prints a
/// probe diagnostic. What the user is OFFERED comes from
/// [`extract_level_candidates`] instead.
pub(crate) fn extract_level_blocks(v: &serde_json::Value) -> Vec<LevelBlock> {
    let mut out = Vec::new();
    let Some(groups) = v
        .pointer("/audioGraph/guitarNodes")
        .and_then(|g| g.as_object())
    else {
        return out;
    };
    for (group_id, nodes) in groups {
        let Some(nodes) = nodes.as_array() else {
            continue;
        };
        for node in nodes {
            let Some(node) = crate::audiograph::Node::of(node) else {
                continue;
            };
            let (Some(node_id), Some(model_id)) = (node.instance(), node.model()) else {
                continue;
            };
            let Some(params) = node.params() else {
                continue;
            };
            for (pname, pval) in params {
                if let Some(f) = pval.as_f64() {
                    if is_level_param(pname) {
                        out.push(LevelBlock {
                            group_id: group_id.clone(),
                            node_id: node_id.to_string(),
                            model_id: model_id.to_string(),
                            parameter_id: pname.clone(),
                            value: f as f32,
                        });
                    }
                }
            }
        }
    }
    out
}

/// The leveling PICKER's block-control list: every control under
/// `audioGraph.guitarNodes.{G1..G7}[].dspUnitParameters` that
/// [`crate::param_class::classify`] recognises as a level or wet/mix control, via the shared
/// [`crate::footswitch::level_candidates_for_node`].
///
/// Same walk and same [`LevelBlock`] shape as [`extract_level_blocks`] — only the GATE
/// differs, and that is the point. The legacy name-substring rule ([`is_level_param`]) and the
/// classifier disagree in both directions, and the disagreement was user-visible: the preset
/// lane's picker OFFERED `ACD_TMRumbleV3.level` (an amp knob the run-time gate then refuses)
/// and HID wet `mix` / `ACD_Boost.gain` (raw dB, HW-verified fw 1.8.45) that the footswitch
/// and scene-handle pickers both offer. One gate for every picker, so the same control can no
/// longer be answered two ways.
///
/// GUITAR graph only, like [`extract_level_blocks`]: re-amp drives the instrument input, so
/// only the guitar chain reaches the USB-Out the leveler measures.
pub(crate) fn extract_level_candidates(v: &serde_json::Value) -> Vec<LevelBlock> {
    let mut out = Vec::new();
    let Some(groups) = v
        .pointer("/audioGraph/guitarNodes")
        .and_then(|g| g.as_object())
    else {
        return out;
    };
    for (group_id, nodes) in groups {
        for node in nodes.as_array().into_iter().flatten() {
            let Some(node) = crate::audiograph::Node::of(node) else {
                continue;
            };
            // The classifier's override key is the model. A graph carrying no `FenderId`
            // falls back to the node id — the same fallback `leveller::FsParamTarget::from_preset`
            // applies, so the picker and the solve classify such a node identically.
            let (Some(node_id), Some(model_id)) = (node.instance(), node.model()) else {
                continue;
            };
            let Some(params) = node.params() else {
                continue;
            };
            out.extend(
                crate::footswitch::level_candidates_for_node(group_id, node_id, model_id, params)
                    .into_iter()
                    .map(|c| LevelBlock {
                        group_id: c.group_id,
                        node_id: c.node_id,
                        model_id: c.fender_id,
                        parameter_id: c.parameter_id,
                        value: c.current as f32,
                    }),
            );
        }
    }
    out
}

/// Doctor's param allowlist (see the [`GraphNode::params`] doc): the reverb wet/dry mix
/// names, the cab low/high cut, and the EQ-10 band gains. The ONE definition — the base
/// graph and every per-sound override (`doctor::NodeOverride`) filter through it, so a
/// sound's effective graph can never carry a param its base graph would have dropped.
pub(crate) fn doctor_param_kept(k: &str) -> bool {
    k == "mix"
        || k == "wetdrymix"
        || k == "hpf"
        || k == "lpf"
        || (k.starts_with("gain") && k.ends_with("hz"))
}

/// A `dspUnitParameters` object narrowed to [`doctor_param_kept`] keys with numeric values.
pub(crate) fn doctor_params(
    params: Option<&serde_json::Map<String, serde_json::Value>>,
) -> std::collections::HashMap<String, f64> {
    params
        .map(|o| {
            o.iter()
                .filter(|(k, _)| doctor_param_kept(k))
                .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f)))
                .collect()
        })
        .unwrap_or_default()
}

/// Walk decoded preset JSON into an [`ActiveGraph`] for the active-preset signal chain strip: every
/// node under `audioGraph.{guitarNodes,micNodes}.<group>[]` (model id + bypass),
/// in stable sorted-group then array order, plus `audioGraph.template` for
/// routing and `audioGraph.splitMix` for its controls. Tolerant of every missing branch — a
/// truncated partial that only reached `guitarNodes` still yields its nodes.
pub(crate) fn extract_active_graph(
    v: &serde_json::Value,
    template_hint: Option<&str>,
) -> ActiveGraph {
    // Collect each graph's group slots in sorted-key order (G1..G7, M1..M4).
    // A BTreeMap keeps the deterministic slot order the stage mapper relies on.
    let parse_groups = |graph: &str| -> std::collections::BTreeMap<String, Vec<GraphNode>> {
        let mut out = std::collections::BTreeMap::new();
        let Some(groups) = v
            .pointer(&format!("/audioGraph/{graph}"))
            .and_then(|g| g.as_object())
        else {
            return out;
        };
        for (group_id, val) in groups {
            let Some(arr) = val.as_array() else { continue };
            let mut blocks = Vec::new();
            for node in arr {
                let Some(view) = crate::audiograph::Node::of(node) else {
                    continue;
                };
                let (Some(model), Some(node_id)) = (view.model(), view.instance()) else {
                    continue;
                };
                let params = node.get("dspUnitParameters");
                let bypassed = params
                    .and_then(|p| p.get("bypass"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                // Cab-sim params. `cabsimid` NAMES the cab — present on the standalone
                // CabSim block AND on amp combos/half-stacks (their built-in cab), so the
                // strip can draw a head-over-cab half-stack. The dual-cab SPLIT fields
                // (cab2simid / cabsim2enabled) are a real SECOND parallel cab ONLY on the
                // standalone CabSim block; on an amp the same keys mean a dual MIC on ONE
                // cab — suppress them there so the strip never splits a half-stack amp.
                // Keys are the device's exact lowercase `dspUnitParameters` names.
                let is_cab_block = model == "ACD_CabSimTMS";
                let cab_sim_id = params
                    .and_then(|p| p.get("cabsimid"))
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                let dual = params.filter(|_| is_cab_block);
                let cab_sim_id2 = dual
                    .and_then(|p| p.get("cab2simid"))
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                let cab_sim2_enabled = dual
                    .and_then(|p| p.get("cabsim2enabled"))
                    .and_then(|b| b.as_bool());
                let node_params = doctor_params(params.and_then(|p| p.as_object()));
                blocks.push(GraphNode {
                    group_id: group_id.clone(),
                    node_id: node_id.to_string(),
                    model: model.to_string(),
                    bypassed,
                    cab_sim_id,
                    cab_sim_id2,
                    cab_sim2_enabled,
                    params: node_params,
                });
            }
            if !blocks.is_empty() {
                out.insert(group_id.clone(), blocks);
            }
        }
        out
    };

    let guitar = parse_groups("guitarNodes");
    let mic = parse_groups("micNodes");

    // Flat list (every block, guitar then mic, in slot+array order) — back-compat.
    let nodes: Vec<GraphNode> = guitar
        .values()
        .chain(mic.values())
        .flat_map(|b| b.iter().cloned())
        .collect();

    let split_mix = v.pointer("/audioGraph/splitMix").cloned();
    let template = v
        .pointer("/audioGraph/template")
        .and_then(|x| x.as_str())
        .or(template_hint)
        .map(str::to_string);
    let route = build_route_graph(template.as_deref(), &guitar, &mic);

    ActiveGraph {
        name: v
            .pointer("/info/displayName")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        slot: v
            .pointer("/info/userSlot")
            .and_then(|x| x.as_u64())
            .and_then(|s| u32::try_from(s).ok())
            .and_then(|s| DeviceSlot::try_from(s).ok())
            .map(ListIndex::from),
        template,
        split_mix,
        nodes,
        input_type: route.input_type,
        output_type: route.output_type,
        inputs: route.inputs,
        outputs: route.outputs,
        lanes: route.lanes,
        stages: route.stages,
    }
}

pub(crate) fn is_known_routing_template(template: Option<&str>) -> bool {
    matches!(
        template,
        Some(
            "gtrSeries"
                | "gtrParallel1"
                | "gtrParallel2"
                | "gtrMicSeries"
                | "micSeries"
                | "micParallel1"
                | "gtrMicParallel"
                | "gtrMicMix"
                | "gtrMicMix2"
                | "gtrMicMix3"
                | "gtrSplit"
                | "micSplit"
        )
    )
}

pub(super) fn build_route_graph(
    template: Option<&str>,
    guitar: &std::collections::BTreeMap<String, Vec<GraphNode>>,
    mic: &std::collections::BTreeMap<String, Vec<GraphNode>>,
) -> RouteGraph {
    let g = |key: &str| guitar.get(key).cloned().unwrap_or_default();
    let m = |key: &str| mic.get(key).cloned().unwrap_or_default();
    let concat_g = |keys: &[&str]| -> Vec<GraphNode> { keys.iter().flat_map(|k| g(k)).collect() };
    let concat_m = |keys: &[&str]| -> Vec<GraphNode> { keys.iter().flat_map(|k| m(k)).collect() };
    let all_g = || concat_g(&["G1", "G2", "G3", "G4", "G5", "G6", "G7"]);
    let all_m = || concat_m(&["M1", "M2", "M3", "M4"]);
    let mut route = RouteGraph {
        input_type: Some("guitar".to_string()),
        output_type: Some("out".to_string()),
        inputs: None,
        outputs: None,
        lanes: None,
        stages: Vec::new(),
    };

    match template {
        Some("micSeries") => {
            route.input_type = Some("mic".to_string());
            push_series(&mut route.stages, all_m());
        }
        Some("micParallel1") => {
            route.input_type = Some("mic".to_string());
            push_series(&mut route.stages, m("M1"));
            push_split(&mut route.stages, m("M2"), m("M3"));
            push_series(&mut route.stages, m("M4"));
        }
        Some("gtrParallel1") => {
            push_series(&mut route.stages, g("G1"));
            push_split(&mut route.stages, g("G2"), g("G3"));
            push_series(&mut route.stages, concat_g(&["G4", "G5", "G6", "G7"]));
        }
        Some("gtrParallel2") => {
            push_series(&mut route.stages, g("G1"));
            push_split(&mut route.stages, g("G2"), g("G3"));
            push_series(&mut route.stages, g("G4"));
            push_split(&mut route.stages, g("G5"), g("G6"));
            push_series(&mut route.stages, g("G7"));
        }
        Some("gtrMicSeries") => {
            route.input_type = None;
            route.inputs = Some(InputPair {
                a: InputLane {
                    kind: "guitar".to_string(),
                    blocks: Vec::new(),
                },
                b: InputLane {
                    kind: "mic".to_string(),
                    blocks: Vec::new(),
                },
            });
            push_series(
                &mut route.stages,
                all_g().into_iter().chain(all_m()).collect(),
            );
        }
        Some("gtrMicMix" | "gtrMicMix2" | "gtrMicMix3") => {
            route.input_type = None;
            route.inputs = Some(InputPair {
                a: InputLane {
                    kind: "guitar".to_string(),
                    blocks: all_g(),
                },
                b: InputLane {
                    kind: "mic".to_string(),
                    blocks: all_m(),
                },
            });
        }
        Some("gtrMicParallel") => {
            route.input_type = None;
            route.output_type = None;
            route.lanes = Some(vec![
                IndependentLane {
                    input: "guitar".to_string(),
                    output: "out1".to_string(),
                    blocks: all_g(),
                },
                IndependentLane {
                    input: "mic".to_string(),
                    output: "out2".to_string(),
                    blocks: all_m(),
                },
            ]);
        }
        Some("gtrSplit") => {
            // One array, read once here and passed to the unmapped-group check below
            // — a single source of truth for "which groups this arm reads" instead of
            // a second hand-maintained literal that could silently drift from it.
            const USED: [&str; 3] = ["G1", "G2", "G3"];
            push_series(&mut route.stages, g(USED[0]));
            route.output_type = None;
            route.outputs = Some(OutputPair {
                a: OutputLane {
                    kind: "out1".to_string(),
                    blocks: g(USED[1]),
                },
                b: OutputLane {
                    kind: "out2".to_string(),
                    blocks: g(USED[2]),
                },
            });
            warn_unmapped_groups("gtrSplit", guitar, &USED);
        }
        Some("micSplit") => {
            const USED: [&str; 3] = ["M1", "M2", "M3"];
            route.input_type = Some("mic".to_string());
            route.output_type = None;
            push_series(&mut route.stages, m(USED[0]));
            route.outputs = Some(OutputPair {
                a: OutputLane {
                    kind: "out1".to_string(),
                    blocks: m(USED[1]),
                },
                b: OutputLane {
                    kind: "out2".to_string(),
                    blocks: m(USED[2]),
                },
            });
            warn_unmapped_groups("micSplit", mic, &USED);
        }
        _ => push_series(&mut route.stages, all_g()),
    }

    route
}

/// Append a series stage, dropping it when there are no blocks.
pub(super) fn push_series(stages: &mut Vec<Stage>, blocks: Vec<GraphNode>) {
    if !blocks.is_empty() {
        stages.push(Stage::Series { blocks });
    }
}

/// Append a split stage only when at least one lane has blocks (an all-empty
/// split slot pair is dropped rather than drawn as an empty fork).
pub(super) fn push_split(stages: &mut Vec<Stage>, a: Vec<GraphNode>, b: Vec<GraphNode>) {
    if !a.is_empty() || !b.is_empty() {
        stages.push(Stage::Split { a, b });
    }
}

/// gtrSplit/micSplit only read a fixed 3-group subset (G1-G3 / M1-M3) — the
/// same shape of unverified "which groups does this template read" assumption
/// that bunched the wrong groups per lane before it was HW-corrected. Warn
/// (don't fail) if a device payload carries blocks in a group this arm never
/// reads, so a future firmware/template surprise shows up in the logs instead
/// of silently vanishing from the rendered strip.
pub(super) fn warn_unmapped_groups(
    template: &str,
    groups: &std::collections::BTreeMap<String, Vec<GraphNode>>,
    used: &[&str],
) {
    for (group_id, blocks) in groups {
        if !blocks.is_empty() && !used.contains(&group_id.as_str()) {
            log::warn!(
                "build_route_graph: {template} has {} block(s) in unmapped group {group_id} \
                 — not rendered in the signal-chain strip",
                blocks.len()
            );
        }
    }
}
