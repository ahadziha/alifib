//! The loaded session's state and the answers commands add to it.

use serde::Serialize;

use crate::aux::Tag;
use crate::core::complex::{Complex, MapDomain};
use crate::core::diagram::{CellData, Diagram, Sign};
use crate::core::matching::MatchResult;
use crate::core::partial_map::PartialMap;
use crate::output::render_diagram;
use super::engine::{RewriteEngine, Steps};
use super::render::render_step;

/// The image of a loaded session's activity, or nothing loaded.
#[derive(Debug, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum State {
    Unloaded,
    Idle { module: String, backward: bool },
    /// `activity` changes whenever a rewrite, resume, or fill begins.
    Rewrite { activity: u64, backward: bool, rewrite: RewriteState },
    Fill { activity: u64, backward: bool, hole: FillInfo, by: FillState },
}

/// How the active hole is being filled.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FillState {
    Rewrite(RewriteState),
    ZeroCell(ZeroCellInfo),
}

/// An engine's current diagrams and available rewrites.
#[derive(Debug, Serialize)]
pub struct RewriteState {
    pub step_count: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    pub current: DiagramInfo,
    pub initial: DiagramInfo,
    pub target: Option<DiagramInfo>,
    pub target_reached: bool,
    pub parallel: bool,
    pub rewrites: Vec<RewriteInfo>,
}

/// What a command adds; state is read separately, never embedded in an answer.
pub struct Answer {
    pub body: Body,
    /// Whether the command rewrote the running source and store.
    pub edited: bool,
}

/// A one-line message, or an outcome the transcript renders: never both.
pub enum Body {
    Message(String),
    Outcome(Outcome),
}

impl Answer {
    pub fn message(text: impl Into<String>) -> Self {
        Self { body: Body::Message(text.into()), edited: false }
    }

    pub fn outcome(outcome: Outcome) -> Self {
        Self { body: Body::Outcome(outcome), edited: false }
    }
}

pub enum Outcome {
    Help,
    State,
    Auto(AutoInfo),
    Types(Vec<TypeInfo>),
    Type(TypeInfo),
    Homology(HomologyInfo),
    Holes { holes: Vec<HoleInfo>, constraints: Vec<ConstraintInfo> },
    Rules(Vec<RuleInfo>),
    History(Vec<HistoryEntry>),
    /// The running proof; absent while a 0-cell fill has no choice.
    Proof(Option<Proof>),
    Stored(StoredInfo),
    Source(String),
}

/// Everything the browser derives from the loaded module.
#[derive(Serialize)]
pub struct ModuleInfo {
    pub base: Option<String>,
    pub root: String,
    pub types: Vec<TypeInfo>,
    pub holes: Vec<HoleInfo>,
    pub constraints: Vec<ConstraintInfo>,
    pub source: String,
}

/// Cellular homology of a type, for the `homology` response.
#[derive(Debug)]
pub struct HomologyInfo {
    pub groups: Vec<HomologyGroupInfo>,
    pub euler_characteristic: i64,
}

/// One homology group `H_d`, with its display string (e.g. `Z`, `Z/2`) and the
/// torsion witnesses for its finite-order classes (empty if `H_d` is free).
#[derive(Debug)]
pub struct HomologyGroupInfo {
    pub dim: usize,
    pub display: String,
    pub witnesses: Vec<TorsionWitnessInfo>,
}

/// A torsion class of `H_d`, made self-certifying: the witnessing `d`-cycle and
/// the `(d+1)`-chain preimage `w` with `d(w) = order · cycle`.
#[derive(Debug)]
pub struct TorsionWitnessInfo {
    pub order: i64,
    pub cycle: String,
    pub preimage: String,
}

/// One open hole of a map in the current module, numbered for `fill`.
#[derive(Debug, Clone, Serialize)]
pub struct HoleInfo {
    pub module: String,
    pub index: usize,
    pub type_name: String,
    pub map_name: String,
    pub domain_name: String,
    pub source_name: String,
    pub dim: usize,
    pub boundary: String,
}

