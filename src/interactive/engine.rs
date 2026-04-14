//! Stateful rewrite engine: holds session state in memory for O(1) undo
//! and incremental step/apply without re-interpreting the source file.

use crate::aux::loader::Loader;
use crate::aux::Tag;
use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, Sign};
use crate::core::matching::{MatchResult, find_matches};
use crate::interpreter::{GlobalStore, InterpretedFile};
use super::session::{Move, SessionFile};
use std::sync::Arc;

/// A snapshot of a single past step, stored for O(1) undo.
struct HistoryEntry {
    /// The serialisable move record (choice + rule name).
    mov: Move,
    /// The current n-diagram *before* this step was applied.
    prev_diagram: Diagram,
    /// The running (n+1)-diagram *before* this step was applied.
    prev_running: Option<Diagram>,
}

/// Stateful rewrite session engine.
///
/// Load once with [`RewriteEngine::init`] or [`RewriteEngine::from_session`];
/// then use [`step`](RewriteEngine::step), [`undo`](RewriteEngine::undo), and
/// the accessor methods to drive the session without re-interpreting the
/// source file.
pub struct RewriteEngine {
    // Immutable context (loaded once)
    store: Arc<GlobalStore>,
    type_complex: Arc<Complex>,
    source_diagram: Diagram,
    target_diagram: Option<Diagram>,

    // Mutable session state
    current_diagram: Diagram,
    running_diagram: Option<Diagram>,
    history: Vec<HistoryEntry>,
    available_rewrites: Vec<MatchResult>,

