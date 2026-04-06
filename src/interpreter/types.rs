#![allow(dead_code)]

use super::global_store::GlobalStore;
use crate::aux::{GlobalId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, Sign as DiagramSign},
    partial_map::PartialMap,
};
use crate::language::{ast::Span, error::Error};
use std::fmt;
use std::sync::Arc;

// ---- Context ----

/// The interpreter's read/write context, threaded through all interpretation steps.
///
/// `state` is shared via `Arc` so that modules included into the current module
/// can be interpreted without copying the store; mutations go through `state_mut`,
/// which uses `Arc::make_mut` for copy-on-write semantics.
#[derive(Debug, Clone)]
pub struct Context {
    /// Name of the module currently being interpreted.  Used to locate the
    /// module's `Complex` in the global store.
    pub current_module: String,
    /// Shared reference to the global persistent state (cells, types, modules).
    pub state: Arc<GlobalStore>,
}

impl Context {
    pub fn new(module_id: String, state: GlobalStore) -> Self {
        Self {
            current_module: module_id,
            state: Arc::new(state),
        }
    }

    pub fn new_empty(module_id: String) -> Self {
        Self::new(module_id, GlobalStore::empty())
    }

    /// Create a Context for a new module that shares global state with `other`.
    /// Used when interpreting an included module so that types created there
    /// are visible in the parent module without copying the store.
    pub fn new_sharing_state(module_id: String, other: &Context) -> Self {
        Self {
            current_module: module_id,
            state: Arc::clone(&other.state),
        }
    }

    /// Get a mutable reference to the state via Arc::make_mut (copy-on-write).
    pub fn state_mut(&mut self) -> &mut GlobalStore {
        Arc::make_mut(&mut self.state)
    }

    /// Mutate the current module's Complex in place.
    pub fn modify_current_module(&mut self, f: impl FnOnce(&mut Complex)) {
        let module_id = self.current_module.clone();
        self.state_mut().modify_module(&module_id, f);
    }
}

impl fmt::Display for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.state)
    }
}

// ---- Hole info ----

/// Tracks a `?` hole encountered during interpretation.
///
/// Holes are created without boundary information and enriched with
/// `HoleBoundaryInfo` later when a surrounding `partial_map` clause provides context.
#[derive(Debug, Clone)]
pub struct HoleInfo {
    /// Source location of the hole.
    pub span: Span,
    /// Boundary context for the hole; `None` until the enclosing map clause provides it.
    pub boundary: Option<HoleBoundaryInfo>,
    /// Source cell tag, for deferred boundary computation in partial_map context.
    pub source_tag: Option<Tag>,
}

impl HoleInfo {
    pub fn new(span: Span) -> Self {
        Self { span, boundary: None, source_tag: None }
    }
}

/// The pretty-printed source/target boundary context for a hole.
#[derive(Debug, Clone)]
pub struct HoleBoundaryInfo {
    /// Pretty-printed source (input) boundary of the hole.
    pub boundary_in: String,
    /// Pretty-printed target (output) boundary of the hole.
    pub boundary_out: String,
}

// ---- Interpretation result ----

/// The accumulated result of one or more interpretation steps.
///
/// Sequential steps are merged with `combine`, which advances the context
/// while collecting errors and holes from all steps.
#[derive(Debug, Clone)]
pub struct InterpResult {
    /// The updated context after this step.
    pub context: Context,
    /// Errors encountered during this step.  Interpretation continues past
    /// errors to collect as many diagnostics as possible.
    pub errors: Vec<Error>,
    /// Holes (`?`) found during this step, possibly enriched with boundary info.
    pub holes: Vec<HoleInfo>,
}

pub type Step<T> = (Option<T>, InterpResult);

impl InterpResult {
    pub fn ok(context: Context) -> Self {
        Self {
            context,
            errors: vec![],
            holes: vec![],
        }
    }

    pub fn add_error(&mut self, err: Error) {
        self.errors.push(err);
    }

    pub fn add_hole(&mut self, hole: HoleInfo) {
        self.holes.push(hole);
    }

    pub fn combine(prev: InterpResult, next: InterpResult) -> InterpResult {
        let mut errors = prev.errors;
        errors.extend(next.errors);
        let mut holes = prev.holes;
        holes.extend(next.holes);
        InterpResult {
            context: next.context,
            errors,
            holes,
        }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn has_holes(&self) -> bool {
        !self.holes.is_empty()
    }

    pub fn report_holes(&self, source: &str, path: &str) {
        for hole in &self.holes {
            let message = match &hole.boundary {
                Some(bd) => format!("{} -> {}", bd.boundary_in, bd.boundary_out),
                None => "unknown boundary".to_string(),
            };
            crate::language::error::report_hole(hole.span, &message, source, path);
        }
    }
}

// ---- Mode ----

/// Scoping mode for the current interpretation step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Definitions are committed to the global store and visible to all modules.
    Global,
    /// Definitions remain in a temporary local scope inside a type body.
    Local,
}

// ---- TypeScope ----

/// The local environment being built while interpreting a type body.
///
/// After the body is fully interpreted, `working_complex` is committed back
/// to the `GlobalStore` under `owner_type_id`.
#[derive(Debug, Clone)]
pub struct TypeScope {
    /// GlobalId of the type whose body is being interpreted.
    pub owner_type_id: GlobalId,
    /// The Complex being accumulated for this type; a mutable local view
    /// that is written back to the store once the type body is complete.
    pub working_complex: Complex,
}

// ---- Term types ----

