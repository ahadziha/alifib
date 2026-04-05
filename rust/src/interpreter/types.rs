#![allow(dead_code)]

use super::global_store::GlobalStore;
use crate::aux::{GlobalId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, Sign as DiagramSign},
    map::PMap,
};
use crate::language::{ast::Span, error::Error};
use std::sync::Arc;

// ---- Context ----

#[derive(Debug, Clone)]
pub struct Context {
    pub current_module: String,
    pub state: Arc<GlobalStore>,
}

impl Context {
    pub fn new(module_id: String, state: GlobalStore) -> Self {
        Self {
            current_module: module_id,
            state: Arc::new(state),
        }
    }

    pub fn new_sharing_state(module_id: String, other: &Context) -> Self {
        Self {
            current_module: module_id,
            state: Arc::clone(&other.state),
        }
    }

    pub fn with_state(&self, state: GlobalStore) -> Self {
        Self {
            current_module: self.current_module.clone(),
            state: Arc::new(state),
        }
    }

    /// Get a mutable reference to the state via Arc::make_mut (copy-on-write).
    pub fn state_mut(&mut self) -> &mut GlobalStore {
        Arc::make_mut(&mut self.state)
    }
}

// ---- Hole info ----

#[derive(Debug, Clone)]
pub struct HoleInfo {
    pub span: Span,
    pub boundary: Option<HoleBoundaryInfo>,
    /// Source cell tag, for deferred boundary computation in pmap context.
    pub source_tag: Option<Tag>,
}

#[derive(Debug, Clone)]
pub struct HoleBoundaryInfo {
    pub boundary_in: String,
    pub boundary_out: String,
}

// ---- Interpretation result ----

#[derive(Debug, Clone)]
pub struct InterpResult {
    pub context: Context,
    pub errors: Vec<Error>,
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
}

// ---- Mode ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Global,
    Local,
}

// ---- TypeScope ----

#[derive(Debug, Clone)]
pub struct TypeScope {
    pub owner_type_id: GlobalId,
    pub working_complex: Complex,
}

// ---- Term types ----

/// A partial map together with its domain complex, the result of evaluating a map expression.
#[derive(Debug, Clone)]
pub struct EvalMap {
    pub map: PMap,
    pub domain: Arc<Complex>,
}

#[derive(Debug, Clone)]
pub enum Term {
    Map(EvalMap),
    Diag(Diagram),
}

#[derive(Debug, Clone)]
pub enum Component {
    Value(Term),
    Hole,
    Bd(DiagramSign),
}

#[derive(Debug, Clone)]
pub enum TermPair {
    Maps {
        fst: PMap,
        snd: PMap,
        domain: Arc<Complex>,
    },
    Diagrams {
        fst: Diagram,
        snd: Diagram,
    },
}

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

pub fn fail<T>(context: &Context, span: Span, message: impl Into<String>) -> Step<T> {
    let mut result = InterpResult::ok(context.clone());
    result.add_error(make_error(span, message));
    (None, result)
}

pub fn ensure_name_free(
    context: &Context,
    scope: &Complex,
    name: &str,
    span: Span,
    kind: NameKind,
) -> Option<InterpResult> {
    if scope.name_in_use(name) {
        let mut result = InterpResult::ok(context.clone());
        result.add_error(make_error(
            span,
            format!("{} name already in use: {}", kind.as_str(), name),
        ));
        Some(result)
    } else {
        None
    }
}

pub fn dim_index(dim: isize) -> usize {
    dim.max(0) as usize
}

pub fn sorted_generators(complex: &Complex) -> Vec<(usize, LocalId, Tag)> {
    let mut generators: Vec<(usize, LocalId, Tag)> = complex
        .generator_names()
        .into_iter()
        .filter_map(|name| {
            complex
                .find_generator(&name)
                .map(|entry| (entry.dim, name, entry.tag.clone()))
        })
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

    let Some(root_entry) = module_space.find_generator(&empty_name) else {
        result.add_error(make_error(span, "Root generator not found"));
        return (None, result);
    };

    match root_entry.tag {
        Tag::Global(id) => (Some(id), result),
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
) -> Step<Complex> {
    let mut result = InterpResult::ok(context.clone());
    let Some(type_entry) = context.state.find_type(type_id) else {
        result.add_error(make_error(span, format!("{} {}", missing_prefix, type_id)));
        return (None, result);
    };
    (Some((*type_entry.complex).clone()), result)
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
pub fn identity_map(context: &Context, domain: &Complex) -> PMap {
    let entries: Vec<(Tag, usize, CellData, Diagram)> = domain
        .generator_names()
        .into_iter()
        .filter_map(|name| {
            let gen_entry = domain.find_generator(&name)?;
            let tag = gen_entry.tag.clone();
            let dim = gen_entry.dim;
            let cell_data = get_cell_data(context, domain, &tag)?;
            let image = domain.classifier(&name)?.clone();
            Some((tag, dim, cell_data, image))
        })
        .collect();
    PMap::of_entries(entries, true)
}