    // Metadata (for session file persistence)
    source_file: String,
    type_name: String,
    source_diagram_name: String,
    target_diagram_name: Option<String>,
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Load a file and return the store, canonical path, and the interpreter's
/// display output (the same string `alifib <file>` would print to stdout).
///
/// Used by the REPL to load context eagerly at startup so that `types`,
/// `rules`, and `info` commands can work before the user selects a type.
pub fn load_file_context(
    source_file: &str,
) -> Result<(Arc<GlobalStore>, String, String), String> {
    let loader = Loader::default(vec![]);
    let file = InterpretedFile::load(&loader, source_file)
        .into_result()
        .map_err(|_| format!("failed to interpret '{}'", source_file))?;
    let canonical_path = file.path.clone();
    let output = file.to_string();
    let store = Arc::clone(&file.state);
    Ok((store, canonical_path, output))
}

/// Resolve a type name to its [`Complex`] given an already-loaded store.
///
/// Called by the REPL when the user types `@ <TypeName>`.
pub fn resolve_type(
    store: &GlobalStore,
    canonical_path: &str,
    type_name: &str,
) -> Result<Arc<Complex>, String> {
    let module_complex = store
        .find_module(canonical_path)
        .ok_or_else(|| format!("module '{}' not found in store", canonical_path))?;

    let (type_tag, _) = module_complex
        .find_generator(type_name)
        .ok_or_else(|| format!("type '{}' not found", type_name))?;

    let type_gid = match type_tag {
        Tag::Global(gid) => *gid,
        Tag::Local(_) => return Err(format!("'{}' is a local cell, not a type", type_name)),
    };

    store
        .find_type(type_gid)
        .ok_or_else(|| format!("type entry for '{}' not found", type_name))
        .map(|e| Arc::clone(&e.complex))
}

/// Load a file and return the store, type complex, and canonical file path —
/// without locating any diagrams.
///
/// Convenience wrapper over [`load_file_context`] + [`resolve_type`].
/// Used by session_repl, cli, and daemon which always know the type upfront.
pub fn load_type_context(
    source_file: &str,
    type_name: &str,
) -> Result<(Arc<GlobalStore>, Arc<Complex>, String), String> {
    let (store, canonical_path, _output) = load_file_context(source_file)?;
    let type_complex = resolve_type(&store, &canonical_path, type_name)?;
    Ok((store, type_complex, canonical_path))
}

type LoadedRewriteContext = (Arc<GlobalStore>, Arc<Complex>, Diagram, Option<Diagram>);

/// Resolve a source or target diagram from either a name or a diagram expression.
///
/// First tries a fast named lookup in `type_complex` and the module complex at
/// `module_key` in `store`. If neither succeeds, parses `expr` as a diagram
/// expression and evaluates it against `type_complex` using the interpreter.
///
/// This means `source <name>` and `source f g` both work at the REPL.
pub fn eval_diagram_expr(
    store: &Arc<GlobalStore>,
    type_complex: &Complex,
    module_key: &str,
    expr: &str,
) -> Result<Diagram, String> {
    // Fast path: named lookup.
    if let Some(d) = type_complex.find_diagram(expr) {
        return Ok(d.clone());
    }
    if let Some(d) = store.find_module(module_key).and_then(|m| m.find_diagram(expr)) {
        return Ok(d.clone());
    }
    // Slow path: parse and interpret as a diagram expression.
    let ast = crate::language::parse_diagram(expr)
        .map_err(|e| format!("'{}' is not a diagram name or valid expression: {}", expr, e))?;
    let ctx = crate::interpreter::Context::new_with_resolutions(
        module_key.to_owned(),
        std::sync::Arc::new(crate::aux::loader::ModuleResolutions::empty()),
        Arc::clone(store),
    );
    let (diagram_opt, interp_result) = crate::interpreter::interpret_diagram(&ctx, type_complex, &ast);
    if interp_result.has_errors() {
        let msgs: Vec<String> = interp_result.errors.iter()
            .map(|e| format!("{}", e))
            .collect();
        return Err(format!("'{}': {}", expr, msgs.join("; ")));
    }
    diagram_opt.ok_or_else(|| format!("'{}' did not produce a diagram", expr))
}

/// Load a file and locate the type complex and source/target diagrams.
///
/// Source and target may be diagram names or diagram expressions.
fn load_context(
    source_file: &str,
    type_name: &str,
    source_diagram_name: &str,
    target_diagram_name: Option<&str>,
) -> Result<LoadedRewriteContext, String> {
    let (store, type_complex, canonical_path) = load_type_context(source_file, type_name)?;

    let source_diagram =
        eval_diagram_expr(&store, &type_complex, &canonical_path, source_diagram_name)?;
    let target_diagram = target_diagram_name
        .map(|expr| eval_diagram_expr(&store, &type_complex, &canonical_path, expr))
        .transpose()?;

    Ok((store, type_complex, source_diagram, target_diagram))
}

fn compute_rewrites(
    store: &GlobalStore,
    type_complex: &Complex,
    current: &Diagram,
) -> Result<Vec<MatchResult>, String> {
    let n = current.top_dim();
    let mut all_matches = Vec::new();

    for (name, _tag, dim) in type_complex.generators_iter() {
        if dim != n + 1 { continue; }
        let Some(rewrite) = type_complex.classifier(name) else { continue; };

        match find_matches(type_complex, rewrite, current, name) {
            Ok(matches) => all_matches.extend(matches),
            Err(e) => return Err(format!("failed to match rule '{}': {}", name, e)),
        }
    }

    // Stable sort by (rule_name, image_positions) for deterministic indexing.
    all_matches.sort_by(|a, b| {
        a.rule_name.cmp(&b.rule_name)
            .then_with(|| a.image_positions.cmp(&b.image_positions))
    });

    Ok(all_matches)
}

// ── Constructor impls ─────────────────────────────────────────────────────────

impl RewriteEngine {
    /// Create a fresh session from an already-loaded store and type complex.
    ///
    /// `source_diagram_name` and `target_diagram_name` may be either plain diagram
    /// names (resolved against `type_complex` and the module at `source_file`) or
    /// full diagram expressions in the alifib language (e.g. `"f g"` or `"(f #0 g)"`).
    /// This constructor is used by the workspace session REPL and the interactive REPL.
    pub fn from_store(
        store: Arc<GlobalStore>,
        type_complex: Arc<Complex>,
        source_diagram_name: &str,
        target_diagram_name: Option<&str>,
        source_file: String,
        type_name: String,
    ) -> Result<Self, String> {
        let source_diagram =
            eval_diagram_expr(&store, &type_complex, &source_file, source_diagram_name)?;
        let target_diagram = target_diagram_name
            .map(|expr| eval_diagram_expr(&store, &type_complex, &source_file, expr))
            .transpose()?;

        let available_rewrites = compute_rewrites(&store, &type_complex, &source_diagram)?;

        Ok(Self {
            current_diagram: source_diagram.clone(),
            running_diagram: None,
            history: Vec::new(),
            available_rewrites,
            source_file,
            type_name,
            source_diagram_name: source_diagram_name.to_owned(),
            target_diagram_name: target_diagram_name.map(str::to_owned),
            store,
            type_complex,
            source_diagram,
            target_diagram,
        })
    }

