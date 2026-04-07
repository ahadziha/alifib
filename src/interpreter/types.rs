use super::global_store::GlobalStore;
use super::inference::{Constraint, HoleId};
use crate::aux::{GlobalId, LocalId, Tag};
use crate::aux::loader::ModuleResolutions;
use crate::core::{
    complex::Complex,
    diagram::{CellData, Diagram, Sign as DiagramSign},
    partial_map::PartialMap,
};
use crate::language::{ast::Span, error::Error};
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
    /// Resolution mappings from (parent path, module name) to canonical path,
    /// used by `IncludeModule` instructions to look up pre-interpreted modules.
    pub resolutions: Arc<ModuleResolutions>,
}

impl Context {
    /// Create a context with an empty global store and no module resolutions.
    pub fn new_empty(module_id: String) -> Self {
        Self {
            current_module: module_id,
            state: Arc::new(GlobalStore::default()),
            resolutions: Arc::new(ModuleResolutions::empty()),
        }
    }

    /// Create a context with explicit resolutions and pre-accumulated state.
    /// Used in the topo-order pre-interpretation loop.
    pub fn new_with_resolutions(
        module_id: String,
        resolutions: Arc<ModuleResolutions>,
        state: Arc<GlobalStore>,
    ) -> Self {
        Self { current_module: module_id, state, resolutions }
    }

    /// Create a context for a new module that shares global state and resolutions
    /// with `other`.
    pub fn new_sharing_state(module_id: String, other: &Context) -> Self {
        Self {
            current_module: module_id,
            state: Arc::clone(&other.state),
            resolutions: Arc::clone(&other.resolutions),
        }
    }

    /// Get a mutable reference to the state via `Arc::make_mut` (copy-on-write).
    pub fn state_mut(&mut self) -> &mut GlobalStore {
        Arc::make_mut(&mut self.state)
    }

    /// Mutate the current module's `Complex` in place via the global store.
    pub fn modify_current_module(&mut self, f: impl FnOnce(&mut Complex)) {
        let module_id = self.current_module.clone();
        self.state_mut().modify_module(&module_id, f);
    }
}


// ---- Hole info ----

/// Structured boundary data for a hole, stored without rendering.
/// Tracks a `?` hole encountered during interpretation.
///
/// Boundary information is derived after the fact by the constraint solver in
/// [`super::inference::solve`]; see [`super::load::InterpretedFile::solved_holes`].
#[derive(Debug, Clone)]
pub struct HoleInfo {
    /// Unique identifier used to link this hole to constraints in the solver.
    pub id: HoleId,
    /// Source location of the hole.
    pub span: Span,
    /// Source cell tag set when the hole appears as the RHS of a partial-map
    /// clause, so that `enrich_holes` can look up boundary data for it.
    pub source_tag: Option<Tag>,
    /// True when the hole is the *entire* RHS of a partial-map clause
    /// (e.g. `arr => ?`), not embedded in a composite (e.g. `arr => ? g`).
    ///
    /// When true, `enrich_holes` will attempt a full `PartialMap::apply` on the
    /// source cell's boundary diagrams and emit `BoundaryEq` if the map covers
    /// them completely, enabling consistency checking against other constraints.
    pub direct_in_partial_map: bool,
}

impl HoleInfo {
    /// Create a hole record at the given source location, with no boundary information yet.
    pub fn new(span: Span) -> Self {
        Self { id: HoleId::fresh(), span, source_tag: None, direct_in_partial_map: false }
    }
}

// ---- Interpretation result ----

/// The accumulated result of one or more interpretation steps.
///
/// Sequential steps are merged with `combine`, which advances the context
/// while collecting errors and holes from all steps.
#[must_use = "interpreter results carry errors and holes that must be propagated"]
#[derive(Debug, Clone)]
pub struct InterpResult {
    /// The updated context after this step.
    pub context: Context,
    /// Errors encountered during this step.  Interpretation continues past
    /// errors to collect as many diagnostics as possible.
    pub errors: Vec<Error>,
    /// Holes (`?`) found during this step, possibly enriched with boundary info.
    pub holes: Vec<HoleInfo>,
    /// Constraints emitted by inference sites for later solving.
    pub constraints: Vec<Constraint>,
}

/// A failable interpretation step: an optional produced value paired with an `InterpResult`.
///
/// A `None` value indicates that the step failed (errors will be in the result).
/// A `Some(v)` value means the step succeeded; the result still carries any holes.
pub type Step<T> = (Option<T>, InterpResult);

