//! JSON request/response types for the `alifib serve` daemon protocol.
//!
//! Communication is via stdin/stdout JSON-lines: one JSON object per line
//! in each direction. The daemon processes requests sequentially.
//!
//! # Request format
//!
//! ```json
//! {"command":"init","source_file":"Idem.ali","type_name":"Idem","source_diagram":"lhs"}
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
use crate::core::strdiag::{StrDiag, VertexKind};
use crate::output::render_diagram;
use super::engine::RewriteEngine;
use super::render::render_step;

// ── Requests ─────────────────────────────────────────────────────────────────

/// A request sent by the client to the daemon.
#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    /// Start a new rewrite session.
    Init {
        source_file: String,
        type_name: String,
        source_diagram: String,
        #[serde(default)]
        target_diagram: Option<String>,
    },
    /// Resume from a session file on disk.
    Resume {
        session_file: String,
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
    /// Undo the last step.
    Undo,
    /// Undo back to a specific step count (0 = reset to source).
    UndoTo {
        step: usize,
    },
    /// Return the current state (no mutation).
    Show,
    /// Save the current session to a file (for `resume`).
    Save {
        path: String,
    },
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
    pub current: DiagramInfo,
    pub source: DiagramInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<DiagramInfo>,
    pub target_reached: bool,
    pub parallel: bool,
    pub rewrites: Vec<RewriteInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<ProofInfo>,
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
}

/// A single generator with optional boundary, for `type_info` and `cell`.
#[derive(Debug, Serialize)]
pub struct GeneratorInfo {
    pub name: String,
    pub dim: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DiagramInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<DiagramInfo>,
}

/// A let-binding or session-stored diagram entry.
#[derive(Debug, Serialize)]
pub struct DiagramEntryInfo {
    pub name: String,
    pub dim: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DiagramInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<DiagramInfo>,
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
    pub source: Option<DiagramInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<DiagramInfo>,
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
    pub source: DiagramInfo,
    pub target: DiagramInfo,
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
    pub source: DiagramInfo,
    pub target: DiagramInfo,
}

