//! JSON request/response types for the `alifib serve` daemon protocol.
//!
//! Communication is via stdin/stdout JSON-lines: one JSON object per line
//! in each direction. The daemon processes requests sequentially.
//!
//! # Request format
//!
//! ```json
//! {"command":"start","source_file":"Idem.ali","type_name":"Idem","initial":"lhs"}
//! {"command":"step","choice":0}
//! {"command":"undo"}
//! {"command":"show"}
//! {"command":"store","name":"myproof"}
//! {"command":"types"}
//! {"command":"type","name":"Idem"}
//! {"command":"cell","name":"idem"}
//! ```
//!
//! # Response format
//!
//! Every response is either `{"status":"ok","data":{...}}` or
//! `{"status":"error","message":"..."}`.

use serde::{Deserialize, Serialize};

use crate::aux::Tag;
use crate::core::complex::{Complex, MapDomain};
use crate::core::diagram::{CellData, Diagram, Sign};
use crate::core::matching::MatchResult;
use crate::core::partial_map::PartialMap;
use crate::analysis::strdiag::{StrDiag, VertexKind};
use crate::output::render_diagram;
use super::engine::RewriteEngine;
use super::render::render_step;

// ── Requests ─────────────────────────────────────────────────────────────────

/// A request sent by the client to the daemon.
#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    /// Start a new rewrite session from an initial (and optional target) diagram.
    Start {
        source_file: String,
        type_name: String,
        #[serde(alias = "source_diagram", alias = "initial_diagram")]
        initial: String,
        #[serde(default, alias = "target_diagram")]
        target: Option<String>,
        #[serde(default)]
        backward: bool,
    },
    /// Resume a session from a proof diagram, decomposing it into its steps.
    /// `proof` and `target` are diagram names or expressions in the source.
    Resume {
        source_file: String,
        type_name: String,
        proof: String,
        #[serde(default)]
        target: Option<String>,
        #[serde(default)]
        backward: bool,
    },
    /// Apply the rewrite at the given choice index.
    Step {
        choice: usize,
    },
    /// Apply multiple rewrites in parallel by their indices.
    StepMulti {
        choices: Vec<usize>,
    },
    /// Apply up to `max_steps` rewrites automatically, always picking index 0.
    ///
    /// Stops early when the target is reached or no rewrites remain.
    Auto {
        max_steps: usize,
    },
    /// Apply randomly selected available rewrite.
    Random {
        max_steps: usize,
    },
    /// Undo the last step.
    Undo,
    /// Undo back to a specific step count (0 = reset to source).
    UndoTo {
        step: usize,
    },
    /// Redo the last undone step.
    Redo,
    /// Redo forward to a specific step count.
    RedoTo {
        step: usize,
    },
    /// Return the current state (no mutation).
    Show,
    /// Return the current proof as a re-parseable expression (for saving and
    /// resuming); `None` when no steps have been applied.
    Proof,
    /// List all rewrite rules in the type at the current dimension.
    ListRules,
    /// Return the full move history.
    History,
    /// Store the current proof as a named diagram in the type.
    Store {
        name: String,
    },
    /// List all types defined in the loaded source file.
    Types,
    /// Inspect a named type: generators by dimension, diagrams, maps.
    #[serde(rename = "type")]
    TypeInfo {
        name: String,
    },
    /// Inspect a named generator or let-binding in the active type complex.
    Cell {
        name: String,
    },
    /// Toggle parallel rewrite mode.
    Parallel {
        on: bool,
    },
    /// Set or change the target diagram on a running session.
    SetTarget {
        name: String,
    },
    /// Compute cellular homology of a named type.
    Homology {
        name: String,
    },
    /// List the open holes of maps in the current module.
    Holes,
    /// Start a hole-filling session for the hole at `index` (0-based, as listed
    /// by `holes`).
    Fill {
        index: usize,
        #[serde(default)]
        backward: bool,
    },
    /// Finalise the active fill, extending the map's definition and returning the
    /// updated source.
    Done,
    /// Load a source file (and its dependencies) without starting a rewrite
    /// session — the "loaded, idle" state from which `holes`/`fill` work.
    Load {
        source_file: String,
    },
    /// Show or toggle backward rewrite mode (when idle); reports the mode.
    Backward {
        #[serde(default)]
        on: Option<bool>,
    },
    /// End the active rewrite session or abandon the active fill.
    Stop,
    /// Persist the running source.  The CLI writes it to `path`; the web returns
    /// it for the editor to save.
    Save {
        #[serde(default)]
        path: Option<String>,
    },
    /// Shut down the daemon.
    Shutdown,
}

