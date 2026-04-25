//! Stateful rewrite engine: holds session state in memory for O(1) undo
//! and incremental step/apply without re-interpreting the source file.

use crate::aux::loader::Loader;
use crate::aux::Tag;
use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, Sign};
use crate::core::matching::{
    MatchResult, ParallelMatchResult, RulePattern,
    find_matches, find_matches_impl, find_compatible_families,
};
use crate::interpreter::{GlobalStore, InterpretedFile};
use super::session::{Move, SessionFile};
use std::collections::HashMap;
use std::sync::Arc;

/// A snapshot of a single past step, stored for O(1) undo.
struct HistoryEntry {
    /// The serialisable move record (choice + rule name).
    mov: Move,
    /// The current n-diagram *before* this step was applied.
    prev_diagram: Diagram,
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
    /// The individual (n+1)-dimensional rewrite steps applied so far.
    /// The full proof diagram is only built on demand — see [`assemble_proof`].
    steps: Vec<Diagram>,
    history: Vec<HistoryEntry>,
    available_rewrites: Vec<MatchResult>,

    // Parallel rewrite mode
    parallel: bool,
    parallel_rewrites: Vec<ParallelMatchResult>,

    /// Per-rule precomputed pattern data (normalised input boundary + embedding
    /// into the rule's shape).  Built once when the engine is constructed and
    /// reused for every [`find_matches`] call so that rule-side boundary
    /// traversal and normalisation aren't redone on every step.
    rule_patterns: HashMap<String, RulePattern>,

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
/// Used by the cli and the daemon which always know the type upfront.
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

/// Build precomputed [`RulePattern`]s for every rewrite rule at dimension
/// `n + 1` in `type_complex`, indexed by rule name.
fn build_rule_patterns(
    type_complex: &Complex,
    n: usize,
) -> Result<HashMap<String, RulePattern>, String> {
    let mut out = HashMap::new();
    for (name, _tag, dim) in type_complex.generators_iter() {
        if dim != n + 1 { continue; }
        let Some(rewrite) = type_complex.classifier(name) else { continue; };
        let rp = RulePattern::new(rewrite).map_err(|e| {
            format!("failed to precompute pattern for rule '{}': {}", name, e)
        })?;
        out.insert(name.to_owned(), rp);
    }
    Ok(out)
}

fn compute_rewrites(
    type_complex: &Complex,
    rule_patterns: &HashMap<String, RulePattern>,
    current: &Diagram,
) -> Result<Vec<MatchResult>, String> {
    let n = current.top_dim();
    let mut all_matches = Vec::new();

    for (name, _tag, dim) in type_complex.generators_iter() {
        if dim != n + 1 { continue; }
        let Some(rewrite) = type_complex.classifier(name) else { continue; };
        let Some(rp) = rule_patterns.get(name) else { continue; };

        match find_matches(type_complex, rewrite, rp, current, name) {
            Ok(matches) => all_matches.extend(matches),
            Err(e) => return Err(format!("failed to match rule '{}': {}", name, e)),
        }
    }

    Ok(all_matches)
}

fn compute_first_rewrite(
    type_complex: &Complex,
    rule_patterns: &HashMap<String, RulePattern>,
    current: &Diagram,
) -> Result<Option<MatchResult>, String> {
    let n = current.top_dim();

    for (name, _tag, dim) in type_complex.generators_iter() {
        if dim != n + 1 { continue; }
        let Some(rewrite) = type_complex.classifier(name) else { continue; };
        let Some(rp) = rule_patterns.get(name) else { continue; };

        match find_matches_impl(type_complex, rewrite, rp, current, name, Some(1)) {
            Ok(matches) => if let Some(m) = matches.into_iter().next() { return Ok(Some(m)); },
            Err(e) => return Err(format!("failed to match rule '{}': {}", name, e)),
        }
    }

    Ok(None)
}

// ── Constructor impls ─────────────────────────────────────────────────────────

impl RewriteEngine {
    /// Create a fresh session from an already-loaded store and type complex.
    ///
    /// `source_diagram_name` and `target_diagram_name` may be either plain diagram
    /// names (resolved against `type_complex` and the module at `source_file`) or
    /// full diagram expressions in the alifib language (e.g. `"f g"` or `"(f #0 g)"`).
    /// This constructor is used by the interactive REPL and the web backends.
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