/// The constraints a single conditional pending assignment (`x => a`) imposes on
/// a map's holes: one `lhs = rhs` equation per boundary side of `x` that still
/// contains metavariables.  Display-only — there is no command that picks one.
#[derive(Debug, Clone, Serialize)]
pub struct ConstraintInfo {
    pub module: String,
    pub type_name: String,
    pub map_name: String,
    pub domain_name: String,
    /// Each equation `F(x.side) = a.side`, one per hole-bearing boundary side.
    pub equations: Vec<String>,
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

/// Populated for `auto` and `random` responses.
#[derive(Debug)]
pub struct AutoInfo {
    pub applied: usize,
    pub stop_reason: Option<&'static str>,
}

/// Populated for successful `store` responses, including zero-step proofs.
#[derive(Debug)]
pub struct StoredInfo {
    pub type_name: String,
    pub def_name: String,
    /// The rendered proof diagram as an alifib expression suitable for
    /// pasting into a `let` binding.
    pub expr: String,
}

/// A single generator with optional boundary, for `type_info`.
#[derive(Debug, Serialize)]
pub struct GeneratorInfo {
    pub tag: Option<usize>,
    pub face_tags: Vec<usize>,
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

/// Everything the front ends show about a type.
#[derive(Debug, Serialize)]
pub struct TypeInfo {
    pub name: String,
    /// The full module path, as keyed by the store.
    pub module: String,
    pub thin_tags: Vec<usize>,
    pub generators: Vec<GeneratorInfo>,
    pub diagrams: Vec<DiagramEntryInfo>,
    pub maps: Vec<MapEntry>,
}

/// Rich information about a diagram for client display.
#[derive(Debug, Serialize)]
pub struct DiagramInfo {
    /// The rendered diagram expression.
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
#[derive(Debug)]
pub struct RuleInfo {
    pub name: String,
    pub input: DiagramInfo,
    pub output: DiagramInfo,
}

/// The expression `store` would persist, and the rendered boundary of the
/// (n+1)-cell it denotes; a 0-cell has none.
#[derive(Debug)]
pub struct Proof {
    pub boundary: Option<(String, String)>,
    pub expr: String,
}

/// A single entry in the move history.
#[derive(Debug)]
pub struct HistoryEntry {
    pub step: usize,
    pub rule_name: String,
    pub choice: Option<Vec<usize>>,
}

// ── Builders ──────────────────────────────────────────────────────────────────

/// Build a [`DiagramInfo`] from a diagram and its rendering scope.
///
/// Collects the rendered expression, dimension, top-dim cell count,
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
            if let Some(name) = domain_complex.find_generator_by_tag(tag)
                && !name.is_empty()
            {
                gens.push(MapDomainGenerator { name: name.clone(), dim });
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

/// Build the image of a rewrite engine, without history or proof.
pub fn rewrite_state(engine: &RewriteEngine) -> RewriteState {
    let scope = engine.type_complex();
    RewriteState {
        step_count: engine.step_count(),
        can_undo: engine.applied() > 0,
        can_redo: engine.can_redo(),
        current: diagram_info(engine.current_diagram(), scope),
        initial: diagram_info(engine.initial_diagram(), scope),
        target: engine.target_diagram().map(|t| diagram_info(t, scope)),
        target_reached: engine.target_reached(),
        parallel: engine.parallel(),
        rewrites: engine.rewrites().iter().enumerate()
            .map(|(i, pr)| build_rewrite_info_from_family(i, pr, scope)).collect(),
    }
}

/// The running proof, as [`RewriteEngine::proof_expr`] renders it.
pub fn proof(engine: &RewriteEngine) -> Proof {
    let scope = engine.type_complex();
    let (input, output) = if engine.backward() {
        (engine.current_diagram(), engine.initial_diagram())
    } else {
        (engine.initial_diagram(), engine.current_diagram())
    };
    Proof {
        boundary: Some((render_diagram(input, scope), render_diagram(output, scope))),
        expr: engine.proof_expr(),
    }
}

/// The full move history, separate from the engine's state.
pub fn build_history(engine: &RewriteEngine) -> Vec<HistoryEntry> {
    engine.history().enumerate().map(|(i, e)| HistoryEntry {
        step: i + 1,
        rule_name: e.rule_name.clone(),
        choice: e.choice.clone(),
    }).collect()
}

/// Every rewrite rule at the current dimension, whether or not it matches.
pub fn build_list_rules(engine: &RewriteEngine) -> Vec<RuleInfo> {
    let scope = engine.type_complex();
    let store = engine.store();
    let n = engine.current_diagram().top_dim();
    scope.generators_iter_by_dim(n + 1).filter_map(|(name, tag)| {
        match store.cell_data_for_tag(scope, tag)? {
            CellData::Boundary { boundary_in, boundary_out } => Some(RuleInfo {
                name: name.clone(),
                input: diagram_info(&boundary_in, scope),
                output: diagram_info(&boundary_out, scope),
            }),
            CellData::Zero => None,
        }
    }).collect()
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
            .map(|(dim, g)| HomologyGroupInfo {
                dim: *dim,
                display: format!("{}", g),
                witnesses: h.torsion_witnesses.get(dim).map_or_else(Vec::new, |ws| {
                    ws.iter().map(|w| TorsionWitnessInfo {
                        order: w.order,
                        cycle: w.cycle_str(),
                        preimage: w.preimage_str(),
                    }).collect()
                }),
            })
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