// ── Responses ─────────────────────────────────────────────────────────────────

/// The top-level response envelope.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum Response {
    Ok { data: ResponseData },
    Error { message: String },
}

impl Response {
    pub fn error(msg: impl Into<String>) -> Self {
        Self::Error { message: msg.into() }
    }
}

/// The payload of a successful response.
#[derive(Debug, Serialize)]
pub struct ResponseData {
    pub step_count: usize,
    pub can_redo: bool,
    /// The current/initial diagrams of an active rewrite session.  Absent for
    /// non-session responses (`holes`, idle, a 0-cell fill).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<DiagramInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial: Option<DiagramInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<DiagramInfo>,
    pub target_reached: bool,
    pub parallel: bool,
    pub backward: bool,
    pub rewrites: Vec<RewriteInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<ProofInfo>,
    /// The current proof as a re-parseable expression; only populated by `proof`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_expr: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<HistoryEntry>,
    /// All rewrite rules at the current dimension; only populated by `list_rules`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RuleInfo>,
    /// Type summaries; only populated by `types`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<TypeSummaryInfo>,
    /// Full type detail; only populated by `type_info`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_detail: Option<TypeDetailInfo>,
    /// Cell detail; only populated by `cell`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_detail: Option<CellDetailInfo>,
    /// Summary of an `auto` run; only populated by the `auto` command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto: Option<AutoInfo>,
    /// Summary of a `store` operation; only populated by the `store` command
    /// when the session had at least one rewrite step.  Lets frontends
    /// display or append the rendered proof expression back to the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stored: Option<StoredInfo>,
    /// Set when this session is a hole-filling rather than a free rewrite, so
    /// the frontend can label it and offer `done`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<FillInfo>,
    /// A canonical one-line result message (`Applied r`, `Filled ?x with …`, …),
    /// shared verbatim by every front-end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The module's open holes; only populated by `holes`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<HoleInfo>,
    /// The boundaryless 0-cell fill state; only populated during a 0-cell fill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zero_cell: Option<ZeroCellInfo>,
    /// The updated running source; populated by `done`/`save`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Cellular homology; only populated by `homology`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homology: Option<HomologyInfo>,
}

impl ResponseData {
    /// A blank response for non-session results (`holes`, idle, a `done`/`save`
    /// that leaves no active session); fields are filled in by the caller.
    pub fn empty() -> Self {
        ResponseData {
            step_count: 0,
            can_redo: false,
            current: None,
            initial: None,
            target: None,
            target_reached: false,
            parallel: false,
            backward: false,
            rewrites: vec![],
            proof: None,
            proof_expr: None,
            history: vec![],
            rules: vec![],
            types: vec![],
            type_detail: None,
            cell_detail: None,
            auto: None,
            stored: None,
            fill: None,
            message: None,
            holes: vec![],
            zero_cell: None,
            source: None,
            homology: None,
        }
    }
}

/// Cellular homology of a type, for the `homology` response.
#[derive(Debug, Clone, Serialize)]
pub struct HomologyInfo {
    pub groups: Vec<HomologyGroupInfo>,
    pub euler_characteristic: i64,
}

/// One homology group `H_d`, with its display string (e.g. `Z`, `Z/2`).
#[derive(Debug, Clone, Serialize)]
pub struct HomologyGroupInfo {
    pub dim: usize,
    pub display: String,
}

/// One open hole of a map in the current module, numbered for `fill`.
#[derive(Debug, Clone, Serialize)]
pub struct HoleInfo {
    pub index: usize,
    pub type_name: String,
    pub map_name: String,
    pub domain_name: String,
    pub source_name: String,
    pub dim: usize,
    pub boundary: String,
}

