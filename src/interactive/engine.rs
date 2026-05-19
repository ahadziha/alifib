//! Stateful rewrite engine: holds session state in memory for incremental
//! step/apply without re-interpreting the source file.

use crate::aux::loader::Loader;
use crate::aux::Tag;
use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, Sign};
use crate::core::matching::{
    FamilyMember, MatchResult, RulePattern,
    build_rule_patterns,
    confirm_candidate,
    for_each_rule_candidate,
    greedy_parallel_auto_step,
    try_family_from_members,
};
use crate::interpreter::{GlobalStore, InterpretedFile};
use super::session::{Move, SessionFile};
use rand::RngExt;
use rand::rngs::SmallRng;
use std::collections::HashMap;
use std::sync::Arc;

struct HistoryEntry {
    mov: Move,
}

/// Cached assembled proof diagram at a known step count.
///
/// When proof view is active the frontend needs the (n+1)-dimensional proof
/// diagram at every step.  Rather than rebuilding from scratch each time,
/// we cache the last assembled proof and extend it incrementally.
pub struct ProofCache {
    pub snapshot: Diagram,
    pub at_step: usize,
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
    initial_diagram: Diagram,
    target_diagram: Option<Diagram>,
    backward: bool,

    // Mutable session state
    current_diagram: Diagram,
    /// The individual (n+1)-dimensional rewrite steps applied so far.
    /// The full proof diagram is only built on demand — see [`assemble_proof`].
    /// Entries beyond `active_len` form the redo buffer.
    steps: Vec<Diagram>,
    history: Vec<HistoryEntry>,
    /// How many entries in `steps`/`history` are active. The rest are the
    /// redo buffer, discarded only when a genuinely new step is applied.
    active_len: usize,
    rewrites: Vec<MatchResult>,
    parallel: bool,
    rng: SmallRng,

    /// Per-rule precomputed pattern data (normalised input or output boundary
    /// + embedding into the rule's shape, depending on [`backward`]).
    /// Built once when the engine is constructed and reused for every
    /// [`find_matches`] call.
    rule_patterns: HashMap<String, RulePattern>,

    /// Incremental proof cache, active only while proof view is enabled.
    proof_cache: Option<ProofCache>,

    // Metadata (for session file persistence)
    source_file: String,
    type_name: String,
    initial_diagram_name: String,
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
        Arc::new(String::new()),
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
    initial_diagram_name: &str,
    target_diagram_name: Option<&str>,
) -> Result<LoadedRewriteContext, String> {
    let (store, type_complex, canonical_path) = load_type_context(source_file, type_name)?;

    let initial_diagram =
        eval_diagram_expr(&store, &type_complex, &canonical_path, initial_diagram_name)?;
    let target_diagram = target_diagram_name
        .map(|expr| eval_diagram_expr(&store, &type_complex, &canonical_path, expr))
        .transpose()?;

    Ok((store, type_complex, initial_diagram, target_diagram))
}

/// Check that `source` and `target` are parallel: same dimension, and (for
/// dim > 0) isomorphic input and output boundaries.
fn check_parallel(source: &Diagram, target: &Diagram) -> Result<(), String> {
    if source.dim() != target.dim() {
        return Err(format!(
            "source has dimension {} but target has dimension {}",
            source.top_dim(), target.top_dim(),
        ));
    }
    if source.dim() <= 0 {
        return Ok(());
    }
    let k = source.top_dim() - 1;
    let src_in = Diagram::boundary_normal(Sign::Source, k, source)
        .map_err(|e| format!("source input boundary: {}", e))?;
    let tgt_in = Diagram::boundary_normal(Sign::Source, k, target)
        .map_err(|e| format!("target input boundary: {}", e))?;
    if !Diagram::isomorphic(&src_in, &tgt_in) {
        return Err("source and target are not parallel: input boundaries do not match".to_owned());
    }
    let src_out = Diagram::boundary_normal(Sign::Target, k, source)
        .map_err(|e| format!("source output boundary: {}", e))?;
    let tgt_out = Diagram::boundary_normal(Sign::Target, k, target)
        .map_err(|e| format!("target output boundary: {}", e))?;
    if !Diagram::isomorphic(&src_out, &tgt_out) {
        return Err("source and target are not parallel: output boundaries do not match".to_owned());
    }
    Ok(())
}