    /// Load the source file and initialise a fresh session (no moves applied).
    pub fn init(
        source_file: &str,
        type_name: &str,
        source_diagram_name: &str,
        target_diagram_name: Option<&str>,
    ) -> Result<Self, String> {
        let (store, type_complex, source_diagram, target_diagram) =
            load_context(source_file, type_name, source_diagram_name, target_diagram_name)?;

        let available_rewrites = compute_rewrites(&store, &type_complex, &source_diagram)?;

        Ok(Self {
            current_diagram: source_diagram.clone(),
            running_diagram: None,
            history: Vec::new(),
            available_rewrites,
            store,
            type_complex,
            source_diagram,
            target_diagram,
            source_file: source_file.to_owned(),
            type_name: type_name.to_owned(),
            source_diagram_name: source_diagram_name.to_owned(),
            target_diagram_name: target_diagram_name.map(str::to_owned),
        })
    }

    /// Load the source file and replay a saved session, building undo snapshots
    /// incrementally so that subsequent [`step`](Self::step) / [`undo`](Self::undo)
    /// calls are O(diagram) with no further replay.
    pub fn from_session(session: SessionFile) -> Result<Self, String> {
        let (store, type_complex, source_diagram, target_diagram) = load_context(
            &session.source_file,
            &session.type_name,
            &session.source_diagram,
            session.target_diagram.as_deref(),
        )?;

        let n = source_diagram.top_dim();
        let mut current = source_diagram.clone();
        let mut running: Option<Diagram> = None;
        let mut history: Vec<HistoryEntry> = Vec::with_capacity(session.moves.len());

        for (step_idx, mov) in session.moves.iter().enumerate() {
            let candidates = compute_rewrites(&store, &type_complex, &current)
                .map_err(|e| format!("replay failed at step {}: {}", step_idx + 1, e))?;

            let match_result = candidates.get(mov.choice).ok_or_else(|| {
                format!(
                    "replay failed at step {}: choice {} out of range ({} candidate(s) available)",
                    step_idx + 1, mov.choice, candidates.len(),
                )
            })?;

            if match_result.rule_name != mov.rule_name {
                return Err(format!(
                    "replay sanity check failed at step {}: \
                     expected rule '{}' at choice {}, found '{}'",
                    step_idx + 1, mov.rule_name, mov.choice, match_result.rule_name,
                ));
            }

            let step = match_result.step.clone();

            // Save snapshot before advancing.
            history.push(HistoryEntry {
                mov: mov.clone(),
                prev_diagram: current.clone(),
                prev_running: running.clone(),
            });

            running = Some(match running {
                None => step,
                Some(r) => Diagram::paste(n, &r, &step)
                    .map_err(|e| format!("compose failed at step {}: {}", step_idx + 1, e))?,
            });

            current = Diagram::boundary(Sign::Target, n, running.as_ref().unwrap())
                .map_err(|e| format!("target boundary at step {}: {}", step_idx + 1, e))?;
        }

        let available_rewrites = compute_rewrites(&store, &type_complex, &current)?;

        Ok(Self {
            current_diagram: current,
            running_diagram: running,
            history,
            available_rewrites,
            source_file: session.source_file.clone(),
            type_name: session.type_name.clone(),
            source_diagram_name: session.source_diagram.clone(),
            target_diagram_name: session.target_diagram.clone(),
            store,
            type_complex,
            source_diagram,
            target_diagram,
        })
    }
}

// ── Mutating operations ───────────────────────────────────────────────────────

impl RewriteEngine {
    /// Apply the rewrite at index `choice` in the current available-rewrites list.
    ///
    /// Saves a snapshot for O(diagram) undo, advances the current diagram,
    /// and recomputes available rewrites.
    pub fn step(&mut self, choice: usize) -> Result<&str, String> {
        let match_result = self.available_rewrites.get(choice).ok_or_else(|| {
            format!(
                "choice {} out of range ({} rewrite(s) available)",
                choice, self.available_rewrites.len(),
            )
        })?;

        let n = self.current_diagram.top_dim();
        let rule_name = match_result.rule_name.clone();
        let step = match_result.step.clone();

        let prev_diagram = self.current_diagram.clone();
        let prev_running = self.running_diagram.clone();

        self.running_diagram = Some(match self.running_diagram.take() {
            None => step,
            Some(r) => Diagram::paste(n, &r, &step)
                .map_err(|e| format!("compose step failed: {}", e))?,
        });

        self.current_diagram = Diagram::boundary(
            Sign::Target, n, self.running_diagram.as_ref().unwrap(),
        ).map_err(|e| format!("target boundary failed: {}", e))?;

        self.history.push(HistoryEntry {
            mov: Move { choice, rule_name: rule_name.clone() },
            prev_diagram,
            prev_running,
        });

        self.available_rewrites = compute_rewrites(
            &self.store,
            &self.type_complex,
            &self.current_diagram,
        )?;

        Ok(&self.history.last().unwrap().mov.rule_name)
    }

