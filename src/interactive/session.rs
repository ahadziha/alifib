//! The shared interactive session state machine.
//!
//! `Session` owns everything a REPL session needs — the store, the running
//! source, the active rewrite engine or fill, the `backward` flag — and a single
//! [`apply`](Session::apply) method that performs **all** command semantics,
//! state transitions, and canonical user messages.  The three front-ends
//! (CLI, stdio daemon, web) are thin adapters over it: they parse input into a
//! [`Request`], call `apply`, and render the resulting [`ResponseData`] in their
//! own medium.  Behaviour therefore lives in one place and cannot drift between
//! front-ends; a new command lands on all three at once.
//!
//! Loading the source (disk vs. virtual modules) and the final byte-rendering
//! are the only genuinely per-front-end concerns; the first is captured by
//! [`LoadStrategy`], the second is left to the adapters.

use std::collections::HashMap;
use std::sync::Arc;

use crate::aux::loader::Loader;
use crate::interpreter::{GlobalStore, InterpretedFile, LoadResult};
use crate::output::render_diagram;

use super::engine::{load_file_context, reevaluate, resolve_type, RewriteEngine};
use super::fill::{
    edit_for_fill, filled_report, list_open_holes, start_fill,
    FillContext, FillSession,
};
use super::protocol::{
    build_list_rules_response, build_response, AutoInfo, FillInfo, HoleInfo, Request,
    ResponseData, StoredInfo, ZeroCellChoice, ZeroCellInfo,
};

/// How the session loads and re-evaluates source.
pub enum LoadStrategy {
    /// Read the root and its dependencies from disk.
    Disk,
    /// Serve the root and all `include`s from an in-memory file map (the web).
    Virtual(HashMap<String, String>),
}

/// A live interactive session: the single source of truth for session behaviour.
pub struct Session {
    store: Arc<GlobalStore>,
    root_path: String,
    source: String,
    /// A free rewrite session (`start`/`resume`); mutually exclusive with `fill`.
    engine: Option<RewriteEngine>,
    /// A hole-filling session; mutually exclusive with `engine`.
    fill: Option<(FillContext, FillSession)>,
    backward: bool,
    loader: LoadStrategy,
}

impl Session {
    // ── Construction ────────────────────────────────────────────────────────

    /// Load `source_file` and its dependencies from disk.
    pub fn from_disk(source_file: &str) -> Result<Self, String> {
        let (store, root_path, _output) = load_file_context(source_file)?;
        let source = std::fs::read_to_string(source_file).unwrap_or_default();
        Ok(Self::new(store, root_path, source, LoadStrategy::Disk))
    }

    /// Load `source` plus virtual `modules` (`<Name>.ali → contents`), as the web
    /// does.  `name` overrides the root's virtual filename.  Returns the session
    /// and the loaded store on success.
    pub fn from_virtual(
        source: &str,
        modules: HashMap<String, String>,
        name: Option<&str>,
    ) -> Result<Self, String> {
        let root_path = name
            .filter(|n| !n.is_empty())
            .map(|n| if n.ends_with(".ali") { n.to_string() } else { format!("{}.ali", n) })
            .unwrap_or_else(|| "source.ali".to_string());

        let mut files = modules.clone();
        files.insert(root_path.clone(), source.to_string());
        let loader = Loader::with_virtual_files(files);
        match InterpretedFile::load(&loader, &root_path) {
            LoadResult::Loaded(file) => {
                let store = Arc::clone(&file.state);
                Ok(Self::new(store, root_path, source.to_owned(), LoadStrategy::Virtual(modules)))
            }
            LoadResult::LoadError(e) => Err(format!("{:?}", e)),
            LoadResult::InterpError { errors, source: src, path } => Err(errors
                .iter()
                .map(|e| e.to_diagnostic(&src, Some(path.clone())).message)
                .collect::<Vec<_>>()
                .join("; ")),
        }
    }

    /// Construct a session around an already-loaded store (used by the web,
    /// which does its own load to surface structured diagnostics).
    pub fn from_loaded(store: Arc<GlobalStore>, root_path: String, source: String, loader: LoadStrategy) -> Self {
        Self::new(store, root_path, source, loader)
    }