/// Confirm each candidate individually and collect all successes (non-parallel).
fn collect_confirmed_matches(
    type_complex: &Complex,
    rule_patterns: &HashMap<String, RulePattern>,
    current: &Diagram,
) -> Result<Vec<MatchResult>, String> {
    let mut results = Vec::new();
    for_each_rule_candidate(type_complex, rule_patterns, current, |cand| {
        if let Some(mr) = confirm_candidate(&cand, type_complex, current, rule_patterns) {
            results.push(mr);
        }
        false
    })?;
    Ok(results)
}

/// Find the first confirmed match lazily (non-parallel auto).
fn find_first_match(
    type_complex: &Complex,
    rule_patterns: &HashMap<String, RulePattern>,
    current: &Diagram,
) -> Result<Option<MatchResult>, String> {
    let mut result = None;
    for_each_rule_candidate(type_complex, rule_patterns, current, |cand| {
        if let Some(mr) = confirm_candidate(&cand, type_complex, current, rule_patterns) {
            result = Some(mr);
            true
        } else {
            false
        }
    })?;
    Ok(result)
}

// ── Constructor impls ─────────────────────────────────────────────────────────

impl RewriteEngine {
    /// Create a fresh session from an already-loaded store and type complex.
    ///
    /// `initial_diagram_name` and `target_diagram_name` may be either plain diagram
    /// names (resolved against `type_complex` and the module at `source_file`) or
    /// full diagram expressions in the alifib language (e.g. `"f g"` or `"(f #0 g)"`).
    /// This constructor is used by the interactive REPL and the web backends.
    pub fn from_store(
        store: Arc<GlobalStore>,
        type_complex: Arc<Complex>,
        initial_diagram_name: &str,
        target_diagram_name: Option<&str>,
        source_file: String,
        type_name: String,
        backward: bool,
    ) -> Result<Self, String> {
        let initial_diagram =
            eval_diagram_expr(&store, &type_complex, &source_file, initial_diagram_name)?;
        let target_diagram = target_diagram_name
            .map(|expr| eval_diagram_expr(&store, &type_complex, &source_file, expr))
            .transpose()?;
        if let Some(ref target) = target_diagram {
            check_parallel(&initial_diagram, target)?;
        }

        let rule_patterns = build_rule_patterns(&type_complex, initial_diagram.top_dim(), backward)?;
        let rewrites = collect_confirmed_matches(
            &type_complex, &rule_patterns, &initial_diagram,
        )?;

        Ok(Self {
            current_diagram: initial_diagram.clone(),
            steps: Vec::new(),
            history: Vec::new(),
            active_len: 0,
            rewrites,
            parallel: true,
            backward,
            rng: rand::make_rng(),
            rule_patterns,
            proof_cache: None,
            source_file,
            type_name,
            initial_diagram_name: initial_diagram_name.to_owned(),
            target_diagram_name: target_diagram_name.map(str::to_owned),
            store,
            type_complex,
            initial_diagram,
            target_diagram,
        })
    }

