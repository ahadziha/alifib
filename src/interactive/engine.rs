//! Stateful rewrite engine: holds session state in memory for O(1) undo
//! and incremental step/apply without re-interpreting the source file.

use crate::aux::loader::Loader;
use crate::aux::Tag;
use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, Sign};
use crate::core::rewrite::{CandidateRewrite, apply_rewrite, find_candidate_rewrites};
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
    available_rewrites: Vec<CandidateRewrite>,

    // Metadata (for session file persistence)
    source_file: String,
    type_name: String,
    source_diagram_name: String,
    target_diagram_name: Option<String>,
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Load a file and locate the type complex and source/target diagrams.
fn load_context(
    source_file: &str,
    type_name: &str,
    source_diagram_name: &str,
    target_diagram_name: Option<&str>,
) -> Result<(Arc<GlobalStore>, Arc<Complex>, Diagram, Option<Diagram>), String> {
    let loader = Loader::default(vec![]);
    let file = InterpretedFile::load(&loader, source_file)
        .into_result()
        .map_err(|_| format!("failed to interpret '{}'", source_file))?;

    let store = Arc::clone(&file.state);

    let module_complex = store
        .find_module(&file.path)
        .ok_or_else(|| format!("module '{}' not found in store", file.path))?;

    let (type_tag, _) = module_complex
        .find_generator(type_name)
        .ok_or_else(|| format!("type '{}' not found in module", type_name))?;

    let type_gid = match type_tag {
        Tag::Global(gid) => *gid,
        Tag::Local(_) => {
            return Err(format!("'{}' is a local cell, not a type", type_name));
        }
    };

    let type_entry = store
        .find_type(type_gid)
        .ok_or_else(|| format!("type entry for '{}' not found", type_name))?;
    let type_complex = Arc::clone(&type_entry.complex);

    let find_diagram = |name: &str| -> Option<Diagram> {
        type_complex.find_diagram(name).cloned()
            .or_else(|| module_complex.find_diagram(name).cloned())
    };

    let source_diagram = find_diagram(source_diagram_name)
        .ok_or_else(|| format!(
            "diagram '{}' not found in type '{}' or module",
            source_diagram_name, type_name,
        ))?;

    let target_diagram = target_diagram_name
        .map(|name| {
            find_diagram(name).ok_or_else(|| format!(
                "target diagram '{}' not found in type '{}' or module",
                name, type_name,
            ))
        })
        .transpose()?;

    Ok((store, type_complex, source_diagram, target_diagram))
}

fn compute_rewrites(
    store: &GlobalStore,
    type_complex: &Complex,
    current: &Diagram,
) -> Vec<CandidateRewrite> {
    find_candidate_rewrites(
        |cx, tag| store.cell_data_for_tag(cx, tag),
        type_complex,
        current,
    )
}

// ── Constructor impls ─────────────────────────────────────────────────────────

impl RewriteEngine {
    /// Create a fresh session from an already-loaded store and type complex.
    ///
    /// Diagrams are looked up by name in `type_complex` (falling back to the
    /// module complex for `source_file` in `store`).  This constructor is used
    /// by the workspace session REPL to avoid re-loading from disk.
    pub fn from_store(
        store: Arc<GlobalStore>,
        type_complex: Arc<Complex>,
        source_diagram_name: &str,
        target_diagram_name: Option<&str>,
        source_file: String,
        type_name: String,
    ) -> Result<Self, String> {
        let find_diagram_full = |name: &str| -> Option<Diagram> {
            type_complex.find_diagram(name).cloned().or_else(|| {
                store.find_module(&source_file)
                    .and_then(|m| m.find_diagram(name))
                    .cloned()
            })
        };

        let source_diagram = find_diagram_full(source_diagram_name)
            .ok_or_else(|| format!(
                "diagram '{}' not found in type '{}' or module",
                source_diagram_name, type_name,
            ))?;

        let target_diagram = target_diagram_name
            .map(|name| {
                find_diagram_full(name).ok_or_else(|| format!(
                    "target diagram '{}' not found in type '{}' or module",
                    name, type_name,
                ))
            })
            .transpose()?;

        let available_rewrites = compute_rewrites(&store, &type_complex, &source_diagram);

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

        let available_rewrites = compute_rewrites(&store, &type_complex, &source_diagram);

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
            let candidates = compute_rewrites(&store, &type_complex, &current);

            let candidate = candidates.get(mov.choice).ok_or_else(|| {
                format!(
                    "replay failed at step {}: choice {} out of range ({} candidate(s) available)",
                    step_idx + 1, mov.choice, candidates.len(),
                )
            })?;

            if candidate.rule_name != mov.rule_name {
                return Err(format!(
                    "replay sanity check failed at step {}: \
                     expected rule '{}' at choice {}, found '{}'",
                    step_idx + 1, mov.rule_name, mov.choice, candidate.rule_name,
                ));
            }

            let step = apply_rewrite(&current, candidate)
                .map_err(|e| format!("replay failed at step {}: {}", step_idx + 1, e))?;

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

        let available_rewrites = compute_rewrites(&store, &type_complex, &current);

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
        let candidate = self.available_rewrites.get(choice).ok_or_else(|| {
            format!(
                "choice {} out of range ({} rewrite(s) available)",
                choice, self.available_rewrites.len(),
            )
        })?;

        let n = self.current_diagram.top_dim();
        let rule_name = candidate.rule_name.clone();

        let step = apply_rewrite(&self.current_diagram, candidate)
            .map_err(|e| format!("apply rewrite failed: {}", e))?;

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

        self.available_rewrites =
            compute_rewrites(&self.store, &self.type_complex, &self.current_diagram);

        Ok(&self.history.last().unwrap().mov.rule_name)
    }

    /// Undo the last applied step, restoring the previous diagram state.
    ///
    /// Returns an error if there are no moves to undo.
    pub fn undo(&mut self) -> Result<(), String> {
        let entry = self.history.pop().ok_or("nothing to undo")?;
        self.current_diagram = entry.prev_diagram;
        self.running_diagram = entry.prev_running;
        self.available_rewrites =
            compute_rewrites(&self.store, &self.type_complex, &self.current_diagram);
        Ok(())
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
    pub fn step_count(&self) -> usize { self.history.len() }
    pub fn current_diagram(&self) -> &Diagram { &self.current_diagram }
    pub fn source_diagram(&self) -> &Diagram { &self.source_diagram }
    pub fn target_diagram(&self) -> Option<&Diagram> { self.target_diagram.as_ref() }
    pub fn running_diagram(&self) -> Option<&Diagram> { self.running_diagram.as_ref() }
    pub fn available_rewrites(&self) -> &[CandidateRewrite] { &self.available_rewrites }
    pub fn store(&self) -> &GlobalStore { &self.store }
    pub fn type_complex(&self) -> &Complex { &self.type_complex }

    /// Iterate over the moves in history order without materialising a [`SessionFile`].
    pub fn history_moves(&self) -> impl Iterator<Item = &Move> {
        self.history.iter().map(|e| &e.mov)
    }
    pub fn source_file(&self) -> &str { &self.source_file }
    pub fn type_name(&self) -> &str { &self.type_name }
    pub fn source_diagram_name(&self) -> &str { &self.source_diagram_name }
    pub fn target_diagram_name(&self) -> Option<&str> { self.target_diagram_name.as_deref() }

    pub fn target_reached(&self) -> bool {
        self.target_diagram.as_ref()
            .map(|t| Diagram::equal(&self.current_diagram, t))
            .unwrap_or(false)
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