    fn new(store: Arc<GlobalStore>, root_path: String, source: String, loader: LoadStrategy) -> Self {
        Session { store, root_path, source, engine: None, fill: None, backward: false, loader }
    }

    // ── Accessors (for the front-ends' renderers) ────────────────────────────

    pub fn store(&self) -> &Arc<GlobalStore> { &self.store }
    pub fn root_path(&self) -> &str { &self.root_path }
    pub fn source(&self) -> &str { &self.source }
    pub fn backward(&self) -> bool { self.backward }
    pub fn engine(&self) -> Option<&RewriteEngine> { self.engine.as_ref() }
    pub fn fill(&self) -> Option<&(FillContext, FillSession)> { self.fill.as_ref() }
    /// The engine driving the current rewrite — a free session or a fill's — for
    /// renderers (string diagrams, proof view).  `None` during a 0-cell fill.
    pub fn active_engine(&self) -> Option<&RewriteEngine> { self.engine_ref() }
    pub fn active_engine_mut(&mut self) -> Option<&mut RewriteEngine> { self.engine_mut() }
    pub fn session_active(&self) -> bool { self.engine.is_some() || self.fill.is_some() }

    /// A `ResponseData` snapshot of the current state (for an initial emit).
    pub fn state(&self) -> ResponseData { self.snapshot() }

    // ── Command dispatch ─────────────────────────────────────────────────────

    /// Perform one command.  `Ok` carries a `ResponseData` snapshot of the
    /// resulting state (with a canonical `message` where applicable); `Err`
    /// carries the user-facing error.  Read-only queries (`types`/`type`/`cell`/
    /// `homology`) and front-end-only commands (`print`/`help`/`quit`/`shutdown`)
    /// are handled by the adapters, not here.
    pub fn apply(&mut self, req: Request) -> Result<ResponseData, String> {
        use Request::*;
        match req {
            Start { type_name, initial, target, backward, .. } =>
                self.start_rewrite(&type_name, &initial, target.as_deref(), backward),
            Resume { type_name, proof, target, backward, .. } =>
                self.resume_rewrite(&type_name, &proof, target.as_deref(), backward),
            Stop => self.stop(),
            Backward { on } => self.set_backward(on),
            Holes => Ok(self.holes_response()),
            Fill { index, backward } => self.begin_fill(index, backward),
            Done => self.finalize_fill(),
            Save { path } => self.save(path),

            Step { choice } => self.engine_step(&[choice]),
            StepMulti { choices } => self.engine_step(&choices),
            Auto { max_steps } => self.engine_auto(max_steps, false),
            Random { max_steps } => self.engine_auto(max_steps, true),
            Undo => self.engine_undo(None, false),
            UndoTo { step } => self.engine_undo(Some(step), false),
            Redo => self.engine_undo(None, true),
            RedoTo { step } => self.engine_undo(Some(step), true),
            Parallel { on } => self.set_parallel(on),
            SetTarget { name } => self.engine_set_target(&name),
            Show => self.snapshot_active(),
            Proof => self.proof_response(),
            History => self.history_response(),
            ListRules => self.rules_response(),
            Store { name } => self.store_proof(&name),

            Load { .. } | Types | TypeInfo { .. } | Cell { .. } | Homology { .. } | Shutdown =>
                Err("Not a session command".to_owned()),
        }
    }

    // ── Session transitions ───────────────────────────────────────────────────

    fn start_rewrite(&mut self, type_name: &str, initial: &str, target: Option<&str>, backward: bool)
        -> Result<ResponseData, String>
    {
        if self.session_active() {
            return Err("Session already active — stop it first".to_owned());
        }
        let tc = resolve_type(&self.store, &self.root_path, type_name)?;
        let engine = RewriteEngine::from_store(
            Arc::clone(&self.store), tc, initial, target,
            self.root_path.clone(), type_name.to_owned(), backward,
        )?;
        self.engine = Some(engine);
        let mut data = self.snapshot();
        data.message = Some("Started rewrite session".to_owned());
        Ok(data)
    }