    /// Load the source file and initialise a fresh session (no moves applied).
    pub fn init(
        source_file: &str,
        type_name: &str,
        initial_diagram_name: &str,
        target_diagram_name: Option<&str>,
    ) -> Result<Self, String> {
        let (store, type_complex, initial_diagram, target_diagram) =
            load_context(source_file, type_name, initial_diagram_name, target_diagram_name)?;
        if let Some(ref target) = target_diagram {
            check_parallel(&initial_diagram, target)?;
        }

        let rule_patterns = build_rule_patterns(&type_complex, initial_diagram.top_dim(), false)?;
        let rewrites = collect_confirmed_matches(
            &type_complex, &rule_patterns, &initial_diagram,
        )?;

        Ok(Self {
            current_diagram: initial_diagram.clone(),
            steps: Vec::new(),
            history: Vec::new(),
            active_len: 0,
            rewrites,
            parallel: true,
            backward: false,
            rng: rand::make_rng(),
            rule_patterns,
            proof_cache: None,
            store,
            type_complex,
            initial_diagram,
            target_diagram,
            source_file: source_file.to_owned(),
            type_name: type_name.to_owned(),
            initial_diagram_name: initial_diagram_name.to_owned(),
            target_diagram_name: target_diagram_name.map(str::to_owned),
        })
    }

    /// Load the source file and replay a saved session.
    pub fn from_session(session: SessionFile) -> Result<Self, String> {
        let backward = session.backward;
        let (store, type_complex, initial_diagram, target_diagram) = load_context(
            &session.source_file,
            &session.type_name,
            &session.initial_diagram,
            session.target_diagram.as_deref(),
        )?;
        if let Some(ref target) = target_diagram {
            check_parallel(&initial_diagram, target)?;
        }

        let n = initial_diagram.top_dim();
        let rule_patterns = build_rule_patterns(&type_complex, n, backward)?;
        let step_sign = if backward { Sign::Source } else { Sign::Target };
        let mut current = initial_diagram.clone();
        let mut steps: Vec<Diagram> = Vec::with_capacity(session.moves.len());
        let mut history: Vec<HistoryEntry> = Vec::with_capacity(session.moves.len());

        for (step_idx, mov) in session.moves.iter().enumerate() {
            let step = if let Some(ref choices) = mov.choices {
                let rewrites = collect_confirmed_matches(&type_complex, &rule_patterns, &current)
                    .map_err(|e| format!("replay failed at step {}: {}", step_idx + 1, e))?;
                let members: Vec<FamilyMember> = choices.iter().map(|&c| {
                    rewrites.get(c).ok_or_else(|| format!(
                        "replay failed at step {}: choice {} out of range ({} rewrite(s) available)",
                        step_idx + 1, c, rewrites.len(),
                    )).map(|r| r.members[0].clone())
                }).collect::<Result<_, _>>()?;
                try_family_from_members(
                    members, &type_complex, &current, &rule_patterns,
                ).ok_or_else(|| format!(
                    "replay failed at step {}: parallel rewrite construction failed", step_idx + 1,
                ))?.step
            } else if mov.parallel {
                greedy_parallel_auto_step(&type_complex, &rule_patterns, &current)
                    .map_err(|e| format!("replay failed at step {}: {}", step_idx + 1, e))?
                    .ok_or_else(|| format!(
                        "replay failed at step {}: no parallel rewrite available", step_idx + 1,
                    ))?.step
            } else {
                let choice = mov.choice.unwrap_or(0);
                let rewrites = collect_confirmed_matches(&type_complex, &rule_patterns, &current)
                    .map_err(|e| format!("replay failed at step {}: {}", step_idx + 1, e))?;
                rewrites.get(choice).ok_or_else(|| {
                    format!(
                        "replay failed at step {}: choice {} out of range ({} rewrite(s) available)",
                        step_idx + 1, choice, rewrites.len(),
                    )
                })?.step.clone()
            };

            history.push(HistoryEntry {
                mov: mov.clone(),
            });

            current = Diagram::boundary(step_sign, n, &step)
                .map_err(|e| format!("boundary at step {}: {}", step_idx + 1, e))?;
            steps.push(step);
        }

        let rewrites = collect_confirmed_matches(
            &type_complex, &rule_patterns, &current,
        )?;

        let active_len = steps.len();
        Ok(Self {
            current_diagram: current,
            steps,
            history,
            active_len,
            rewrites,
            parallel: true,
            backward,
            rng: rand::make_rng(),
            rule_patterns,
            proof_cache: None,
            source_file: session.source_file.clone(),
            type_name: session.type_name.clone(),
            initial_diagram_name: session.initial_diagram.clone(),
            target_diagram_name: session.target_diagram.clone(),
            store,
            type_complex,
            initial_diagram,
            target_diagram,
        })
    }
}