/// The state of a boundaryless 0-cell fill: the candidate 0-cells (offered only
/// while unchosen) and the current pick.
#[derive(Debug, Clone, Serialize)]
pub struct ZeroCellInfo {
    pub choices: Vec<ZeroCellChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chosen: Option<String>,
    pub target_reached: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZeroCellChoice {
    pub index: usize,
    pub name: String,
}

/// Identifies the hole a fill session is constructing.
#[derive(Debug, Clone, Serialize)]
pub struct FillInfo {
    pub type_name: String,
    pub map_name: String,
    pub domain_name: String,
    pub source_name: String,
    pub dim: usize,
}

/// Populated for `auto` responses.
#[derive(Debug, Serialize)]
pub struct AutoInfo {
    pub applied: usize,
    pub stop_reason: String,
}

/// Populated for successful `store` responses that had at least one step.
#[derive(Debug, Serialize)]
pub struct StoredInfo {
    pub type_name: String,
    pub def_name: String,
    /// The rendered proof diagram as an alifib expression suitable for
    /// pasting into a `let` binding.
    pub expr: String,
}

/// Summary of a single type, for the `types` response.
#[derive(Debug, Serialize)]
pub struct TypeSummaryInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_dim: Option<usize>,
    pub generator_count: usize,
    pub diagram_count: usize,
    pub map_count: usize,
}

/// A single generator with optional boundary, for `type_info` and `cell`.
#[derive(Debug, Serialize)]
pub struct GeneratorInfo {
    pub name: String,
    pub dim: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<DiagramInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<DiagramInfo>,
}

/// A let-binding or session-stored diagram entry.
#[derive(Debug, Serialize)]
pub struct DiagramEntryInfo {
    pub name: String,
    pub dim: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<DiagramInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<DiagramInfo>,
    /// The diagram expression as a source-language string.
    pub expr: String,
}

/// A generator in the domain of a map.
#[derive(Debug, Serialize)]
pub struct MapDomainGenerator {
    pub name: String,
    pub dim: usize,
}

/// A named partial map in a type.
#[derive(Debug, Serialize)]
pub struct MapEntry {
    pub name: String,
    pub domain: String,
    pub generators: Vec<MapDomainGenerator>,
    /// Pre-rendered boundaries of the map's open holes (`?name : in → out`),
    /// so front-ends can show `… with holes`.  Empty for a total map.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<String>,
}

/// Full detail of a type, for the `type_info` response.
#[derive(Debug, Serialize)]
pub struct TypeDetailInfo {
    pub name: String,
    pub generators: Vec<GeneratorInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagrams: Vec<DiagramEntryInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<MapEntry>,
}

/// Detail of a single cell or let-binding, for the `cell` response.
#[derive(Debug, Serialize)]
pub struct CellDetailInfo {
    pub name: String,
    pub dim: usize,
    /// `"generator"` or `"diagram"`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<DiagramInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<DiagramInfo>,
    /// Source-language expression; only present for let-bindings / stored proofs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
}

/// Rich information about a diagram for client display.
#[derive(Debug, Serialize)]
pub struct DiagramInfo {
    /// Space-separated top-level label string, e.g. `"id id id"`.
    pub label: String,
    /// Dimension of the diagram.
    pub dim: usize,
    /// Number of top-dimensional cells.
    pub cell_count: usize,
    /// Labels at each dimension, from 0 up to top_dim.
    pub cells_by_dim: Vec<DimSlice>,
}

/// The cells at a single dimension.
#[derive(Debug, Serialize)]
pub struct DimSlice {
    pub dim: usize,
    pub cells: Vec<String>,
}

/// Rich information about a single candidate rewrite or a parallel family.
#[derive(Debug, Serialize)]
pub struct RewriteInfo {
    pub index: usize,
    pub rule_name: String,
    pub input: DiagramInfo,
    pub output: DiagramInfo,
    /// Positions of the matched top-dim cells within the current diagram.
    pub match_positions: Vec<usize>,
    /// Current diagram with matched cells bracketed, e.g. `"[id id] id"`.
    pub match_display: String,
    /// Non-empty for parallel families: the constituent matches.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub family: Vec<FamilyMember>,
}

/// A member of a parallel rewrite family.
#[derive(Debug, Serialize)]
pub struct FamilyMember {
    pub rule_name: String,
    pub match_positions: Vec<usize>,
}