/// A summary of the running proof diagram.
#[derive(Debug, Serialize)]
pub struct ProofInfo {
    pub dim: usize,
    pub step_count: usize,
    pub source_label: String,
    pub target_label: String,
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

fn domain_label(store: &crate::interpreter::GlobalStore, domain: &MapDomain) -> String {
    match domain {
        MapDomain::Type(gid) => {
            let tag = Tag::Global(*gid);
            store.modules_iter()
                .find_map(|(_, mc)| mc.find_generator_by_tag(&tag).cloned())
                .filter(|n| !n.is_empty())
                .unwrap_or_default()
        }
        MapDomain::Module(mid) => mid.clone(),
    }
}

pub fn build_map_entries(
    tc: &Complex,
    store: &crate::interpreter::GlobalStore,
) -> Vec<MapEntry> {
    tc.maps_iter()
        .filter(|(n, _, _)| !n.is_empty())
        .map(|(map_name, pmap, domain)| {
            let gens = resolve_domain_complex(store, domain)
                .map(|dc| map_domain_generators(pmap, dc))
                .unwrap_or_default();
            MapEntry {
                name: map_name.clone(),
                domain: domain_label(store, domain),
                generators: gens,
            }
        })
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

    let rewrites: Vec<RewriteInfo> = engine
        .rewrites()
        .iter()
        .enumerate()
        .map(|(i, pr)| build_rewrite_info_from_family(i, pr, scope))
        .collect();

    // The proof's source/target boundaries are the session's source diagram
    // and the current diagram respectively — no need to assemble the full
    // proof diagram just to extract these.
    let proof = if engine.steps().is_empty() {
        None
    } else {
        let n = engine.source_diagram().top_dim();
        Some(ProofInfo {
            dim: n + 1,
            step_count: engine.step_count(),
            source_label: render_diagram(engine.source_diagram(), scope),
            target_label: render_diagram(engine.current_diagram(), scope),
        })
    };

    let history = if include_history {
        engine.history_moves()
            .enumerate()
            .map(|(i, m)| HistoryEntry {
                step: i + 1,
                rule_name: m.rule_name.clone(),
                choice: if let Some(ref v) = m.choices {
                    Some(v.clone())
                } else {
                    m.choice.map(|c| vec![c])
                },
            })
            .collect()
    } else {
        vec![]
    };

    ResponseData {
        step_count: engine.step_count(),
        current: diagram_info(current, scope),
        source: diagram_info(engine.source_diagram(), scope),
        target: engine.target_diagram().map(|t| diagram_info(t, scope)),
        target_reached: engine.target_reached(),
        parallel: engine.parallel(),
        rewrites,
        proof,
        history,
        rules: vec![],
        types: vec![],
        type_detail: None,
        cell_detail: None,
        auto: None,
        stored: None,
    }
}

/// Build a [`ResponseData`] listing all types in the loaded source file.
pub fn build_types_response(engine: &RewriteEngine) -> ResponseData {
    let normalized = engine.store().normalize();
    let source_file = engine.source_file();

    let types: Vec<TypeSummaryInfo> = normalized
        .modules
        .iter()
        .find(|m| m.path == source_file)
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
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let mut data = build_response(engine, false);
    data.types = types;
    data
}

/// Build a [`ResponseData`] with full detail for the named type.
pub fn build_type_info_response(
    engine: &RewriteEngine,
    name: &str,
) -> Result<ResponseData, String> {
    let type_detail: Result<TypeDetailInfo, String> = {
        let store = engine.store();

        // Prefer the live complex for the active session type (includes stored proofs).
        let live_tc: &Complex = if engine.type_name() == name {
            engine.type_complex()
        } else {
            let gid = store
                .find_type_gid(name)
                .ok_or_else(|| format!("type '{}' not found", name))?;
            let entry = store
                .find_type(gid)
                .ok_or_else(|| format!("type '{}' not found in store", name))?;
            &*entry.complex
        };

        // All named generators, grouped by dimension, with boundaries.
        let generators: Vec<GeneratorInfo> = live_tc
            .generators_iter()
            .filter(|(n, _, _)| !n.is_empty())
            .map(|(gen_name, tag, dim)| {
                let (source, target) = if dim > 0 {
                    match store.cell_data_for_tag(live_tc, tag) {
                        Some(CellData::Boundary { boundary_in, boundary_out }) => (
                            Some(diagram_info(&boundary_in, live_tc)),
                            Some(diagram_info(&boundary_out, live_tc)),
                        ),
                        _ => (None, None),
                    }
                } else {
                    (None, None)
                };
                GeneratorInfo { name: gen_name.clone(), dim, source, target }
            })
            .collect();

        // All named diagrams (including generator classifiers).
        let diagrams: Vec<DiagramEntryInfo> = live_tc
            .diagrams_iter()
            .filter(|(n, _)| !n.is_empty())
            .map(|(diag_name, diag)| {
                let dim = diag.top_dim();
                let (source, target) = if dim > 0 {
                    let k = dim - 1;
                    let src = Diagram::boundary(Sign::Source, k, diag)
                        .ok()
                        .map(|d| diagram_info(&d, live_tc));
                    let tgt = Diagram::boundary(Sign::Target, k, diag)
                        .ok()
                        .map(|d| diagram_info(&d, live_tc));
                    (src, tgt)
                } else {
                    (None, None)
                };
                DiagramEntryInfo {
                    name: diag_name.clone(),
                    dim,
                    source,
                    target,
                    expr: render_diagram(diag, live_tc),
                }
            })
            .collect();

        let maps = build_map_entries(live_tc, store);

        Ok(TypeDetailInfo { name: name.to_owned(), generators, diagrams, maps })
    };

    let type_detail = type_detail?;
    let mut data = build_response(engine, false);
    data.type_detail = Some(type_detail);
    Ok(data)
}

/// Build a [`ResponseData`] with detail for a single named cell or let-binding.
pub fn build_cell_response(engine: &RewriteEngine, name: &str) -> Result<ResponseData, String> {
    let cell_detail: Result<CellDetailInfo, String> = {
        let scope = engine.type_complex();
        let store = engine.store();

        if let Some((tag, dim)) = scope.find_generator(name) {
            let (source, target) = if dim > 0 {
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
                source,
                target,
                expr: None,
            })
        } else if let Some(diag) = scope.find_diagram(name) {
            let dim = diag.top_dim();
            let (source, target) = if dim > 0 {
                let k = dim - 1;
                let src = Diagram::boundary(Sign::Source, k, diag)
                    .ok()
                    .map(|d| diagram_info(&d, scope));
                let tgt = Diagram::boundary(Sign::Target, k, diag)
                    .ok()
                    .map(|d| diagram_info(&d, scope));
                (src, tgt)
            } else {
                (None, None)
            };
            Ok(CellDetailInfo {
                name: name.to_owned(),
                dim,
                kind: "diagram".to_owned(),
                source,
                target,
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
        .generators_iter()
        .filter(|(_, _, dim)| *dim == n + 1)
        .filter_map(|(name, tag, _)| {
            match store.cell_data_for_tag(scope, tag)? {
                CellData::Boundary { boundary_in, boundary_out } => Some(RuleInfo {
                    name: name.clone(),
                    source: diagram_info(&boundary_in, scope),
                    target: diagram_info(&boundary_out, scope),
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
        Tag::Local(_) => serde_json::Value::Null,
    }
}

/// Serialize a [`StrDiag`] to a JSON value.
pub fn strdiag_to_json(sd: &StrDiag) -> serde_json::Value {
    fn edges_json(graph: &crate::core::graph::DiGraph) -> Vec<[usize; 2]> {
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
/// of the diagram first. Returns a JSON object with `strdiag`, `dim`, `src`,
/// and `tgt` fields.
fn diagram_strdiag_json(
    diagram: &Diagram,
    scope: &Complex,
    boundary: Option<(usize, &str)>,
) -> Result<serde_json::Value, String> {
    let target_diagram = match boundary {
        None => diagram.clone(),
        Some((k, sign_str)) => {
            let sign = match sign_str {
                "input" => Sign::Source,
                _ => Sign::Target,
            };
            Diagram::boundary(sign, k, diagram)
                .map_err(|e| format!("boundary extraction failed: {}", e))?
        }
    };

    let dim = target_diagram.top_dim();
    let (src, tgt) = if dim >= 1 {
        let s = Diagram::boundary(Sign::Source, dim - 1, &target_diagram)
            .map_err(|e| format!("{}", e))?;
        let t = Diagram::boundary(Sign::Target, dim - 1, &target_diagram)
            .map_err(|e| format!("{}", e))?;
        (
            render_diagram(&s, scope),
            render_diagram(&t, scope),
        )
    } else {
        (String::new(), String::new())
    };

    let label = render_diagram(&target_diagram, scope);
    let sd = StrDiag::from_diagram(&target_diagram, scope);

    Ok(serde_json::json!({
        "strdiag": strdiag_to_json(&sd),
        "dim": dim,
        "label": label,
        "src": src,
        "tgt": tgt,
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

/// Build a StrDiag JSON for the target boundary of a step diagram.
pub fn step_target_strdiag_json(
    step: &Diagram,
    scope: &Complex,
) -> Result<serde_json::Value, String> {
    let n = step.top_dim().checked_sub(1)
        .ok_or_else(|| "step diagram has dim 0".to_string())?;
    let tgt = Diagram::boundary(Sign::Target, n, step)
        .map_err(|e| format!("{}", e))?;
    Ok(strdiag_json_from_diagram(&tgt, scope))
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

    let generators: Vec<GeneratorInfo> = type_complex
        .generators_iter()
        .filter(|(n, _, _)| !n.is_empty())
        .map(|(gen_name, tag, dim)| {
            let (source, target) = if dim > 0 {
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
            GeneratorInfo { name: gen_name.clone(), dim, source, target }
        })
        .collect();

    let diagrams: Vec<DiagramEntryInfo> = type_complex
        .diagrams_iter()
        .filter(|(n, _)| !n.is_empty())
        .map(|(diag_name, diag)| {
            let dim = diag.top_dim();
            let (source, target) = if dim > 0 {
                let k = dim - 1;
                let src = Diagram::boundary(Sign::Source, k, diag)
                    .ok()
                    .map(|d| diagram_info(&d, &type_complex));
                let tgt = Diagram::boundary(Sign::Target, k, diag)
                    .ok()
                    .map(|d| diagram_info(&d, &type_complex));
                (src, tgt)
            } else {
                (None, None)
            };
            DiagramEntryInfo {
                name: diag_name.clone(),
                dim,
                source,
                target,
                expr: render_diagram(diag, &type_complex),
            }
        })
        .collect();

    let maps = build_map_entries(&type_complex, store);

    Ok(TypeDetailInfo { name: name.to_owned(), generators, diagrams, maps })
}

/// Compute cellular homology of a named type and return as JSON.
pub fn build_homology_response(
    store: &crate::interpreter::GlobalStore,
    source_path: &str,
    type_name: &str,
) -> Result<serde_json::Value, String> {
    let tc = super::engine::resolve_type(store, source_path, type_name)?;
    let h = crate::core::homology::compute_homology(&tc);
    let groups: Vec<serde_json::Value> = h.groups.iter()
        .map(|(dim, g)| serde_json::json!({
            "dim": dim,
            "display": format!("{}", g),
        }))
        .collect();
    Ok(serde_json::json!({
        "homology": groups,
        "euler_characteristic": h.euler_characteristic,
    }))
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

    let (source, target) = if is_single {
        let rule_tag = pr.step.labels_at(n_plus_1).and_then(|ls| ls.first());
        let classifier = rule_tag
            .and_then(|tag| scope.find_generator_by_tag(tag))
            .and_then(|name| scope.classifier(name));
        match classifier {
            Some(cl) => match (
                Diagram::boundary(Sign::Source, n, cl),
                Diagram::boundary(Sign::Target, n, cl),
            ) {
                (Ok(src), Ok(tgt)) => (diagram_info(&src, scope), diagram_info(&tgt, scope)),
                _ => (placeholder(), placeholder()),
            },
            None => (placeholder(), placeholder()),
        }
    } else {
        (
            Diagram::boundary(Sign::Source, n, &pr.step)
                .map(|d| diagram_info(&d, scope))
                .unwrap_or_else(|_| placeholder()),
            Diagram::boundary(Sign::Target, n, &pr.step)
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
        source,
        target,
        match_positions: pr.image_positions.clone(),
        match_display,
        family,
    }
}