// ── Mutating operations ───────────────────────────────────────────────────────

impl RewriteEngine {
    fn diagram_after_step(&self, step_idx: usize) -> Result<Diagram, String> {
        let n = self.initial_diagram.top_dim();
        Diagram::boundary(self.step_sign(), n, &self.steps[step_idx])
            .map_err(|e| format!("step boundary failed: {}", e))
    }

    /// Recompute available rewrites (individual matches for manual selection).
    /// Parallel mode only affects `auto()`, not the manual rewrites list.
    fn refresh_rewrites(&mut self) -> Result<(), String> {
        self.rewrites = collect_confirmed_matches(
            &self.type_complex, &self.rule_patterns, &self.current_diagram,
        )?;
        Ok(())
    }

    fn total_choices(&self) -> usize {
        self.rewrites.len()
    }

    /// Discard the redo buffer, keeping only the active prefix.
    fn truncate_redo(&mut self) {
        self.steps.truncate(self.active_len);
        self.history.truncate(self.active_len);
        if let Some(ref cache) = self.proof_cache {
            if cache.at_step > self.active_len {
                self.proof_cache = None;
            }
        }
    }

    /// Apply the rewrite at index `choice`. Records the step and advances
    /// the current diagram. Discards any redo buffer.
    pub fn step(&mut self, choice: usize) -> Result<&str, String> {
        let total = self.total_choices();
        if choice >= total {
            return Err(format!(
                "choice {} out of range ({} rewrite(s) available)",
                choice, total,
            ));
        }

        let pr = &self.rewrites[choice];
        let names: Vec<&str> = pr.members.iter()
            .map(|m| m.rule_name.as_str())
            .collect();
        let rule_name = names.join(", ");
        let step = pr.step.clone();

        let n = self.current_diagram.top_dim();

        let new_current = Diagram::boundary(self.step_sign(), n, &step)
            .map_err(|e| format!("step boundary failed: {}", e))?;

        self.truncate_redo();
        self.current_diagram = new_current;
        self.steps.push(step);

        self.history.push(HistoryEntry {
            mov: Move { choice: Some(choice), choices: None, rule_name: rule_name.clone(), parallel: false },
        });
        self.active_len = self.steps.len();

        self.refresh_rewrites()?;

        Ok(&self.history.last().unwrap().mov.rule_name)
    }

    /// Apply multiple rewrites in parallel by their indices.
    ///
    /// Checks that the selected matches are pairwise disjoint, then builds a
    /// combined parallel step via multi-pushout. Discards any redo buffer.
    pub fn step_multi(&mut self, choices: &[usize]) -> Result<&str, String> {
        let total = self.total_choices();
        for &c in choices {
            if c >= total {
                return Err(format!(
                    "choice {} out of range ({} rewrite(s) available)", c, total,
                ));
            }
        }
        let mut sorted = choices.to_vec();
        sorted.sort_unstable();
        if sorted.windows(2).any(|w| w[0] == w[1]) {
            return Err("duplicate choice index".to_string());
        }

        for i in 0..choices.len() {
            for j in (i + 1)..choices.len() {
                let a = &self.rewrites[choices[i]].image_positions;
                let b = &self.rewrites[choices[j]].image_positions;
                if a.iter().any(|x| b.contains(x)) {
                    return Err(format!(
                        "rewrites {} and {} overlap", choices[i], choices[j],
                    ));
                }
            }
        }

        let members: Vec<FamilyMember> = choices.iter().map(|&c| {
            let pr = &self.rewrites[c];
            pr.members[0].clone()
        }).collect();

        let names: Vec<&str> = members.iter().map(|m| m.rule_name.as_str()).collect();
        let rule_name = names.join(", ");

        let pr = try_family_from_members(
            members, &self.type_complex, &self.current_diagram, &self.rule_patterns,
        ).ok_or("parallel rewrite construction failed")?;

        let n = self.current_diagram.top_dim();

        let new_current = Diagram::boundary(self.step_sign(), n, &pr.step)
            .map_err(|e| format!("step boundary failed: {}", e))?;

        self.truncate_redo();
        self.current_diagram = new_current;
        self.steps.push(pr.step);
        self.history.push(HistoryEntry {
            mov: Move {
                choice: None,
                choices: Some(choices.to_vec()),
                rule_name,
                parallel: true,
            },
        });
        self.active_len = self.steps.len();

        self.refresh_rewrites()?;

        Ok(&self.history.last().unwrap().mov.rule_name)
    }