    /// Undo the last applied step, restoring the previous diagram state.
    ///
    /// Returns an error if there are no moves to undo.
    pub fn undo(&mut self) -> Result<(), String> {
        let entry = self.history.pop().ok_or("nothing to undo")?;
        self.current_diagram = entry.prev_diagram;
        self.running_diagram = entry.prev_running;
        self.available_rewrites = compute_rewrites(
            &self.store,
            &self.type_complex,
            &self.current_diagram,
        )?;
        Ok(())
    }

    /// Undo all steps, resetting to the source diagram.
    pub fn undo_all(&mut self) -> Result<(), String> {
        self.undo_to(0)
    }

    /// Undo back to (but not past) the given step index (0 = fully undone,
    /// 1 = after step 1, etc.).
    pub fn undo_to(&mut self, target_step: usize) -> Result<(), String> {
        if target_step > self.history.len() {
            return Err(format!(
                "cannot undo to step {}: only {} step(s) applied",
                target_step, self.history.len(),
            ));
        }
        while self.history.len() > target_step {
            self.undo()?;
        }
        Ok(())
    }
}

// ── Read-only accessors ───────────────────────────────────────────────────────

impl RewriteEngine {
    /// Number of rewrite steps applied so far.
    pub fn step_count(&self) -> usize { self.history.len() }

    /// The n-diagram being actively rewritten (changes after each [`step`](Self::step)).
    pub fn current_diagram(&self) -> &Diagram { &self.current_diagram }

    /// The fixed source n-diagram that the session started from.
    pub fn source_diagram(&self) -> &Diagram { &self.source_diagram }

    /// The declared goal diagram, or `None` if no target was specified.
    pub fn target_diagram(&self) -> Option<&Diagram> { self.target_diagram.as_ref() }

    /// The accumulated (n+1)-dimensional proof cell, or `None` if no steps have
    /// been taken.  Each [`step`](Self::step) pastes a new rewrite onto this.
    pub fn running_diagram(&self) -> Option<&Diagram> { self.running_diagram.as_ref() }

    /// The candidate rewrites applicable to the current diagram, precomputed
    /// after each [`step`](Self::step) or [`undo`](Self::undo).
    pub fn available_rewrites(&self) -> &[MatchResult] { &self.available_rewrites }

    /// The interpreter's global store, shared read-only across the session.
    pub fn store(&self) -> &GlobalStore { &self.store }

    /// The type complex whose (n+1)-generators are the rewrite rules.
    pub fn type_complex(&self) -> &Complex { &self.type_complex }

    /// Iterate over the moves in history order without materialising a [`SessionFile`].
    pub fn history_moves(&self) -> impl Iterator<Item = &Move> {
        self.history.iter().map(|e| &e.mov)
    }

    /// Canonical path to the `.ali` source file for this session.
    pub fn source_file(&self) -> &str { &self.source_file }

    /// Name of the type whose generators serve as rewrite rules.
    pub fn type_name(&self) -> &str { &self.type_name }

    /// Name of the source n-diagram (as declared in the type or module).
    pub fn source_diagram_name(&self) -> &str { &self.source_diagram_name }

    /// Name of the target diagram, or `None` if no goal was declared.
    pub fn target_diagram_name(&self) -> Option<&str> { self.target_diagram_name.as_deref() }

    /// Returns true only if the current diagram equals the target AND at least
    /// one rewrite step has been applied.  Regular directed complexes have no
    /// identities, so source == target at the start is never a valid proof.
    pub fn target_reached(&self) -> bool {
        self.running_diagram.is_some()
            && self.target_diagram.as_ref()
                .map(|t| Diagram::equal(&self.current_diagram, t))
                .unwrap_or(false)
    }

    /// Render the running proof diagram as a `.ali` source expression, for the completion
    /// message and the `store`/`save` commands.
    ///
    /// Returns `None` if no steps have been taken yet.
    pub fn proof_label(&self) -> Option<String> {
        self.running_diagram.as_ref().map(|d| {
            crate::output::diagram_to_source(d, &self.type_complex)
        })
    }