impl InterpResult {
    /// Create a successful result with no errors or holes.
    pub fn ok(context: Context) -> Self {
        Self {
            context,
            errors: vec![],
            holes: vec![],
            constraints: vec![],
        }
    }

    /// Append an error to this result.
    pub fn add_error(&mut self, err: Error) {
        self.errors.push(err);
    }

    /// Append a hole record to this result.
    pub fn add_hole(&mut self, hole: HoleInfo) {
        self.holes.push(hole);
    }

    /// Append a constraint to this result.
    pub fn add_constraint(&mut self, c: Constraint) {
        self.constraints.push(c);
    }

    /// Merge two sequential results: concatenate errors, holes, and constraints;
    /// advance to `next`'s context.
    pub fn merge(mut self, next: InterpResult) -> InterpResult {
        self.errors.extend(next.errors);
        self.holes.extend(next.holes);
        self.constraints.extend(next.constraints);
        self.context = next.context;
        self
    }

    /// Returns `true` if any errors have been recorded.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Returns `true` if any holes have been recorded.
    pub fn has_holes(&self) -> bool {
        !self.holes.is_empty()
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
    /// Global ID of the type whose body is being interpreted.
    pub owner_type_id: GlobalId,
    /// The `Complex` being accumulated for this type; a mutable local view
    /// that is written back to the store once the type body is complete.
    pub working_complex: Complex,
}

// ---- Term types ----

/// A partial map together with its domain complex, produced by evaluating a map expression.
#[derive(Debug, Clone)]
pub struct EvalMap {
    /// The evaluated partial map.
    pub map: PartialMap,
    /// The complex that is the domain of definition for `map`.
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

/// A component produced by name lookup, `.in`/`.out`, or a hole position.
///
/// Used as an intermediate representation before a component is placed in
/// a diagram context that resolves it to a concrete `Term`.
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
    /// A generator declaration.
    Generator,
    /// A `let` diagram binding.
    Diagram,
    /// A partial map definition.
    PartialMap,
}

impl NameKind {
    /// Returns the human-readable label for this kind, used in error messages.
    pub fn as_str(self) -> &'static str {
        match self {
            NameKind::Generator => "Generator",
            NameKind::Diagram => "Diagram",
            NameKind::PartialMap => "Partial map",
        }
    }
}

// ---- Error helpers ----

/// Construct a runtime error at the given span with the provided message.
pub fn make_error(span: Span, message: impl Into<String>) -> Error {
    Error::Runtime {
        message: message.into(),
        span,
        notes: vec![],
    }
}

/// Wrap a core-level error into a language-level runtime error at the given span.
pub fn make_error_from_core(span: Span, error: crate::aux::Error) -> Error {
    Error::Runtime {
        message: error.message,
        span,
        notes: error.notes,
    }
}

/// Create an `InterpResult` containing a single error at the given span.
pub fn error_result(context: &Context, span: Span, message: impl Into<String>) -> InterpResult {
    InterpResult {
        context: context.clone(),
        errors: vec![make_error(span, message)],
        holes: vec![],
        constraints: vec![],
    }
}

/// Create a failed `Step` with no value and a single error at the given span.
pub fn fail<T>(context: &Context, span: Span, message: impl Into<String>) -> Step<T> {
    (None, error_result(context, span, message))
}

/// Check that `name` is not already in use in `scope`.
///
/// Returns `Some(error_result)` if the name is already taken, `None` if it is free.
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

/// Collect all generators from a complex, sorted ascending by dimension.
pub fn sorted_generators(complex: &Complex) -> Vec<(usize, LocalId, Tag)> {
    let mut generators: Vec<(usize, LocalId, Tag)> = complex
        .generators_iter()
        .map(|(name, tag, dim)| (dim, name.clone(), tag.clone()))
        .collect();
    generators.sort_by_key(|(dim, _, _)| *dim);
    generators
}

/// Join a prefix and a name with a `.` separator, handling empty components.
///
/// If either part is empty, returns the other unchanged; otherwise returns `"prefix.name"`.
pub fn qualify_name(prefix: &str, name: &str) -> LocalId {
    if prefix.is_empty() {
        name.to_owned()
    } else if name.is_empty() {
        prefix.to_owned()
    } else {
        format!("{}.{}", prefix, name)
    }
}

// ---- Cell data lookup ----

/// Look up cell data for a tag, checking both cells and types in state,
/// and local cells in a complex.
pub fn get_cell_data(context: &Context, source: &Complex, tag: &Tag) -> Option<CellData> {
    context.state.cell_data_for_tag(source, tag)
}

/// Build an identity partial map for a complex using the state for cell data lookup.
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