    /// Move the cursor to `target` and restore the corresponding diagram.
    fn seek(&mut self, target: usize) -> Result<(), String> {
        self.active_len = target;
        self.current_diagram = if target == 0 {
            self.initial_diagram.clone()
        } else {
            self.diagram_after_step(target - 1)?
        };
        self.refresh_rewrites()
    }

    /// Undo the last applied step. The undone step is kept in the redo buffer.
    pub fn undo(&mut self) -> Result<(), String> {
        if self.active_len == 0 { return Err("nothing to undo".to_owned()); }
        self.seek(self.active_len - 1)
    }

    /// Undo all steps, resetting to the source diagram.
    pub fn undo_all(&mut self) -> Result<(), String> {
        self.seek(0)
    }

    /// Redo the last undone step.
    pub fn redo(&mut self) -> Result<(), String> {
        if self.active_len >= self.history.len() { return Err("nothing to redo".to_owned()); }
        self.seek(self.active_len + 1)
    }

    /// Redo forward to the given step index.
    pub fn redo_to(&mut self, target_step: usize) -> Result<(), String> {
        if target_step > self.history.len() {
            return Err(format!(
                "cannot redo to step {}: only {} step(s) in history",
                target_step, self.history.len(),
            ));
        }
        if target_step <= self.active_len { return Ok(()); }
        self.seek(target_step)
    }

    /// Apply up to `max_steps` rewrites automatically, always picking the
    /// first available candidate. Discards any redo buffer.
    ///
    /// In parallel mode, uses a greedy algorithm to find a compatible family
    /// and apply it in one step.  In non-parallel mode, applies the first
    /// confirmed individual match.
    pub fn auto(&mut self, max_steps: usize) -> Result<(usize, Option<&'static str>), String> {
        self.truncate_redo();
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

            let pr = if self.parallel {
                greedy_parallel_auto_step(
                    &self.type_complex, &self.rule_patterns, &self.current_diagram,
                )?
            } else {
                find_first_match(
                    &self.type_complex, &self.rule_patterns, &self.current_diagram,
                )?
            };
            let Some(pr) = pr else {
                stop_reason = Some("no rewrites available");
                break;
            };

            let names: Vec<&str> = pr.members.iter()
                .map(|m| m.rule_name.as_str())
                .collect();
            let rule_name = names.join(", ");
            let is_parallel = self.parallel && pr.members.len() > 1;

            let n = self.current_diagram.top_dim();

            let new_current = Diagram::boundary(self.step_sign(), n, &pr.step)
                .map_err(|e| format!("step boundary failed: {}", e))?;

            self.current_diagram = new_current;
            self.steps.push(pr.step);
            self.history.push(HistoryEntry {
                mov: Move {
                    choice: if is_parallel { None } else { Some(0) },
                    choices: None,
                    rule_name,
                    parallel: is_parallel,
                },
            });
            applied += 1;
        }

        self.active_len = self.steps.len();
        self.refresh_rewrites()?;