/// A partial map together with its domain complex, the result of evaluating a map expression.
#[derive(Debug, Clone)]
pub struct EvalMap {
    pub map: PartialMap,
    pub domain: Arc<Complex>,
}

/// A fully evaluated expression: either a diagram or a partial map.
#[derive(Debug, Clone)]
pub enum Term {
    /// A partial map together with its domain complex.
    Map(EvalMap),
    /// A diagram.
    Diag(Diagram),
}

/// A component produced by the `.in` / `.out` boundary operators.
#[derive(Debug, Clone)]
pub enum Component {
    /// A concrete term (diagram or map).
    Value(Term),
    /// An unresolved position (`?`) in the diagram.
    Hole,
    /// A boundary direction requested via `.in` or `.out`.
    Bd(DiagramSign),
}

/// Two evaluated terms for comparison in an equality assertion.
#[derive(Debug, Clone)]
pub enum TermPair {
    /// Two partial maps with a shared domain complex.
    Maps {
        fst: PartialMap,
        snd: PartialMap,
        domain: Arc<Complex>,
    },
    /// Two diagrams.
    Diagrams {
        fst: Diagram,
        snd: Diagram,
    },
}

/// The kind of binding being declared, for use in duplicate-name error messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    Generator,
    Diagram,
    PartialMap,
}

impl NameKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NameKind::Generator => "Generator",
            NameKind::Diagram => "Diagram",
            NameKind::PartialMap => "Partial map",
        }
    }
}

// ---- Error helpers ----

pub fn unknown_span() -> Span {
    Span { start: 0, end: 0 }
}

pub fn make_error(span: Span, message: impl Into<String>) -> Error {
    Error::Runtime {
        message: message.into(),
        span,
    }
}

pub fn error_result(context: &Context, span: Span, message: impl Into<String>) -> InterpResult {
    let mut result = InterpResult::ok(context.clone());
    result.add_error(make_error(span, message));
    result
}

pub fn fail<T>(context: &Context, span: Span, message: impl Into<String>) -> Step<T> {
    (None, error_result(context, span, message))
}

pub fn ensure_name_free(
    context: &Context,
    scope: &Complex,
    name: &str,
    span: Span,
    kind: NameKind,
) -> Option<InterpResult> {
    if scope.name_in_use(name) {
        Some(error_result(context, span, format!("{} name already in use: {}", kind.as_str(), name)))
    } else {
        None
    }
}

pub fn sorted_generators(complex: &Complex) -> Vec<(usize, LocalId, Tag)> {
    let mut generators: Vec<(usize, LocalId, Tag)> = complex
        .generators_iter()
        .map(|(name, tag, dim)| (dim, name.clone(), tag.clone()))
        .collect();
    generators.sort_by_key(|(dim, _, _)| *dim);
    generators
}

pub fn qualify_name(prefix: &str, name: &str) -> LocalId {
    if prefix.is_empty() {
        name.to_owned()
    } else if name.is_empty() {
        prefix.to_owned()
    } else {
        format!("{}.{}", prefix, name)
    }
}

pub fn resolve_root_owner_type_id(
    context: &Context,
    module_space: &Complex,
    span: Span,
) -> Step<GlobalId> {
    let empty_name: LocalId = String::new();
    let mut result = InterpResult::ok(context.clone());

    let Some((root_tag, _)) = module_space.find_generator(&empty_name) else {
        result.add_error(make_error(span, "Root generator not found"));
        return (None, result);
    };

    match root_tag {
        Tag::Global(id) => (Some(*id), result),
        Tag::Local(_) => {
            result.add_error(make_error(span, "Root has local tag (unexpected)"));
            (None, result)
        }
    }
}

pub fn resolve_type_complex(
    context: &Context,
    type_id: GlobalId,
    span: Span,
    missing_prefix: &str,
) -> Step<Arc<Complex>> {
    let mut result = InterpResult::ok(context.clone());
    let Some(type_entry) = context.state.find_type(type_id) else {
        result.add_error(make_error(span, format!("{} {}", missing_prefix, type_id)));
        return (None, result);
    };
    (Some(Arc::clone(&type_entry.complex)), result)
}

pub fn resolve_map_domain_complex(
    context: &Context,
    domain: &MapDomain,
    span: Span,
) -> Step<Arc<Complex>> {
    let mut result = InterpResult::ok(context.clone());
    match domain {
        MapDomain::Type(id) => {
            let Some(type_entry) = context.state.find_type(*id) else {
                result.add_error(make_error(span, format!("Type {} not found", id)));
                return (None, result);
            };
            (Some(Arc::clone(&type_entry.complex)), result)
        }
        MapDomain::Module(mid) => {
            let Some(module_arc) = context.state.find_module_arc(mid) else {
                result.add_error(make_error(span, format!("Module `{}` not found", mid)));
                return (None, result);
            };
            (Some(module_arc), result)
        }
    }
}

// ---- Cell data lookup ----

/// Look up cell data for a tag, checking both cells and types in state,
/// and local cells in a complex.
pub fn get_cell_data(context: &Context, source: &Complex, tag: &Tag) -> Option<CellData> {
    context.state.cell_data_for_tag(source, tag)
}

/// Build an identity map for a complex using state for cell data lookup.
pub fn identity_map(context: &Context, domain: &Complex) -> PartialMap {
    let entries: Vec<(Tag, usize, CellData, Diagram)> = domain
        .generators_iter()
        .filter_map(|(name, tag, dim)| {
            let cell_data = get_cell_data(context, domain, tag)?;
            let image = domain.classifier(name)?.clone();
            Some((tag.clone(), dim, cell_data, image))
        })
        .collect();
    PartialMap::of_entries(entries, true)
}