    fn resume_rewrite(&mut self, type_name: &str, proof: &str, target: Option<&str>, backward: bool)
        -> Result<ResponseData, String>
    {
        if self.session_active() {
            return Err("Session already active — stop it first".to_owned());
        }
        let tc = resolve_type(&self.store, &self.root_path, type_name)?;
        let engine = RewriteEngine::resume(
            Arc::clone(&self.store), tc, proof, target,
            self.root_path.clone(), type_name.to_owned(), backward,
        )?;
        self.engine = Some(engine);
        let mut data = self.snapshot();
        data.message = Some("Resumed rewrite session".to_owned());
        Ok(data)
    }

    fn stop(&mut self) -> Result<ResponseData, String> {
        let message = if self.fill.take().is_some() {
            "Fill abandoned"
        } else {
            self.engine = None;
            "Session stopped"
        };
        let mut data = self.snapshot();
        data.message = Some(message.to_owned());
        Ok(data)
    }

    fn set_backward(&mut self, on: Option<bool>) -> Result<ResponseData, String> {
        // Mid-session the mode is fixed; only report it.  Idle, toggle/set it.
        if !self.session_active() {
            if let Some(b) = on { self.backward = b; }
        }
        let mut data = self.snapshot();
        data.backward = self.session_backward();
        data.message = Some(format!("Backward mode {}", if data.backward { "on" } else { "off" }));
        Ok(data)
    }

    fn session_backward(&self) -> bool {
        match (&self.engine, &self.fill) {
            (Some(e), _) => e.backward(),
            (_, Some((_, FillSession::Rewrite(e)))) => e.backward(),
            _ => self.backward,
        }
    }

    // ── Hole filling ──────────────────────────────────────────────────────────

    fn holes_response(&self) -> ResponseData {
        let mut data = ResponseData::empty();
        data.holes = list_open_holes(&self.store, &self.root_path).iter().map(|h| HoleInfo {
            index: h.index,
            type_name: h.type_name.clone(),
            map_name: h.map_name.clone(),
            domain_name: h.domain_name.clone(),
            source_name: h.source_name.clone(),
            dim: h.dim,
            boundary: h.boundary.clone(),
        }).collect();
        data
    }

    fn begin_fill(&mut self, index: usize, backward: bool) -> Result<ResponseData, String> {
        if self.session_active() {
            return Err("Session already active — stop it first".to_owned());
        }
        let (ctx, session) = start_fill(&self.store, &self.root_path, &self.root_path, index, backward)?;
        let boundary = ctx.boundary.clone();
        self.fill = Some((ctx, session));
        let mut data = self.snapshot();
        data.message = Some(format!("Filling {}", boundary));
        Ok(data)
    }

    fn finalize_fill(&mut self) -> Result<ResponseData, String> {
        let (ctx, session) = self.fill.as_ref().ok_or("No active fill — use 'fill <n>'")?;
        let filler = session.filler()?;
        let message = filled_report(&self.store, ctx, &filler);
        let new_source = edit_for_fill(&self.store, ctx, &filler, &self.source)?;
        // An inconsistent fill makes re-evaluation error; report it and keep the
        // session so the user can retry.
        let new_store = self.reevaluate(&new_source)?;
        self.store = new_store;
        self.source = new_source;
        self.fill = None;
        let mut data = self.snapshot();
        data.message = Some(message);
        data.source = Some(self.source.clone());
        Ok(data)
    }

    // ── Persistence ─────────────────────────────────────────────────────────

    fn save(&self, path: Option<String>) -> Result<ResponseData, String> {
        let mut data = self.snapshot();
        match &self.loader {
            LoadStrategy::Disk => {
                let target = path.unwrap_or_else(|| self.root_path.clone());
                std::fs::write(&target, format!("{}\n", self.source.trim_end()))
                    .map_err(|e| format!("cannot write '{}': {}", target, e))?;
                data.message = Some(format!("Saved to '{}'", target));
            }
            LoadStrategy::Virtual(_) => {
                // The editor performs the actual write; hand it the source.
                data.source = Some(self.source.clone());
            }
        }
        Ok(data)
    }

    // ── Engine commands ────────────────────────────────────────────────────────