        Ok((applied, stop_reason))
    }

    /// Apply randomly selected available rewrites.
    pub fn random(&mut self, max_steps : usize) -> Result<(usize, Option<&'static str>), String> {
        for applied in 0..max_steps {
            if self.target_reached() {
                return Ok((applied, Some("target reached")));
            }

            // TODO: Speed this up by not using `step`, but first picking a
            // random rule, and then finding a random instance
            let total = self.total_choices();
            if total == 0 {
                return Ok((applied, Some("no rewrites available")));
            }
            let choice = self.rng.random_range(0..total);
            self.step(choice)?;
        }

        Ok((max_steps, None))
    }

    /// Undo back to (but not past) the given step index (0 = fully undone,
    /// 1 = after step 1, etc.). Undone steps are kept in the redo buffer.
    pub fn undo_to(&mut self, target_step: usize) -> Result<(), String> {
        if target_step > self.active_len {
            return Err(format!(
                "cannot undo to step {}: only {} step(s) applied",
                target_step, self.active_len,
            ));
        }
        if target_step == self.active_len { return Ok(()); }
        self.seek(target_step)
    }

    /// Toggle parallel rewrite mode on or off. Only affects `auto()`.
    pub fn set_parallel(&mut self, on: bool) {
        self.parallel = on;
    }
}

// ── Read-only accessors ───────────────────────────────────────────────────────

impl RewriteEngine {
    /// Number of active rewrite steps (excludes the redo buffer).
    pub fn step_count(&self) -> usize { self.active_len }

    /// Whether there are undone steps that can be redone.
    pub fn can_redo(&self) -> bool { self.active_len < self.history.len() }

    /// The n-diagram being actively rewritten (changes after each [`step`](Self::step)).
    pub fn current_diagram(&self) -> &Diagram { &self.current_diagram }

    /// The fixed n-diagram that the session started from.
    pub fn initial_diagram(&self) -> &Diagram { &self.initial_diagram }

    /// The declared goal diagram, or `None` if no target was specified.
    pub fn target_diagram(&self) -> Option<&Diagram> { self.target_diagram.as_ref() }

    /// Whether this session uses backward rewriting (output → input).
    pub fn backward(&self) -> bool { self.backward }

    /// The boundary sign used to extract the new current diagram from a step.
    /// Forward: `Target` (output boundary). Backward: `Source` (input boundary).
    fn step_sign(&self) -> Sign {
        if self.backward { Sign::Source } else { Sign::Target }
    }

    /// The active (n+1)-dimensional rewrite steps, in order.
    ///
    /// Each step is a single rewrite — they are *not* pasted together here.
    /// Use [`assemble_proof`](Self::assemble_proof) to build the full proof
    /// diagram (only done when storing or typechecking).
    pub fn steps(&self) -> &[Diagram] { &self.steps[..self.active_len] }

    /// Paste the recorded rewrite steps together into the full (n+1)-dimensional
    /// proof diagram.  Returns `Ok(None)` if no steps have been taken.
    ///
    /// This is the one place where the "diagram up to now" is actually built —
    /// call it only when the assembled proof is needed (storing, typechecking,
    /// or rendering the final proof label).
    pub fn assemble_proof(&self) -> Result<Option<Diagram>, String> {
        let n = self.initial_diagram.top_dim();
        let active = &self.steps[..self.active_len];
        let mut iter = active.iter();
        let Some(first) = iter.next() else { return Ok(None); };
        let mut acc = first.clone();
        for step in iter {
            let (left, right) = if self.backward { (step, &acc) } else { (&acc, step) };
            acc = Diagram::paste(n, left, right)
                .map_err(|e| format!("compose step failed: {}", e))?;
        }
        Ok(Some(acc))
    }

    /// The rewrites applicable to the current diagram, precomputed after
    /// each [`step`](Self::step) or [`undo`](Self::undo).
    pub fn rewrites(&self) -> &[MatchResult] { &self.rewrites }

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

