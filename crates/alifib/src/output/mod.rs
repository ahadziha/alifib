use crate::aux::loader::Loader;
use alifib_core::{render_diagram, render_boundary_partial, GlobalStore};
use crate::interpreter::{Context, HoleBd, HoleInfo, interpret_program};
use std::fmt;
use std::sync::Arc;

// ---- InterpretedFile ----

/// The result of interpreting a single alifib source file, ready for display.
pub struct InterpretedFile {
    pub state: Arc<GlobalStore>,
    pub holes: Vec<HoleInfo>,
    pub source: String,
    pub path: String,
}

impl InterpretedFile {
    /// Load, parse, and interpret a source file. Returns `None` and prints
    /// diagnostics to stderr if loading or interpretation fails.
    ///
    /// The pipeline has three phases:
    /// 1. Parse the root file and discover all transitively included modules.
    /// 2. Resolve the full dependency graph (handled inside `loader.load`).
    /// 3. Interpret every dependency in topological order (leaves first), then
    ///    interpret the root.  All results share a single accumulated `GlobalStore`.
    pub fn load(loader: &Loader, path: &str) -> Option<Self> {
        // Phase 1 + 2: read, parse, resolve all dependencies.
        let loaded = match loader.load(path) {
            Ok(f) => f,
            Err(e) => {
                crate::aux::error::report_load_file_error(&e);
                return None;
            }
        };

        let (resolutions, topo_modules) = loaded.modules.into_parts();
        let resolutions = Arc::new(resolutions);

        // Phase 3a: interpret dependency modules in topological order (leaves first).
        let mut prev_state = Arc::new(GlobalStore::empty());
        for (dep_path, dep_module) in &topo_modules {
            let dep_context = Context::new_with_resolutions(
                dep_path.clone(),
                Arc::clone(&resolutions),
                Arc::clone(&prev_state),
            );
            let dep_result = interpret_program(dep_context, &dep_module.program);
            if !dep_result.errors.is_empty() {
                crate::language::report_errors(&dep_result.errors, &dep_module.source, dep_path);
                return None;
            }
            prev_state = dep_result.context.state;
        }

        // Phase 3b: interpret the root module.
        let root_context = Context::new_with_resolutions(
            loaded.canonical_path.clone(),
            Arc::clone(&resolutions),
            prev_state,
        );
        let result = interpret_program(root_context, &loaded.program);

        if !result.errors.is_empty() {
            crate::language::report_errors(&result.errors, &loaded.source, &loaded.canonical_path);
            return None;
        }

        Some(Self {
            state: Arc::clone(&result.context.state),
            holes: result.holes,
            source: loaded.source,
            path: loaded.canonical_path,
        })
    }

    pub fn has_holes(&self) -> bool {
        !self.holes.is_empty()
    }

    /// Print hole diagnostics to stderr using ariadne.
    pub fn report_holes(&self) {
        for hole in &self.holes {
            let message = match &hole.boundary {
                Some(bd) => format!(
                    "{} -> {}",
                    render_hole_bd(&bd.boundary_in),
                    render_hole_bd(&bd.boundary_out)
                ),
                None => "unknown boundary".to_string(),
            };
            crate::language::error::report_hole(hole.span, &message, &self.source, &self.path);
        }
    }
}

impl fmt::Display for InterpretedFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.state)
    }
}

// ---- Hole rendering ----

fn render_hole_bd(bd: &HoleBd) -> String {
    match bd {
        HoleBd::Unknown => "?".to_string(),
        HoleBd::Full(diagram, scope) => render_diagram(diagram, scope),
        HoleBd::Partial { boundary, map, scope } => render_boundary_partial(boundary, map, scope),
    }
}