    fn engine_step(&mut self, choices: &[usize]) -> Result<ResponseData, String> {
        if let Some(zc) = self.zero_cell_mut() {
            let k = choices.first().copied().unwrap_or(0);
            zc.choose(k)?;
            let name = self.zero_cell().and_then(|z| z.chosen_name()).unwrap_or("?").to_owned();
            let mut data = self.snapshot();
            data.message = Some(format!("Chose {}", name));
            return Ok(data);
        }
        let parallel = self.engine_ref().map(|e| e.parallel()).unwrap_or(false);
        let e = self.engine_mut().ok_or("No active session — use 'start' or 'fill'")?;
        let rule = if choices.len() == 1 {
            e.step(choices[0])
        } else if parallel {
            e.step_multi(choices)
        } else {
            return Err("Multi-apply requires parallel mode".to_owned());
        }?.to_owned();
        let mut data = self.snapshot();
        data.message = Some(format!("Applied {}", rule));
        Ok(data)
    }

    fn engine_auto(&mut self, n: usize, random: bool) -> Result<ResponseData, String> {
        let e = self.engine_mut().ok_or("No active session — use 'start' or 'fill'")?;
        let (applied, stop) = if random { e.random(n) } else { e.auto(n) }?;
        let tail = stop.map(|r| format!(" ({})", r)).unwrap_or_default();
        let msg = format!("Applied {} step{}{}", applied, if applied == 1 { "" } else { "s" }, tail);
        let mut data = self.snapshot();
        data.auto = Some(AutoInfo { applied, stop_reason: stop.unwrap_or("").to_owned() });
        data.message = Some(msg);
        Ok(data)
    }

    fn engine_undo(&mut self, step: Option<usize>, redo: bool) -> Result<ResponseData, String> {
        if let Some(zc) = self.zero_cell_mut() {
            if redo { zc.redo()?; } else { zc.undo()?; }
            return self.snapshot_active();
        }
        let e = self.engine_mut().ok_or("No active session — use 'start' or 'fill'")?;
        let reset = !redo && step == Some(0);
        match (redo, step) {
            (false, None) => e.undo()?,
            (false, Some(s)) => e.undo_to(s)?,
            (true, None) => e.redo()?,
            (true, Some(s)) => e.redo_to(s)?,
        }
        let mut data = self.snapshot();
        if reset { data.message = Some("Reset to source".to_owned()); }
        Ok(data)
    }

    fn set_parallel(&mut self, on: bool) -> Result<ResponseData, String> {
        if let Some(e) = self.engine_mut() { e.set_parallel(on); }
        let mut data = self.snapshot();
        data.message = Some(format!("Parallel mode {}", if on { "on" } else { "off" }));
        Ok(data)
    }

    fn engine_set_target(&mut self, name: &str) -> Result<ResponseData, String> {
        let e = self.engine_mut().ok_or("No active session")?;
        e.set_target(name)?;
        self.snapshot_active()
    }

    /// The running proof as the re-parseable expression `store` would persist
    /// (with its `proof` boundary in `data.proof`).  A zero-step session is the
    /// identity proof on the initial diagram — still a proof — so `proof_expr`
    /// is the rendered initial diagram, never `None`, for an engine session.
    fn proof_response(&self) -> Result<ResponseData, String> {
        let mut data = self.snapshot_active()?;
        data.proof_expr = self.engine_ref().map(stored_expr);
        Ok(data)
    }

    fn history_response(&self) -> Result<ResponseData, String> {
        match self.engine_ref() {
            Some(e) => Ok(build_response(e, true)),
            None => self.snapshot_active(),
        }
    }

    fn rules_response(&self) -> Result<ResponseData, String> {
        match self.engine_ref() {
            Some(e) => Ok(build_list_rules_response(e)),
            None => Err("No active session".to_owned()),
        }
    }

    fn store_proof(&mut self, name: &str) -> Result<ResponseData, String> {
        if matches!(&self.fill, Some((_, FillSession::ZeroCell(_)))) {
            return Err("Nothing to store in a 0-cell fill".to_owned());
        }
        let e = self.engine_mut().ok_or("No active session")?;
        let expr = stored_expr(e);
        let type_name = e.type_name().to_owned();
        let (new_store, _) = e.register_proof(name)?;
        self.store = new_store;
        self.source = format!("{}\n\n@{}\nlet {} = {}\n", self.source.trim_end(), type_name, name, expr);
        let mut data = self.snapshot();
        data.stored = Some(StoredInfo { type_name, def_name: name.to_owned(), expr });
        data.message = Some(format!("Stored '{}'", name));
        data.source = Some(self.source.clone());
        Ok(data)
    }