    /// Iterate over the active moves in history order.
    pub fn history_moves(&self) -> impl Iterator<Item = &Move> {
        self.history[..self.active_len].iter().map(|e| &e.mov)
    }

    /// Canonical path to the `.ali` source file for this session.
    pub fn source_file(&self) -> &str { &self.source_file }

    /// Name of the type whose generators serve as rewrite rules.
    pub fn type_name(&self) -> &str { &self.type_name }

    /// Name of the source n-diagram (as declared in the type or module).
    pub fn initial_diagram_name(&self) -> &str { &self.initial_diagram_name }

    /// Name of the target diagram, or `None` if no goal was declared.
    pub fn target_diagram_name(&self) -> Option<&str> { self.target_diagram_name.as_deref() }

    /// Set or replace the target diagram on a running session.
    pub fn set_target(&mut self, name: &str) -> Result<(), String> {
        let diag = eval_diagram_expr(&self.store, &self.type_complex, &self.source_file, name)?;
        check_parallel(&self.initial_diagram, &diag)?;
        self.target_diagram = Some(diag);
        self.target_diagram_name = Some(name.to_owned());
        Ok(())
    }

    // ── Proof cache ──────────────────────────────────────────────────────

    /// Enable the proof cache, building the initial snapshot at the current step.
    pub fn enable_proof_cache(&mut self) -> Result<(), String> {
        match self.assemble_proof()? {
            Some(proof) => {
                self.proof_cache = Some(ProofCache {
                    snapshot: proof,
                    at_step: self.active_len,
                });
            }
            None => {
                self.proof_cache = Some(ProofCache {
                    snapshot: self.initial_diagram.clone(),
                    at_step: 0,
                });
            }
        }
        Ok(())
    }

    pub fn disable_proof_cache(&mut self) {
        self.proof_cache = None;
    }

    pub fn proof_cache(&self) -> Option<&ProofCache> {
        self.proof_cache.as_ref()
    }

    pub fn proof_cache_active(&self) -> bool {
        self.proof_cache.is_some()
    }

    /// Return the assembled proof diagram, using the cache if available.
    /// At step 0, returns the source diagram.
    pub fn proof_diagram(&mut self) -> Result<Diagram, String> {
        if self.active_len == 0 {
            if let Some(ref mut cache) = self.proof_cache {
                cache.snapshot = self.initial_diagram.clone();
                cache.at_step = 0;
            }
            return Ok(self.initial_diagram.clone());
        }
        if let Some(ref cache) = self.proof_cache {
            if cache.at_step == self.active_len {
                return Ok(cache.snapshot.clone());
            }
        }
        self.sync_proof_cache()?;
        Ok(self.proof_cache.as_ref().unwrap().snapshot.clone())
    }

    /// Bring the proof cache up to date with the current active step.
    fn sync_proof_cache(&mut self) -> Result<(), String> {
        if self.proof_cache.is_none() { return Ok(()); }

        if self.active_len == 0 {
            self.proof_cache = Some(ProofCache {
                snapshot: self.initial_diagram.clone(),
                at_step: 0,
            });
            return Ok(());
        }

        let cache_step = self.proof_cache.as_ref().unwrap().at_step;
        if cache_step == self.active_len {
            return Ok(());
        }

        let n = self.initial_diagram.top_dim();

        let proof = if cache_step < self.active_len && cache_step > 0 {
            let mut acc = self.proof_cache.as_ref().unwrap().snapshot.clone();
            for step in &self.steps[cache_step..self.active_len] {
                let (left, right) = if self.backward { (step, &acc) } else { (&acc, step) };
                acc = Diagram::paste(n, left, right)
                    .map_err(|e| format!("proof cache extend failed: {}", e))?;
            }
            acc
        } else {
            self.assemble_proof()?
                .unwrap_or_else(|| self.initial_diagram.clone())
        };

        self.proof_cache = Some(ProofCache {
            snapshot: proof,
            at_step: self.active_len,
        });
        Ok(())
    }

