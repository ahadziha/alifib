#![allow(dead_code)]

use std::sync::Arc;
use crate::aux::{GlobalId, Tag};
use crate::core::{
    complex::Complex,
    diagram::{CellData, Diagram, Sign as DiagramSign},
    map::PMap,
};
use super::state::State;
use crate::language::{
    ast::Span,
    error::Error,
};

// ---- Context ----

#[derive(Debug, Clone)]
pub struct Context {
    pub current_module: String,
    pub state: Arc<State>,
}

impl Context {
    pub fn new(module_id: String, state: State) -> Self {
        Self { current_module: module_id, state: Arc::new(state) }
    }

    pub fn new_sharing_state(module_id: String, other: &Context) -> Self {
        Self { current_module: module_id, state: Arc::clone(&other.state) }
    }

    pub fn with_state(&self, state: State) -> Self {
        Self { current_module: self.current_module.clone(), state: Arc::new(state) }
    }

    /// Get a mutable reference to the state via Arc::make_mut (copy-on-write).
    pub fn state_mut(&mut self) -> &mut State {
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

impl InterpResult {
    pub fn ok(context: Context) -> Self {
        Self { context, errors: vec![], holes: vec![] }
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
        InterpResult { context: next.context, errors, holes }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

// ---- Mode ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode { Global, Local }

// ---- Namespace ----

#[derive(Debug, Clone)]
pub struct Namespace {
    pub root: GlobalId,
    pub location: Complex,
}

// ---- Term types ----

#[derive(Debug, Clone)]
pub struct MapComponent {
    pub map: PMap,
    pub source: Arc<Complex>,
}

#[derive(Debug, Clone)]
pub enum Term {
    MTerm(MapComponent),
    DTerm(Diagram),
}

#[derive(Debug, Clone)]
pub enum Component {
    Term(Term),
    Hole,
    Bd(DiagramSign),
}

#[derive(Debug, Clone)]
pub enum TermPair {
    MTermPair { fst: PMap, snd: PMap, source: Arc<Complex> },
    DTermPair { fst: Diagram, snd: Diagram },
}

// ---- Error helpers ----

pub fn unknown_span() -> Span {
    Span { start: 0, end: 0 }
}

pub fn make_error(span: Span, message: impl Into<String>) -> Error {
    Error::Runtime { message: message.into(), span }
}

// ---- Cell data lookup ----

/// Look up cell data for a tag, checking both cells and types in state,
/// and local cells in a complex.
pub fn get_cell_data(context: &Context, source: &Complex, tag: &Tag) -> Option<CellData> {
    match tag {
        Tag::Global(gid) => {
            context.state.find_cell(*gid)
                .map(|e| e.data.clone())
                .or_else(|| context.state.find_type(*gid).map(|e| e.data.clone()))
        }
        Tag::Local(name) => {
            source.find_local_cell(name).map(|e| e.data.clone())
        }
    }
}

/// Build an identity map for a complex using state for cell data lookup.
pub fn identity_map(context: &Context, domain: &Complex) -> PMap {
    let entries: Vec<(Tag, usize, CellData, Diagram)> = domain.generator_names()
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