/// A rewrite rule (n+1-generator) in the type, independent of the current diagram.
#[derive(Debug, Serialize)]
pub struct RuleInfo {
    pub name: String,
    pub input: DiagramInfo,
    pub output: DiagramInfo,
}

/// A summary of the running proof diagram.
#[derive(Debug, Serialize)]
pub struct ProofInfo {
    pub dim: usize,
    pub step_count: usize,
    pub input_label: String,
    pub output_label: String,
}

/// A single entry in the move history.
#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub step: usize,
    pub rule_name: String,
    pub choice: Option<Vec<usize>>,
}

// ── Builders ──────────────────────────────────────────────────────────────────

/// Build a [`DiagramInfo`] from a diagram and its rendering scope.
///
/// Collects the flat top-level label string, dimension, top-dim cell count,
/// and the full `cells_by_dim` breakdown for all dimensions 0..=top_dim.
pub fn diagram_info(diagram: &Diagram, scope: &Complex) -> DiagramInfo {
    let dim = diagram.top_dim();
    let label = render_diagram(diagram, scope);
    let cell_count = diagram.labels_at(dim).map(|ls| ls.len()).unwrap_or(0);

    let cells_by_dim = (0..=dim)
        .filter_map(|d| {
            diagram.labels_at(d).map(|labels| DimSlice {
                dim: d,
                cells: labels
                    .iter()
                    .map(|tag| {
                        scope
                            .find_generator_by_tag(tag)
                            .filter(|n| !n.is_empty())
                            .cloned()
                            .unwrap_or_else(|| format!("{}", tag))
                    })
                    .collect(),
            })
        })
        .collect();

    DiagramInfo { label, dim, cell_count, cells_by_dim }
}

fn map_domain_generators(pmap: &PartialMap, domain_complex: &Complex) -> Vec<MapDomainGenerator> {
    let mut gens = Vec::new();
    for (dim, tags) in pmap.domain_by_dim() {
        for tag in &tags {
            if let Some(name) = domain_complex.find_generator_by_tag(tag) {
                if !name.is_empty() {
                    gens.push(MapDomainGenerator { name: name.clone(), dim });
                }
            }
        }
    }
    gens
}

pub fn resolve_domain_complex<'a>(
    store: &'a crate::interpreter::GlobalStore,
    domain: &MapDomain,
) -> Option<&'a Complex> {
    match domain {
        MapDomain::Type(gid) => store.find_type(*gid).map(|e| &*e.complex),
        MapDomain::Module(mid) => store.find_module(mid),
    }
}

fn domain_label(module_complex: &Complex, domain: &MapDomain) -> String {
    match domain {
        MapDomain::Type(gid) => {
            let tag = Tag::Global(*gid);
            module_complex
                .find_generator_by_tag(&tag)
                .filter(|n| !n.is_empty())
                .cloned()
                .unwrap_or_default()
        }
        MapDomain::Module(mid) => mid.clone(),
    }
}