    /// Returns true only if the current diagram is isomorphic to the target AND
    /// at least one rewrite step has been applied.  Regular directed complexes
    /// have no identities, so source == target at the start is never a valid proof.
    pub fn target_reached(&self) -> bool {
        self.active_len > 0
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
    /// 1. Initial boundary: the proof's boundary on the initial side is
    ///    isomorphic to `initial_diagram` (source in forward, target in backward).
    /// 2. Round-trip: sourcefies the proof, re-interprets it through the interpreter, and
    ///    confirms the result is isomorphic to the constructed proof.
    ///
    /// Returns `Ok(())` if both pass, `Err(message)` on any failure.
    /// Returns `Err` immediately if no proof steps have been taken.
    pub fn typecheck_proof(&self) -> Result<(), String> {
        let assembled = self.assemble_proof()?
            .ok_or_else(|| "no proof steps taken yet".to_owned())?;
        let diagram = &assembled;

        let n = self.initial_diagram.top_dim();
        let check_sign = if self.backward { Sign::Target } else { Sign::Source };
        let boundary = Diagram::boundary(check_sign, n, diagram)
            .map_err(|e| format!("boundary check failed: {}", e))?;
        if !Diagram::isomorphic(&boundary, &self.initial_diagram) {
            return Err(format!(
                "proof boundary does not match initial diagram '{}' — \
                 this is a bug in the rewrite engine",
                self.initial_diagram_name,
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
            Arc::new(String::new()),
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
        let diagram = if let Some(ref cache) = self.proof_cache {
            if cache.at_step == self.active_len {
                cache.snapshot.clone()
            } else {
                self.assemble_proof()?.unwrap_or_else(|| self.initial_diagram.clone())
            }
        } else {
            self.assemble_proof()?.unwrap_or_else(|| self.initial_diagram.clone())
        };

        if self.type_complex.name_in_use(name) || self.type_complex.find_generator(name).is_some() {
            return Err(format!("name '{}' is already in use", name));
        }

        let type_gid = {
            let mc = self.store.find_module(&self.source_file)
                .ok_or_else(|| format!("module '{}' not found", self.source_file))?;
            match mc.find_generator(&self.type_name) {
                Some((Tag::Global(gid), _)) => *gid,
                _ => return Err(format!("type '{}' not found in module", self.type_name)),
            }
        };

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
            initial_diagram: self.initial_diagram_name.clone(),
            target_diagram: self.target_diagram_name.clone(),
            backward: self.backward,
            moves: self.history[..self.active_len].iter().map(|e| e.mov.clone()).collect(),
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
            Request::StepMulti { choices } => {
                if !self.parallel {
                    Err("multi-apply requires parallel mode".to_string())
                } else {
                    match self.step_multi(choices) {
                        Ok(_) => Ok(build_response(self, false)),
                        Err(e) => Err(e),
                    }
                }
            }
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
            Request::Random { max_steps } => match self.random(*max_steps) {
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
            Request::Redo => self.redo().map(|_| build_response(self, false)),
            Request::RedoTo { step } => {
                self.redo_to(*step).map(|_| build_response(self, false))
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
                    Some(StoredInfo {
                        type_name: self.type_name().to_owned(),
                        def_name: name.clone(),
                        expr: crate::output::render_diagram(self.initial_diagram(), self.type_complex()),
                    })
                } else {
                    let n = self.initial_diagram().top_dim();
                    let scope = self.type_complex();
                    let ordered: Vec<_> = if self.backward {
                        self.steps().iter().rev().collect()
                    } else {
                        self.steps().iter().collect()
                    };
                    let mut steps = ordered.iter();
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
                self.set_parallel(*on);
                Ok(build_response(self, false))
            }
            Request::SetTarget { name } => {
                self.set_target(name).map(|_| build_response(self, false))
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