    // ── Snapshot + internal helpers ──────────────────────────────────────────

    /// Build a `ResponseData` for the current state — the active rewrite engine's
    /// state, a 0-cell fill's state, or an empty response when idle.
    fn snapshot(&self) -> ResponseData {
        match &self.fill {
            Some((ctx, FillSession::Rewrite(e))) => {
                let mut d = build_response(e, false);
                d.fill = Some(fill_info(ctx));
                d
            }
            Some((ctx, FillSession::ZeroCell(zc))) => {
                let mut d = ResponseData::empty();
                d.fill = Some(fill_info(ctx));
                d.target_reached = zc.target_reached();
                d.zero_cell = Some(zero_cell_info(zc));
                d
            }
            None => match &self.engine {
                Some(e) => build_response(e, false),
                None => ResponseData::empty(),
            },
        }
    }

    fn snapshot_active(&self) -> Result<ResponseData, String> {
        if self.session_active() { Ok(self.snapshot()) } else { Err("No active session".to_owned()) }
    }

    fn engine_ref(&self) -> Option<&RewriteEngine> {
        match &self.fill {
            Some((_, FillSession::Rewrite(e))) => Some(e),
            Some((_, FillSession::ZeroCell(_))) => None,
            None => self.engine.as_ref(),
        }
    }

    fn engine_mut(&mut self) -> Option<&mut RewriteEngine> {
        match &mut self.fill {
            Some((_, FillSession::Rewrite(e))) => Some(e),
            Some((_, FillSession::ZeroCell(_))) => None,
            None => self.engine.as_mut(),
        }
    }

    fn zero_cell(&self) -> Option<&super::fill::ZeroCellFill> {
        match &self.fill {
            Some((_, FillSession::ZeroCell(zc))) => Some(zc),
            _ => None,
        }
    }

    fn zero_cell_mut(&mut self) -> Option<&mut super::fill::ZeroCellFill> {
        match &mut self.fill {
            Some((_, FillSession::ZeroCell(zc))) => Some(zc),
            _ => None,
        }
    }

    fn reevaluate(&self, new_source: &str) -> Result<Arc<GlobalStore>, String> {
        match &self.loader {
            LoadStrategy::Disk => reevaluate(&self.root_path, new_source),
            LoadStrategy::Virtual(modules) => {
                let mut files = modules.clone();
                files.insert(self.root_path.clone(), new_source.to_string());
                let loader = Loader::with_virtual_files(files);
                match InterpretedFile::load(&loader, &self.root_path) {
                    LoadResult::Loaded(file) => Ok(Arc::clone(&file.state)),
                    LoadResult::LoadError(e) => Err(format!("{:?}", e)),
                    LoadResult::InterpError { errors, source, path } => Err(errors
                        .iter()
                        .map(|e| e.to_diagnostic(&source, Some(path.clone())).message)
                        .collect::<Vec<_>>()
                        .join("; ")),
                }
            }
        }
    }
}

/// The expression `store` persists for the running proof, and `proof` displays:
/// the composite of the rewrite steps, or — for a **zero-step** (identity)
/// proof — the initial diagram itself.  A zero-step proof is still a proof, so
/// this is always defined for an active engine session.
fn stored_expr(e: &RewriteEngine) -> String {
    e.proof_expr().unwrap_or_else(|| render_diagram(e.initial_diagram(), e.type_complex()))
}

fn fill_info(ctx: &FillContext) -> FillInfo {
    FillInfo {
        type_name: ctx.type_name.clone(),
        map_name: ctx.map_name.clone(),
        domain_name: ctx.domain_name.clone(),
        source_name: ctx.source_name.clone(),
        dim: ctx.dim,
    }
}

fn zero_cell_info(zc: &super::fill::ZeroCellFill) -> ZeroCellInfo {
    let choices = if zc.chosen.is_some() {
        Vec::new()
    } else {
        zc.choices.iter().enumerate()
            .map(|(i, (_, name))| ZeroCellChoice { index: i, name: name.clone() })
            .collect()
    };
    ZeroCellInfo {
        choices,
        chosen: zc.chosen_name().map(str::to_owned),
        target_reached: zc.target_reached(),
        can_undo: zc.chosen.is_some(),
        can_redo: zc.can_redo(),
    }
}