        let rule_patterns = build_rule_patterns(&type_complex, source_diagram.top_dim())?;
        let available_rewrites =
            compute_rewrites(&type_complex, &rule_patterns, &source_diagram)?;

        Ok(Self {
            current_diagram: source_diagram.clone(),
            steps: Vec::new(),
            history: Vec::new(),
            available_rewrites,
            parallel: false,
            parallel_rewrites: Vec::new(),
            rule_patterns,
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

        let rule_patterns = build_rule_patterns(&type_complex, source_diagram.top_dim())?;
        let available_rewrites =
            compute_rewrites(&type_complex, &rule_patterns, &source_diagram)?;

        Ok(Self {
            current_diagram: source_diagram.clone(),
            steps: Vec::new(),
            history: Vec::new(),
            available_rewrites,
            parallel: false,
            parallel_rewrites: Vec::new(),
            rule_patterns,
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
        let rule_patterns = build_rule_patterns(&type_complex, n)?;
        let mut current = source_diagram.clone();
        let mut steps: Vec<Diagram> = Vec::with_capacity(session.moves.len());
        let mut history: Vec<HistoryEntry> = Vec::with_capacity(session.moves.len());

        for (step_idx, mov) in session.moves.iter().enumerate() {
            let candidates = compute_rewrites(&type_complex, &rule_patterns, &current)
                .map_err(|e| format!("replay failed at step {}: {}", step_idx + 1, e))?;

            let step = if mov.parallel {
                // Replay a parallel move: compute families and select by choice index.
                let families = find_compatible_families(
                    &candidates, &type_complex, &current, &rule_patterns, false,
                );
                let total = families.len() + candidates.len();
                if mov.choice >= total {
                    return Err(format!(
                        "replay failed at step {}: choice {} out of range ({} candidate(s) available)",
                        step_idx + 1, mov.choice, total,
                    ));
                }
                if mov.choice < families.len() {
                    families[mov.choice].step.clone()
                } else {
                    let idx = mov.choice - families.len();
                    candidates[idx].step.clone()
                }
            } else {
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
                match_result.step.clone()
            };

            // Save snapshot before advancing.
            history.push(HistoryEntry {
                mov: mov.clone(),
                prev_diagram: current.clone(),
            });

            current = Diagram::boundary(Sign::Target, n, &step)
                .map_err(|e| format!("target boundary at step {}: {}", step_idx + 1, e))?;
            steps.push(step);
        }

        let available_rewrites =
            compute_rewrites(&type_complex, &rule_patterns, &current)?;

        Ok(Self {
            current_diagram: current,
            steps,
            history,
            available_rewrites,
            parallel: false,
            parallel_rewrites: Vec::new(),
            rule_patterns,
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
    /// Recompute individual matches and, when parallel mode is on, compatible families.
    fn refresh_rewrites(&mut self) -> Result<(), String> {
        self.available_rewrites = compute_rewrites(
            &self.type_complex,
            &self.rule_patterns,
            &self.current_diagram,
        )?;
        self.parallel_rewrites = if self.parallel {
            find_compatible_families(
                &self.available_rewrites,
                &self.type_complex,
                &self.current_diagram,
                &self.rule_patterns,
                false,
            )
        } else {
            Vec::new()
        };
        Ok(())
    }

    /// Total number of choices: parallel families (if any) followed by individual matches.
    fn total_choices(&self) -> usize {
        self.parallel_rewrites.len() + self.available_rewrites.len()
    }

    /// Apply the rewrite at index `choice` in the combined rewrites list.
    ///
    /// In parallel mode the list is: parallel families first, then individual
    /// matches.  Records the step and advances the current diagram.
    pub fn step(&mut self, choice: usize) -> Result<&str, String> {
        let total = self.total_choices();
        if choice >= total {
            return Err(format!(
                "choice {} out of range ({} rewrite(s) available)",
                choice, total,
            ));
        }

        let n = self.current_diagram.top_dim();
        let (step, rule_name, is_parallel) = if choice < self.parallel_rewrites.len() {
            let pr = &self.parallel_rewrites[choice];
            let names: Vec<&str> = pr.family.iter()
                .map(|&i| self.available_rewrites[i].rule_name.as_str())
                .collect();
            (pr.step.clone(), names.join(","), true)
        } else {
            let idx = choice - self.parallel_rewrites.len();
            let m = &self.available_rewrites[idx];
            (m.step.clone(), m.rule_name.clone(), false)
        };

        let prev_diagram = self.current_diagram.clone();

        let new_current = Diagram::boundary(Sign::Target, n, &step)
            .map_err(|e| format!("target boundary failed: {}", e))?;

        self.current_diagram = new_current;
        self.steps.push(step);

        self.history.push(HistoryEntry {
            mov: Move { choice, rule_name: rule_name.clone(), parallel: is_parallel },
            prev_diagram,
        });

        self.refresh_rewrites()?;

        Ok(&self.history.last().unwrap().mov.rule_name)
    }

    /// Undo the last applied step, restoring the previous diagram state.
    ///
    /// Returns an error if there are no moves to undo.
    pub fn undo(&mut self) -> Result<(), String> {
        let entry = self.history.pop().ok_or("nothing to undo")?;
        self.current_diagram = entry.prev_diagram;
        self.steps.pop();
        self.refresh_rewrites()?;
        Ok(())
    }

    /// Undo all steps, resetting to the source diagram.
    pub fn undo_all(&mut self) -> Result<(), String> {
        self.undo_to(0)
    }

    /// Apply up to `max_steps` rewrites automatically, always picking the
    /// first available candidate.
    ///
    /// In parallel mode, prefers the first compatible family (largest, then
    /// lexicographically first) at each step; falls back to a single match
    /// if no family of size ≥ 2 exists.
    pub fn auto(&mut self, max_steps: usize) -> Result<(usize, Option<&'static str>), String> {
        let mut applied = 0usize;
        let stop_reason: Option<&'static str>;

        loop {
            if self.target_reached() {
                stop_reason = Some("target reached");
                break;
            }
            if applied >= max_steps {
                stop_reason = None;
                break;
            }

            let (step, rule_name, is_parallel) = if self.parallel {
                // Try parallel family first.
                let all_matches = compute_rewrites(
                    &self.type_complex,
                    &self.rule_patterns,
                    &self.current_diagram,
                )?;
                let families = find_compatible_families(
                    &all_matches,
                    &self.type_complex,
                    &self.current_diagram,
                    &self.rule_patterns,
                    true,
                );
                if let Some(pr) = families.into_iter().next() {
                    let names: Vec<&str> = pr.family.iter()
                        .map(|&i| all_matches[i].rule_name.as_str())
                        .collect();
                    (pr.step, names.join(","), true)
                } else if let Some(m) = all_matches.into_iter().next() {
                    (m.step, m.rule_name, false)
                } else {
                    stop_reason = Some("no rewrites available");
                    break;
                }
            } else {
                let first = compute_first_rewrite(
                    &self.type_complex,
                    &self.rule_patterns,
                    &self.current_diagram,
                )?;
                let Some(m) = first else {
                    stop_reason = Some("no rewrites available");
                    break;
                };
                (m.step, m.rule_name, false)
            };

            let n = self.current_diagram.top_dim();
            let prev_diagram = self.current_diagram.clone();

            let new_current = Diagram::boundary(Sign::Target, n, &step)
                .map_err(|e| format!("target boundary failed: {}", e))?;

            self.current_diagram = new_current;
            self.steps.push(step);
            self.history.push(HistoryEntry {
                mov: Move { choice: 0, rule_name, parallel: is_parallel },
                prev_diagram,
            });
            applied += 1;
        }

        self.refresh_rewrites()?;

        Ok((applied, stop_reason))
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
        if target_step == self.history.len() { return Ok(()); }
        self.current_diagram = if target_step == 0 {
            self.source_diagram.clone()
        } else {
            self.history[target_step].prev_diagram.clone()
        };
        self.history.truncate(target_step);
        self.steps.truncate(target_step);
        self.refresh_rewrites()?;
        Ok(())
    }

    /// Toggle parallel rewrite mode on or off. Recomputes available rewrites.
    pub fn set_parallel(&mut self, on: bool) -> Result<(), String> {
        self.parallel = on;
        self.refresh_rewrites()
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

    /// The individual (n+1)-dimensional rewrite steps applied so far, in order.
    ///
    /// Each step is a single rewrite — they are *not* pasted together here.
    /// Use [`assemble_proof`](Self::assemble_proof) to build the full proof
    /// diagram (only done when storing or typechecking).
    pub fn steps(&self) -> &[Diagram] { &self.steps }

    /// Paste the recorded rewrite steps together into the full (n+1)-dimensional
    /// proof diagram.  Returns `Ok(None)` if no steps have been taken.
    ///
    /// This is the one place where the "diagram up to now" is actually built —
    /// call it only when the assembled proof is needed (storing, typechecking,
    /// or rendering the final proof label).
    pub fn assemble_proof(&self) -> Result<Option<Diagram>, String> {
        let n = self.source_diagram.top_dim();
        let mut iter = self.steps.iter();
        let Some(first) = iter.next() else { return Ok(None); };
        let mut acc = first.clone();
        for step in iter {
            acc = Diagram::paste(n, &acc, step)
                .map_err(|e| format!("compose step failed: {}", e))?;
        }
        Ok(Some(acc))
    }

    /// The candidate rewrites applicable to the current diagram, precomputed
    /// after each [`step`](Self::step) or [`undo`](Self::undo).
    pub fn available_rewrites(&self) -> &[MatchResult] { &self.available_rewrites }

    /// The compatible parallel families, computed when parallel mode is on.
    /// Empty when parallel mode is off.
    pub fn parallel_rewrites(&self) -> &[ParallelMatchResult] { &self.parallel_rewrites }

    /// Whether parallel rewrite mode is currently enabled.
    pub fn parallel(&self) -> bool { self.parallel }

    /// The interpreter's global store, shared read-only across the session.
    pub fn store(&self) -> &GlobalStore { &self.store }

    /// Clone the `Arc` to the interpreter's global store.  Used by callers
    /// that need to keep an independent handle (e.g. the web adapter, which
    /// resyncs its cached store after a successful `register_proof`).
    pub fn store_arc(&self) -> Arc<GlobalStore> { Arc::clone(&self.store) }

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

    /// Returns true only if the current diagram is isomorphic to the target AND
    /// at least one rewrite step has been applied.  Regular directed complexes
    /// have no identities, so source == target at the start is never a valid proof.
    pub fn target_reached(&self) -> bool {
        !self.steps.is_empty()
            && self.target_diagram.as_ref()
                .map(|t| Diagram::isomorphic(&self.current_diagram, t))
                .unwrap_or(false)
    }

    /// Render the assembled proof diagram as a `.ali` source expression, for
    /// the completion message and the `store`/`save` commands.
    ///
    /// Builds the full proof by pasting all recorded steps together — call
    /// only when the rendered label is actually needed.  Returns
    /// `Ok(None)` if no steps have been taken yet.
    pub fn proof_label(&self) -> Result<Option<String>, String> {
        Ok(self.assemble_proof()?
            .map(|d| crate::output::render_diagram(&d, &self.type_complex)))
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
        let assembled = self.assemble_proof()?
            .ok_or_else(|| "no proof steps taken yet".to_owned())?;
        let diagram = &assembled;

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
        let source_expr = crate::output::render_diagram(diagram, &self.type_complex);
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

    /// Register the current running proof as a named diagram (let-binding)
    /// in the type complex.
    ///
    /// Returns `(updated_store, updated_type_complex)` so the caller can resync
    /// its own `Arc` references.
    pub fn register_proof(&mut self, name: &str) -> Result<(Arc<GlobalStore>, Arc<Complex>), String> {
        let diagram = self.assemble_proof()?
            .ok_or_else(|| "no proof steps taken yet".to_owned())?;

        if self.type_complex.name_in_use(name) || self.type_complex.find_generator(name).is_some() {
            return Err(format!("name '{}' is already in use", name));
        }

        let type_gid = self.store
            .find_type_gid(&self.type_name)
            .ok_or_else(|| format!("type '{}' not found in store", self.type_name))?;

        Arc::make_mut(&mut self.store)
            .modify_type_complex(type_gid, |cx| {
                cx.add_diagram(name.to_owned(), diagram);
            })
            .ok_or_else(|| format!("type '{}' not found in store", self.type_name))?;

        self.type_complex = self.store
            .find_type(type_gid)
            .map(|e| Arc::clone(&e.complex))
            .ok_or_else(|| format!("type '{}' missing after registration", self.type_name))?;

        // No need to recompute rewrites — a let-binding doesn't add new rewrite rules.

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

    /// Dispatch an engine-level [`Request`] to the matching method and return
    /// the response data.
    ///
    /// Returns `None` for variants that don't belong to this layer — session
    /// transitions (`Init`, `Resume`, `Save`, `Shutdown`) and store-level
    /// queries (`Homology`).  Callers handle those themselves.
    ///
    /// Errors from the engine (bad choice index, nothing to undo, name
    /// already in use, …) are returned as `Some(Err(msg))`.
    pub fn handle(
        &mut self,
        req: &super::protocol::Request,
    ) -> Option<Result<super::protocol::ResponseData, String>> {
        use super::protocol::*;
        // `self.step(...)` holds `&mut self`; `build_response(self, …)` also
        // borrows.  We split into statements so the mutable borrow ends
        // before the response builder starts.
        let result: Result<ResponseData, String> = match req {
            Request::Step { choice } => match self.step(*choice) {
                Ok(_) => Ok(build_response(self, false)),
                Err(e) => Err(e),
            },
            Request::Auto { max_steps } => match self.auto(*max_steps) {
                Ok((applied, stop_reason)) => {
                    let mut data = build_response(self, false);
                    data.auto = Some(AutoInfo {
                        applied,
                        stop_reason: stop_reason.unwrap_or("").to_owned(),
                    });
                    Ok(data)
                }
                Err(msg) => Err(msg),
            },
            Request::Undo => self.undo().map(|_| build_response(self, false)),
            Request::UndoTo { step } => {
                self.undo_to(*step).map(|_| build_response(self, false))
            }
            Request::Show => Ok(build_response(self, false)),
            Request::History => Ok(build_response(self, true)),
            Request::ListRules => Ok(build_list_rules_response(self)),
            Request::Types => Ok(build_types_response(self)),
            Request::TypeInfo { name } => build_type_info_response(self, name),
            Request::Cell { name } => build_cell_response(self, name),
            Request::Store { name } => {
                // Render the proof expression from the current steps *before*
                // registering — registration rewrites `type_complex`, and the
                // rendered form should reflect the shape at the time of store.
                let stored_info = if self.steps().is_empty() {
                    None
                } else {
                    let n = self.source_diagram().top_dim();
                    let scope = self.type_complex();
                    let mut steps = self.steps().iter();
                    let first = crate::output::render_diagram(steps.next().unwrap(), scope);
                    let rest: String = steps
                        .map(|s| {
                            format!("\n#{} {}", n, crate::output::render_diagram(s, scope))
                        })
                        .collect();
                    Some(StoredInfo {
                        type_name: self.type_name().to_owned(),
                        def_name: name.clone(),
                        expr: format!("{}{}", first, rest),
                    })
                };
                match self.register_proof(name) {
                    Ok(_) => {
                        let mut data = build_response(self, false);
                        data.stored = stored_info;
                        Ok(data)
                    }
                    Err(msg) => Err(msg),
                }
            }
            Request::Parallel { on } => {
                self.set_parallel(*on).map(|_| build_response(self, false))
            }
            Request::Init { .. }
            | Request::Resume { .. }
            | Request::Save { .. }
            | Request::Shutdown
            | Request::Homology { .. } => return None,
        };
        Some(result)
    }
}
