//! Stateful rewrite engine: holds session state in memory for incremental
//! step/apply without re-interpreting the source file.

use crate::aux::loader::Loader;
use crate::aux::Tag;
use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, Sign};
use crate::core::matching::{
    FamilyMember, MatchResult, RulePattern,
    confirm_candidate,
    for_each_rule_candidate,
    greedy_parallel_auto_step,
    try_family_from_members,
};
use crate::interpreter::{GlobalStore, InterpretedFile};
use super::session::{Move, SessionFile};
use std::collections::HashMap;
use std::sync::Arc;

struct HistoryEntry {
    mov: Move,
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
    rewrites: Vec<MatchResult>,
    parallel: bool,

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
        let rewrites = collect_confirmed_matches(
            &type_complex, &rule_patterns, &source_diagram,
        )?;

        Ok(Self {
            current_diagram: source_diagram.clone(),
            steps: Vec::new(),
            history: Vec::new(),
            rewrites,
            parallel: true,
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
        let rewrites = collect_confirmed_matches(
            &type_complex, &rule_patterns, &source_diagram,
        )?;

        Ok(Self {
            current_diagram: source_diagram.clone(),
            steps: Vec::new(),
            history: Vec::new(),
            rewrites,
            parallel: true,
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

    /// Load the source file and replay a saved session.
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

            current = Diagram::boundary(Sign::Target, n, &step)
                .map_err(|e| format!("target boundary at step {}: {}", step_idx + 1, e))?;
            steps.push(step);
        }

        let rewrites = collect_confirmed_matches(
            &type_complex, &rule_patterns, &current,
        )?;

        Ok(Self {
            current_diagram: current,
            steps,
            history,
            rewrites,
            parallel: true,
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
    fn diagram_after_step(&self, step_idx: usize) -> Result<Diagram, String> {
        let n = self.source_diagram.top_dim();
        Diagram::boundary(Sign::Target, n, &self.steps[step_idx])
            .map_err(|e| format!("target boundary failed: {}", e))
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

    /// Apply the rewrite at index `choice`. Records the step and advances
    /// the current diagram.
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

        let new_current = Diagram::boundary(Sign::Target, n, &step)
            .map_err(|e| format!("target boundary failed: {}", e))?;

        self.current_diagram = new_current;
        self.steps.push(step);

        self.history.push(HistoryEntry {
            mov: Move { choice: Some(choice), choices: None, rule_name: rule_name.clone(), parallel: false },
        });

        self.refresh_rewrites()?;

        Ok(&self.history.last().unwrap().mov.rule_name)
    }

    /// Apply multiple rewrites in parallel by their indices.
    ///
    /// Checks that the selected matches are pairwise disjoint, then builds a
    /// combined parallel step via multi-pushout.
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

        let new_current = Diagram::boundary(Sign::Target, n, &pr.step)
            .map_err(|e| format!("target boundary failed: {}", e))?;

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

        self.refresh_rewrites()?;

        Ok(&self.history.last().unwrap().mov.rule_name)
    }

    /// Undo the last applied step, restoring the previous diagram state.
    ///
    /// Returns an error if there are no moves to undo.
    pub fn undo(&mut self) -> Result<(), String> {
        self.history.pop().ok_or("nothing to undo")?;
        self.steps.pop();
        self.current_diagram = if self.steps.is_empty() {
            self.source_diagram.clone()
        } else {
            self.diagram_after_step(self.steps.len() - 1)?
        };
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
    /// In parallel mode, uses a greedy algorithm to find a compatible family
    /// and apply it in one step.  In non-parallel mode, applies the first
    /// confirmed individual match.
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

            let new_current = Diagram::boundary(Sign::Target, n, &pr.step)
                .map_err(|e| format!("target boundary failed: {}", e))?;

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
        self.history.truncate(target_step);
        self.steps.truncate(target_step);
        self.current_diagram = if target_step == 0 {
            self.source_diagram.clone()
        } else {
            self.diagram_after_step(target_step - 1)?
        };
        self.refresh_rewrites()?;
        Ok(())
    }

    /// Toggle parallel rewrite mode on or off. Only affects `auto()`.
    pub fn set_parallel(&mut self, on: bool) {
        self.parallel = on;
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

    /// Set or replace the target diagram on a running session.
    pub fn set_target(&mut self, name: &str) -> Result<(), String> {
        let diag = eval_diagram_expr(&self.store, &self.type_complex, &self.source_file, name)?;
        self.target_diagram = Some(diag);
        self.target_diagram_name = Some(name.to_owned());
        Ok(())
    }

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
        let diagram = self.assemble_proof()?
            .unwrap_or_else(|| self.source_diagram.clone());

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
                    Some(StoredInfo {
                        type_name: self.type_name().to_owned(),
                        def_name: name.clone(),
                        expr: self.source_diagram_name().to_owned(),
                    })
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