pub fn build_map_entries(
    tc: &Complex,
    module_complex: &Complex,
    store: &crate::interpreter::GlobalStore,
) -> Vec<MapEntry> {
    let mut entries: Vec<MapEntry> = tc.maps_iter()
        .filter(|(n, _, _)| !n.is_empty())
        .map(|(map_name, pmap, domain)| {
            let dc = resolve_domain_complex(store, domain);
            let gens = dc.map(|dc| map_domain_generators(pmap, dc)).unwrap_or_default();
            MapEntry {
                name: map_name.clone(),
                domain: domain_label(module_complex, domain),
                generators: gens,
                holes: map_hole_boundaries(tc, map_name, dc.unwrap_or(tc)),
            }
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// The pre-rendered boundaries of a map's open holes (`?name : in → out`), in
/// the same `(dim, name)` order the `holes` command uses.  Empty for a total map.
fn map_hole_boundaries(tc: &Complex, map_name: &str, domain_ref: &Complex) -> Vec<String> {
    let Some(holes) = tc.map_holes(map_name) else { return Vec::new() };
    let mut open: Vec<&crate::core::map_hole::MapHole> =
        holes.iter().filter(|h| h.image.is_none()).collect();
    open.sort_by_key(|h| (h.dim, domain_ref.find_generator_by_tag(&h.source).cloned().unwrap_or_default()));
    open.iter()
        .map(|h| crate::output::normalize::render_hole_boundary(h, holes, tc, domain_ref))
        .collect()
}

/// Build the standard [`ResponseData`] snapshot from an engine.
///
/// Includes current/source/target diagrams, available rewrites, proof info
/// (if steps have been taken), and optionally the full move history.
/// Call with `include_history: false` for every response except `History`.
pub fn build_response(engine: &RewriteEngine, include_history: bool) -> ResponseData {
    let scope = engine.type_complex();
    let current = engine.current_diagram();
    let backward = engine.backward();

    let rewrites: Vec<RewriteInfo> = engine
        .rewrites()
        .iter()
        .enumerate()
        .map(|(i, pr)| build_rewrite_info_from_family(i, pr, scope))
        .collect();

    let proof = if engine.steps().is_empty() {
        None
    } else {
        let n = engine.initial_diagram().top_dim();
        let (input_label, output_label) = if backward {
            (render_diagram(engine.current_diagram(), scope),
             render_diagram(engine.initial_diagram(), scope))
        } else {
            (render_diagram(engine.initial_diagram(), scope),
             render_diagram(engine.current_diagram(), scope))
        };
        Some(ProofInfo {
            dim: n + 1,
            step_count: engine.step_count(),
            input_label,
            output_label,
        })
    };

    let history = if include_history {
        engine.history()
            .enumerate()
            .map(|(i, e)| HistoryEntry {
                step: i + 1,
                rule_name: e.rule_name.clone(),
                choice: e.choice.clone(),
            })
            .collect()
    } else {
        vec![]
    };

    ResponseData {
        step_count: engine.step_count(),
        can_redo: engine.can_redo(),
        current: Some(diagram_info(current, scope)),
        initial: Some(diagram_info(engine.initial_diagram(), scope)),
        target: engine.target_diagram().map(|t| diagram_info(t, scope)),
        target_reached: engine.target_reached(),
        parallel: engine.parallel(),
        backward,
        rewrites,
        proof,
        proof_expr: None,
        history,
        rules: vec![],
        types: vec![],
        type_detail: None,
        cell_detail: None,
        auto: None,
        stored: None,
        fill: None,
        message: None,
        holes: vec![],
        zero_cell: None,
        source: None,
        homology: None,
    }
}

/// Build a [`ResponseData`] with detail for a single named cell or let-binding.
pub fn build_cell_response(engine: &RewriteEngine, name: &str) -> Result<ResponseData, String> {
    let cell_detail: Result<CellDetailInfo, String> = {
        let scope = engine.type_complex();
        let store = engine.store();

        if let Some((tag, dim)) = scope.find_generator(name) {
            let (input, output) = if dim > 0 {
                match store.cell_data_for_tag(scope, tag) {
                    Some(CellData::Boundary { boundary_in, boundary_out }) => (
                        Some(diagram_info(&boundary_in, scope)),
                        Some(diagram_info(&boundary_out, scope)),
                    ),
                    _ => (None, None),
                }
            } else {
                (None, None)
            };
            Ok(CellDetailInfo {
                name: name.to_owned(),
                dim,
                kind: "generator".to_owned(),
                input,
                output,
                expr: None,
            })
        } else if let Some(diag) = scope.find_diagram(name) {
            let dim = diag.top_dim();
            let (input, output) = if dim > 0 {
                let k = dim - 1;
                let inp = Diagram::boundary(Sign::Input, k, diag)
                    .ok()
                    .map(|d| diagram_info(&d, scope));
                let out = Diagram::boundary(Sign::Output, k, diag)
                    .ok()
                    .map(|d| diagram_info(&d, scope));
                (inp, out)
            } else {
                (None, None)
            };
            Ok(CellDetailInfo {
                name: name.to_owned(),
                dim,
                kind: "diagram".to_owned(),
                input,
                output,
                expr: Some(render_diagram(diag, scope)),
            })
        } else {
            Err(format!("'{}' not found in type", name))
        }
    };

    let cell_detail = cell_detail?;
    let mut data = build_response(engine, false);
    data.cell_detail = Some(cell_detail);
    Ok(data)
}

/// Build a [`ResponseData`] that lists all (n+1)-generators in the type.
///
/// Used by the `list_rules` daemon request.  Unlike `build_response`, this
/// populates `rules` with every rewrite rule at dimension `current_dim + 1`,
/// independent of which ones match the current diagram.
pub fn build_list_rules_response(engine: &RewriteEngine) -> ResponseData {
    let scope = engine.type_complex();
    let store = engine.store();
    let n = engine.current_diagram().top_dim();

    let rules: Vec<RuleInfo> = scope
        .generators_iter_by_dim(n + 1)
        .filter_map(|(name, tag)| {
            match store.cell_data_for_tag(scope, tag)? {
                CellData::Boundary { boundary_in, boundary_out } => Some(RuleInfo {
                    name: name.clone(),
                    input: diagram_info(&boundary_in, scope),
                    output: diagram_info(&boundary_out, scope),
                }),
                CellData::Zero => None,
            }
        })
        .collect();

    let mut data = build_response(engine, false);
    data.rules = rules;
    data
}

/// Serialize a [`Tag`] to a JSON value: [`Tag::Global`] becomes its integer ID,
/// anything else becomes `null`.
pub fn tag_to_json(tag: &Tag) -> serde_json::Value {
    match tag {
        Tag::Global(gid) => serde_json::Value::from(gid.as_usize()),
        Tag::Local(_) | Tag::Hole(_) => serde_json::Value::Null,
    }
}

/// Serialize a [`StrDiag`] to a JSON value.
pub fn strdiag_to_json(sd: &StrDiag) -> serde_json::Value {
    fn edges_json(graph: &crate::aux::graph::DiGraph) -> Vec<[usize; 2]> {
        let mut edges = Vec::new();
        for (u, succs) in graph.successors.iter().enumerate() {
            for &v in succs {
                edges.push([u, v]);
            }
        }
        edges
    }

    let vertices: Vec<serde_json::Value> = (0..sd.num_vertices())
        .map(|i| {
            let tag_val = sd.tags[i].as_ref().map(tag_to_json).unwrap_or(serde_json::Value::Null);
            serde_json::json!({
                "index": i,
                "kind": match sd.kinds[i] { VertexKind::Wire => "wire", VertexKind::Node => "node" },
                "label": sd.labels[i],
                "tag": tag_val,
            })
        })
        .collect();

    serde_json::json!({
        "num_wires": sd.num_wires,
        "num_nodes": sd.num_nodes,
        "vertices": vertices,
        "height": { "edges": edges_json(&sd.height) },
        "width": { "edges": edges_json(&sd.width) },
        "depth": { "edges": edges_json(&sd.depth) },
    })
}

/// Build a StrDiag JSON for a named item within a type complex.
///
/// Tries named diagrams first, then generator classifiers.
/// If `boundary` is `Some((dim, sign))`, extracts the `(sign, dim)`-boundary
/// of the diagram first. Returns a JSON object with `strdiag`, `dim`, `input`,
/// and `output` fields.
fn diagram_strdiag_json(
    diagram: &Diagram,
    scope: &Complex,
    boundary: Option<(usize, &str)>,
) -> Result<serde_json::Value, String> {
    let rendered = match boundary {
        None => diagram.clone(),
        Some((k, sign_str)) => {
            let sign = match sign_str {
                "input" => Sign::Input,
                _ => Sign::Output,
            };
            Diagram::boundary(sign, k, diagram)
                .map_err(|e| format!("boundary extraction failed: {}", e))?
        }
    };

    let dim = rendered.top_dim();
    let (input, output) = if dim >= 1 {
        let inp = Diagram::boundary(Sign::Input, dim - 1, &rendered)
            .map_err(|e| format!("{}", e))?;
        let out = Diagram::boundary(Sign::Output, dim - 1, &rendered)
            .map_err(|e| format!("{}", e))?;
        (
            render_diagram(&inp, scope),
            render_diagram(&out, scope),
        )
    } else {
        (String::new(), String::new())
    };

    let label = render_diagram(&rendered, scope);
    let sd = StrDiag::from_diagram(&rendered, scope);

    Ok(serde_json::json!({
        "strdiag": strdiag_to_json(&sd),
        "dim": dim,
        "label": label,
        "input": input,
        "output": output,
    }))
}

pub fn build_strdiag_response(
    store: &crate::interpreter::GlobalStore,
    source_path: &str,
    type_name: &str,
    item_name: &str,
    boundary: Option<(usize, &str)>,
) -> Result<serde_json::Value, String> {
    let type_complex = super::engine::resolve_type(store, source_path, type_name)?;
    let diagram = type_complex.find_diagram(item_name)
        .or_else(|| type_complex.classifier(item_name))
        .ok_or_else(|| format!("'{}' not found in type '{}'", item_name, type_name))?;
    diagram_strdiag_json(diagram, &type_complex, boundary)
}

/// Build a StrDiag JSON for the image of a domain generator under a map.
pub fn build_map_image_strdiag(
    store: &crate::interpreter::GlobalStore,
    source_path: &str,
    type_name: &str,
    map_name: &str,
    gen_name: &str,
    boundary: Option<(usize, &str)>,
) -> Result<serde_json::Value, String> {
    let type_complex = super::engine::resolve_type(store, source_path, type_name)?;
    let (pmap, domain) = type_complex.find_map(map_name)
        .ok_or_else(|| format!("map '{}' not found in type '{}'", map_name, type_name))?;

    let domain_complex = resolve_domain_complex(store, domain)
        .ok_or_else(|| format!("domain of map '{}' could not be resolved", map_name))?;

    let gen_classifier = domain_complex.classifier(gen_name)
        .ok_or_else(|| format!("generator '{}' not found in domain of '{}'", gen_name, map_name))?;

    let image = PartialMap::apply(pmap, gen_classifier)
        .map_err(|e| format!("failed to apply map: {}", e))?;

    diagram_strdiag_json(&image, &type_complex, boundary)
}

/// Build a StrDiag JSON for the output boundary of a step diagram.
pub fn step_output_strdiag_json(
    step: &Diagram,
    scope: &Complex,
) -> Result<serde_json::Value, String> {
    let n = step.top_dim().checked_sub(1)
        .ok_or_else(|| "step diagram has dim 0".to_string())?;
    let out = Diagram::boundary(Sign::Output, n, step)
        .map_err(|e| format!("{}", e))?;
    Ok(strdiag_json_from_diagram(&out, scope))
}

/// Build a StrDiag JSON directly from a diagram and complex.
pub fn strdiag_json_from_diagram(
    diagram: &Diagram,
    scope: &Complex,
) -> serde_json::Value {
    let sd = StrDiag::from_diagram(diagram, scope);
    strdiag_to_json(&sd)
}

/// Build a [`TypeSummaryInfo`] list from the store, without requiring an engine.
pub fn build_types_from_store(
    store: &crate::interpreter::GlobalStore,
    source_path: &str,
) -> Vec<TypeSummaryInfo> {
    let normalized = store.normalize();
    normalized
        .modules
        .iter()
        .find(|m| m.path == source_path)
        .map(|module| {
            module
                .types
                .iter()
                .filter(|t| !t.name.is_empty())
                .map(|t| {
                    let max_dim = t.dims.iter().map(|d| d.dim).max();
                    let generator_count: usize = t.dims.iter().map(|d| d.cells.len()).sum();
                    TypeSummaryInfo {
                        name: t.name.clone(),
                        max_dim,
                        generator_count,
                        diagram_count: t.diagrams.len(),
                        map_count: t.maps.len(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Build a [`TypeDetailInfo`] from the store, without requiring an engine.
pub fn build_type_detail_from_store(
    store: &crate::interpreter::GlobalStore,
    source_path: &str,
    name: &str,
) -> Result<TypeDetailInfo, String> {
    let type_complex = super::engine::resolve_type(store, source_path, name)?;
    let module_complex = store.find_module(source_path)
        .ok_or_else(|| format!("module '{}' not found", source_path))?;

    let mut generators: Vec<GeneratorInfo> = type_complex
        .generators_iter()
        .filter(|(n, _, _)| !n.is_empty())
        .map(|(gen_name, tag, dim)| {
            let (input, output) = if dim > 0 {
                match store.cell_data_for_tag(&type_complex, tag) {
                    Some(CellData::Boundary { boundary_in, boundary_out }) => (
                        Some(diagram_info(&boundary_in, &type_complex)),
                        Some(diagram_info(&boundary_out, &type_complex)),
                    ),
                    _ => (None, None),
                }
            } else {
                (None, None)
            };
            GeneratorInfo { name: gen_name.clone(), dim, input, output }
        })
        .collect();
    generators.sort_by(|a, b| a.dim.cmp(&b.dim).then_with(|| a.name.cmp(&b.name)));

    let mut diagrams: Vec<DiagramEntryInfo> = type_complex
        .diagrams_iter()
        .filter(|(n, _)| !n.is_empty())
        .map(|(diag_name, diag)| {
            let dim = diag.top_dim();
            let (input, output) = if dim > 0 {
                let k = dim - 1;
                let inp = Diagram::boundary(Sign::Input, k, diag)
                    .ok()
                    .map(|d| diagram_info(&d, &type_complex));
                let out = Diagram::boundary(Sign::Output, k, diag)
                    .ok()
                    .map(|d| diagram_info(&d, &type_complex));
                (inp, out)
            } else {
                (None, None)
            };
            DiagramEntryInfo {
                name: diag_name.clone(),
                dim,
                input,
                output,
                expr: render_diagram(diag, &type_complex),
            }
        })
        .collect();
    diagrams.sort_by(|a, b| a.name.cmp(&b.name));

    let maps = build_map_entries(&type_complex, module_complex, store);

    Ok(TypeDetailInfo { name: name.to_owned(), generators, diagrams, maps })
}

/// Compute cellular homology of a named type as a [`HomologyInfo`] — the shared
/// data both front-ends render from.
pub fn build_homology_data(
    store: &crate::interpreter::GlobalStore,
    source_path: &str,
    type_name: &str,
) -> Result<HomologyInfo, String> {
    let tc = super::engine::resolve_type(store, source_path, type_name)?;
    let h = crate::analysis::homology::compute_homology(&tc);
    Ok(HomologyInfo {
        groups: h.groups.iter()
            .map(|(dim, g)| HomologyGroupInfo { dim: *dim, display: format!("{}", g) })
            .collect(),
        euler_characteristic: h.euler_characteristic,
    })
}

fn build_rewrite_info_from_family(
    index: usize,
    pr: &MatchResult,
    scope: &Complex,
) -> RewriteInfo {
    let is_single = pr.members.len() == 1;

    let match_display = render_step(&pr.step, scope);

    let rule_names: Vec<&str> = pr.members.iter()
        .map(|m| m.rule_name.as_str())
        .collect();
    let rule_name = rule_names.join(", ");

    let n_plus_1 = pr.step.top_dim();
    let n = n_plus_1.saturating_sub(1);

    let placeholder = || DiagramInfo { label: "?".into(), dim: 0, cell_count: 0, cells_by_dim: vec![] };

    let (input, output) = if is_single {
        let rule_tag = pr.step.labels_at(n_plus_1).and_then(|ls| ls.first());
        let classifier = rule_tag
            .and_then(|tag| scope.find_generator_by_tag(tag))
            .and_then(|name| scope.classifier(name));
        match classifier {
            Some(cl) => match (
                Diagram::boundary(Sign::Input, n, cl),
                Diagram::boundary(Sign::Output, n, cl),
            ) {
                (Ok(inp), Ok(out)) => (diagram_info(&inp, scope), diagram_info(&out, scope)),
                _ => (placeholder(), placeholder()),
            },
            None => (placeholder(), placeholder()),
        }
    } else {
        (
            Diagram::boundary(Sign::Input, n, &pr.step)
                .map(|d| diagram_info(&d, scope))
                .unwrap_or_else(|_| placeholder()),
            Diagram::boundary(Sign::Output, n, &pr.step)
                .map(|d| diagram_info(&d, scope))
                .unwrap_or_else(|_| placeholder()),
        )
    };

    let family: Vec<FamilyMember> = if is_single {
        vec![]
    } else {
        pr.members.iter().map(|m| FamilyMember {
            rule_name: m.rule_name.clone(),
            match_positions: m.match_positions.clone(),
        }).collect()
    };

    RewriteInfo {
        index,
        rule_name,
        input,
        output,
        match_positions: pr.image_positions.clone(),
        match_display,
        family,
    }
}