    /// Typecheck the current proof diagram.
    ///
    /// Runs two checks:
    /// 1. Source boundary: the proof's source n-boundary is isomorphic to `source_diagram`.
    /// 2. Round-trip: sourcefies the proof, re-interprets it through the interpreter, and
    ///    confirms the result is isomorphic to the constructed proof.
    ///
    /// Returns `Ok(())` if both pass, `Err(message)` on any failure.
    /// Returns `Err` immediately if no proof steps have been taken.
    pub fn typecheck_proof(&self) -> Result<(), String> {
        let diagram = self.running_diagram.as_ref()
            .ok_or_else(|| "no proof steps taken yet".to_owned())?;

        // Check 1: source boundary.
        let n = self.source_diagram.top_dim();
        let src_boundary = Diagram::boundary(Sign::Source, n, diagram)
            .map_err(|e| format!("source boundary check failed: {}", e))?;
        if !Diagram::isomorphic(&src_boundary, &self.source_diagram) {
            return Err(format!(
                "proof source boundary does not match declared source '{}' — \
                 this is a bug in the rewrite engine",
                self.source_diagram_name,
            ));
        }

        // Check 2: round-trip through the interpreter.
        let source_expr = crate::output::diagram_to_source(diagram, &self.type_complex);
        let ast = crate::language::parse_diagram(&source_expr)
            .map_err(|e| format!("sourcefier produced unparseable expression '{}': {}", source_expr, e))?;
        let ctx = crate::interpreter::Context::new_with_resolutions(
            self.source_file.clone(),
            std::sync::Arc::new(crate::aux::loader::ModuleResolutions::empty()),
            Arc::clone(&self.store),
        );
        let (interp_opt, interp_result) =
            crate::interpreter::interpret_diagram(&ctx, &self.type_complex, &ast);
        if interp_result.has_errors() {
            let msgs: Vec<String> = interp_result.errors.iter()
                .map(|e| format!("{}", e))
                .collect();
            return Err(format!(
                "interpreter rejected proof expression '{}': {}",
                source_expr, msgs.join("; "),
            ));
        }
        let interp = interp_opt.ok_or_else(|| format!(
            "interpreter produced no diagram for expression '{}'", source_expr,
        ))?;
        if !Diagram::isomorphic(&interp, diagram) {
            return Err(format!(
                "round-trip check failed: expression '{}' does not reconstruct the proof — \
                 this is a bug in the sourcefier",
                source_expr,
            ));
        }

        Ok(())
    }

    /// Register the current running proof as a first-class generator.
    ///
    /// Computes the proof's source/target boundaries, delegates registration
    /// to [`GlobalStore::register_generator`], then resyncs the engine's own
    /// `type_complex` from the updated store.
    ///
    /// Returns `(updated_store, updated_type_complex)` so the caller can resync
    /// its own `Arc` references.
    pub fn register_proof(&mut self, name: &str) -> Result<(Arc<GlobalStore>, Arc<Complex>), String> {
        let diagram = self.running_diagram.clone()
            .ok_or_else(|| "no proof steps taken yet".to_owned())?;

        let type_gid = self.store
            .find_type_gid(&self.type_name)
            .ok_or_else(|| format!("type '{}' not found in store", self.type_name))?;

        let dim = self.source_diagram.top_dim() + 1;
        Arc::make_mut(&mut self.store)
            .register_proof_diagram(type_gid, name.to_owned(), diagram, dim)?;

        self.type_complex = self.store
            .find_type(type_gid)
            .map(|e| Arc::clone(&e.complex))
            .ok_or_else(|| format!("type '{}' missing after registration", self.type_name))?;

        self.available_rewrites = compute_rewrites(
            &self.store, &self.type_complex, &self.current_diagram,
        )?;

        Ok((Arc::clone(&self.store), Arc::clone(&self.type_complex)))
    }

    /// Export the current session state as a [`SessionFile`] for disk persistence.
    pub fn to_session_file(&self) -> SessionFile {
        SessionFile {
            source_file: self.source_file.clone(),
            type_name: self.type_name.clone(),
            source_diagram: self.source_diagram_name.clone(),
            target_diagram: self.target_diagram_name.clone(),
            moves: self.history.iter().map(|e| e.mov.clone()).collect(),
        }
    }
}
