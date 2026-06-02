//! Shared browser-facing API for the web GUI.
//!
//! `WebRepl` is the stateful adapter used by both web backends — the HTTP
//! server at `web/server/` and the WASM bindings at `web/wasm/`.  Command
//! dispatch delegates to [`RewriteEngine::handle`], which is the same
//! surface the stdio daemon uses at `super::daemon`; the only per-backend
//! work is session setup (`start_session`/`reset`) and the commands that
//! bypass the engine (currently just `homology`, which queries the
//! interpreter's global store directly).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;

use crate::aux::Tag;
use crate::aux::loader::{LoadFileError, Loader};
use crate::core::complex::Complex;
use crate::core::diagram::{CellData, Diagram, Sign};
use crate::analysis::strdiag::StrDiag;
use crate::interpreter::{GlobalStore, InterpretedFile, LoadResult};
use crate::language::error::Diagnostic;

use super::engine::{RewriteEngine, resolve_type};
use super::fill::{edit_for_fill, filled_report, list_open_holes, start_fill, FillContext, FillSession, ZeroCellFill};
use super::protocol::{
    FillInfo, Request, Response, build_homology_response, build_map_entries, build_map_image_strdiag,
    build_response, build_strdiag_response, build_types_from_store, build_type_detail_from_store,
    resolve_domain_complex, step_output_strdiag_json, strdiag_json_from_diagram,
    strdiag_to_json, tag_to_json,
};

pub const WEB_SOURCE_PATH: &str = "source.ali";

/// Stateful REPL wrapper shared by the browser frontends.
///
/// Lifecycle:
/// 1. `new()` — create an empty instance
/// 2. `load_source(text)` — parse and interpret `.ali` source text
/// 3. `run_command(json)` — non-session commands (`types`, `type`, `homology`)
///    work immediately after loading
/// 4. `start_session(type, initial, tgt?)` — start a rewrite session on a type
/// 5. `run_command(json)` — session commands (step/undo/show/…) plus the above
pub struct WebRepl {
    state: State,
    /// The extra virtual modules supplied at the last `load_source`, retained so
    /// that a `done` re-evaluation (after splicing a fill into the root source)
    /// resolves the same `include`s.
    modules: HashMap<String, String>,
}

/// Internal state machine.  Kept private so public callers still see a
/// single `WebRepl` — the HTTP server and wasm-bindgen shim need one
/// long-lived handle and dispatch on runtime state; exposing the states
/// as separate types would only force them to rebuild the same enum
/// around it (without gaining compile-time safety, since their request
/// streams aren't statically ordered).  Encoding the invariants
/// internally still prevents the "store=None, engine=Some(_)" shape
/// from existing, which the previous two-`Option` version allowed by
/// convention only.
enum State {
    Empty,
    Loaded { store: Arc<GlobalStore>, root_path: String, source: String },
    /// A rewrite session.  `fill` is `Some` when it is a hole-filling (which is
    /// finalised by `done`) rather than a free rewrite.
    Active {
        store: Arc<GlobalStore>,
        root_path: String,
        source: String,
        engine: RewriteEngine,
        fill: Option<FillContext>,
    },
    /// A boundaryless 0-cell fill: the moves are choosing/undoing/redoing one of
    /// the type's 0-cells.  Not a [`RewriteEngine`], so it lives in its own state.
    ZeroFill {
        store: Arc<GlobalStore>,
        root_path: String,
        source: String,
        ctx: FillContext,
        fill: ZeroCellFill,
    },
}

impl State {
    fn store(&self) -> Option<&Arc<GlobalStore>> {
        match self {
            State::Empty => None,
            State::Loaded { store, .. }
            | State::Active { store, .. }
            | State::ZeroFill { store, .. } => Some(store),
        }
    }

    fn root_path(&self) -> &str {
        match self {
            State::Empty => WEB_SOURCE_PATH,
            State::Loaded { root_path, .. }
            | State::Active { root_path, .. }
            | State::ZeroFill { root_path, .. } => root_path,
        }
    }

    fn engine(&self) -> Option<&RewriteEngine> {
        if let State::Active { engine, .. } = self { Some(engine) } else { None }
    }

    fn engine_mut(&mut self) -> Option<&mut RewriteEngine> {
        if let State::Active { engine, .. } = self { Some(engine) } else { None }
    }
}

impl Default for WebRepl {
    fn default() -> Self {
        Self::new()
    }
}

impl WebRepl {
    pub fn new() -> Self {
        Self { state: State::Empty, modules: HashMap::new() }
    }

    pub fn reset(&mut self) {
        self.state = State::Empty;
    }

    pub fn stop_session(&mut self) {
        match std::mem::replace(&mut self.state, State::Empty) {
            State::Active { store, root_path, source, .. }
            | State::ZeroFill { store, root_path, source, .. } => {
                self.state = State::Loaded { store, root_path, source };
            }
            other => self.state = other,
        }
    }

    /// Interpret `.ali` source text and return a JSON response with structured
    /// type data (generators with boundaries, diagrams, maps).
    pub fn load_source(&mut self, source: &str) -> String {
        self.load_source_with_modules(source, HashMap::new(), None)
    }

    /// Like [`load_source`], but seeds additional virtual module files into the
    /// loader.  `extra_modules` is a `<Name>.ali → contents` map — any
    /// `include <Name>` in the root source resolves to the matching entry.
    /// The frontend crates use this to bundle `examples/` as library modules,
    /// keeping the main crate free of web-specific data.
    ///
    /// `source_name` optionally overrides the virtual filename for the root
    /// source (e.g. `"Monoidal"` → `"Monoidal.ali"`).  This determines the
    /// same-named subdirectory used for `include` resolution.
    pub fn load_source_with_modules(
        &mut self,
        source: &str,
        extra_modules: HashMap<String, String>,
        source_name: Option<&str>,
    ) -> String {
        // Free old state before allocating the new store so that both don't
        // coexist — in WASM, linear memory pages from the peak are permanent.
        self.state = State::Empty;

        let root_path = source_name
            .filter(|n| !n.is_empty())
            .map(|n| if n.ends_with(".ali") { n.to_string() } else { format!("{}.ali", n) })
            .unwrap_or_else(|| WEB_SOURCE_PATH.to_string());

        // Retain the extra modules so a later `done` re-evaluation resolves the
        // same includes.
        self.modules = extra_modules.clone();

        let mut files = extra_modules;
        files.insert(root_path.clone(), source.to_string());
        let loader = Loader::with_virtual_files(files);

        match InterpretedFile::load(&loader, &root_path) {
            LoadResult::Loaded(file) => {
                let store = Arc::clone(&file.state);
                // Preserve the original frontend contract for `load_source`,
                // which returns `types` at the top level instead of under
                // `data`.
                let json = serde_json::json!({
                    "status": "ok",
                    "types": type_summaries_json(&file.state),
                })
                .to_string();
                self.state = State::Loaded { store, root_path, source: source.to_string() };
                json
            }
            LoadResult::LoadError(e) => load_error_json(&e),
            LoadResult::InterpError { errors, source, path } => {
                let diagnostics: Vec<Diagnostic> = errors
                    .iter()
                    .map(|e| e.to_diagnostic(&source, Some(path.clone())))
                    .collect();
                diagnostics_err_json(&diagnostics)
            }
        }
    }

    /// Resolve the type and install a freshly constructed engine as the active
    /// session.  Any existing session collapses back to `Loaded` first, so a
    /// failed construction leaves the caller with at least the store they had.
    /// `build` supplies the constructor (`from_store` for start, `resume`).
    fn open_session(
        &mut self,
        type_name: &str,
        build: impl FnOnce(Arc<GlobalStore>, Arc<Complex>, String) -> Result<RewriteEngine, String>,
    ) -> String {
        let (store, root_path, source) = match std::mem::replace(&mut self.state, State::Empty) {
            State::Empty => return err_json("no source loaded; call load_source first"),
            State::Loaded { store, root_path, source }
            | State::Active { store, root_path, source, .. }
            | State::ZeroFill { store, root_path, source, .. } => (store, root_path, source),
        };
        self.state = State::Loaded { store: Arc::clone(&store), root_path: root_path.clone(), source: source.clone() };

        let type_complex = match resolve_type(&store, &root_path, type_name) {
            Ok(tc) => tc,
            Err(e) => return err_json(&e),
        };

        match build(Arc::clone(&store), type_complex, root_path.clone()) {
            Ok(engine) => {
                let data = build_response(&engine, false);
                self.state = State::Active { store, root_path, source, engine, fill: None };
                ok_json(data)
            }
            Err(e) => err_json(&e),
        }
    }

    /// Start a rewrite session from an initial diagram (and optional target).
    /// Returns a daemon-protocol JSON response (same shape as `show`).
    pub fn start_session(
        &mut self,
        type_name: &str,
        initial: &str,
        target: Option<String>,
        backward: bool,
    ) -> String {
        self.open_session(type_name, |store, tc, root| {
            RewriteEngine::from_store(store, tc, initial, target.as_deref(), root, type_name.to_string(), backward)
        })
    }

    /// Resume a session from a proof diagram, decomposing it into its steps.
    /// `proof`/`target` are names or expressions; forward starts at `proof.in`,
    /// backward at `proof.out`.
    pub fn resume_session(
        &mut self,
        type_name: &str,
        proof: &str,
        target: Option<String>,
        backward: bool,
    ) -> String {
        self.open_session(type_name, |store, tc, root| {
            RewriteEngine::resume(store, tc, proof, target.as_deref(), root, type_name.to_string(), backward)
        })
    }

    /// Send a daemon-protocol command and return a JSON response.
    ///
    /// Engine-level commands (`step`, `auto`, `random`, `undo`, `undo_to`, `show`,
    /// `history`, `list_rules`, `types`, `type`, `cell`, `store`) are
    /// delegated to [`RewriteEngine::handle`], shared with the daemon.
    ///
    /// `homology` goes through the stored `GlobalStore` directly because it
    /// can be queried without an active session.
    ///
    /// Session-level commands (`init`, `resume`, `shutdown`) are not applicable
    /// in web mode — creation/destruction of the engine is driven by
    /// [`WebRepl::start_session`] / [`WebRepl::resume_session`] + [`WebRepl::reset`].
    pub fn run_command(&mut self, command_json: &str) -> String {
        let request: Request = match serde_json::from_str(command_json) {
            Ok(r) => r,
            Err(e) => return err_json(&format!("invalid command JSON: {e}")),
        };

        match &request {
            Request::Start { .. }
            | Request::Resume { .. }
            | Request::Shutdown => return err_json("command not supported in web mode"),
            Request::Homology { name } => {
                let name = name.clone();
                let Some(store) = self.state.store() else {
                    return err_json("no source loaded");
                };
                return match build_homology_response(store, self.state.root_path(), &name) {
                    Ok(data) => ok_json(data),
                    Err(msg) => err_json(&msg),
                };
            }
            _ => {}
        }

        // Hole-filling commands.  `holes` is a store query; `fill`/`done` drive
        // the fill state machine.
        match &request {
            Request::Holes => {
                let Some(store) = self.state.store() else { return err_json("no source loaded"); };
                return ok_json(holes_json(store, self.state.root_path()));
            }
            Request::Fill { index, backward } => return self.start_fill_session(*index, *backward),
            Request::Done => return self.finalize_fill(),
            _ => {}
        }

        // The 0-cell fill is not an engine — handle its few commands here.
        if matches!(self.state, State::ZeroFill { .. }) {
            return self.handle_zero_fill(&request);
        }

        // Store-level queries: work in both Loaded and Active states.
        // If the engine is active, fall through to the engine dispatch instead.
        if self.state.engine().is_none() {
            let Some(store) = self.state.store() else {
                return err_json("no source loaded");
            };
            match &request {
                Request::Types => {
                    return ok_json(serde_json::json!({
                        "types": build_types_from_store(store, self.state.root_path()),
                    }));
                }
                Request::TypeInfo { name } => {
                    return match build_type_detail_from_store(store, self.state.root_path(), name) {
                        Ok(detail) => ok_json(serde_json::json!({ "type_detail": detail })),
                        Err(msg) => err_json(&msg),
                    };
                }
                _ => {
                    return err_json("no session active — use 'start' to begin");
                }
            }
        }

        // Everything else is engine-level: delegate to the shared dispatcher.
        let State::Active { store, engine, fill, .. } = &mut self.state else {
            return err_json("no session active — use 'start' to begin");
        };
        let fill_marker = fill.as_ref().map(|ctx| fill_info(ctx, engine.initial_diagram().top_dim() + 1));
        match engine.handle(&request) {
            Some(Ok(mut data)) => {
                // `store` updates the engine's store via `Arc::make_mut`; keep
                // the adapter's cached handle in lockstep so subsequent
                // `get_types` and friends see the new let-binding.
                if matches!(request, Request::Store { .. }) {
                    *store = engine.store_arc();
                }
                data.fill = fill_marker;
                ok_json(data)
            }
            Some(Err(msg)) => response_err(msg),
            // Unreachable: the variants `handle` returns `None` for are all
            // matched explicitly above.
            None => err_json("unhandled request"),
        }
    }

    /// Return string diagram data for a named item within a type.
    ///
    /// Tries named diagrams first, then generator classifiers.
    /// Returns `{"status":"ok","data":{...}}` or `{"status":"error","message":"..."}`.
    /// Optional boundary extraction: pass `boundary_dim` and `boundary_sign`
    /// ("input" or "output") to get a boundary diagram instead of the main one.
    pub fn get_strdiag(
        &self,
        type_name: &str,
        item_name: &str,
        boundary_dim: Option<usize>,
        boundary_sign: Option<String>,
    ) -> String {
        let Some(store) = self.state.store() else {
            return err_json("no source loaded; call load_source first");
        };
        let boundary = boundary_dim.map(|d| {
            let sign = boundary_sign.as_deref().unwrap_or("input");
            (d, sign)
        });
        match build_strdiag_response(store, self.state.root_path(), type_name, item_name, boundary) {
            Ok(data) => ok_json(data),
            Err(msg) => err_json(&msg),
        }
    }

    /// Return string diagram data for the image of a domain generator under a map.
    pub fn get_map_image_strdiag(
        &self,
        type_name: &str,
        map_name: &str,
        gen_name: &str,
        boundary_dim: Option<usize>,
        boundary_sign: Option<String>,
    ) -> String {
        let Some(store) = self.state.store() else {
            return err_json("no source loaded; call load_source first");
        };
        let boundary = boundary_dim.map(|d| {
            let sign = boundary_sign.as_deref().unwrap_or("input");
            (d, sign)
        });
        match build_map_image_strdiag(store, self.state.root_path(), type_name, map_name, gen_name, boundary) {
            Ok(data) => ok_json(data),
            Err(msg) => err_json(&msg),
        }
    }

    /// Return the current type list for the accordion (same format as load_source).
    pub fn get_types(&self) -> String {
        let Some(store) = self.state.store() else {
            return err_json("no source loaded");
        };
        ok_json(serde_json::json!({
            "types": type_summaries_json(store.as_ref()),
        }))
    }

    /// Return the string diagram for the current session diagram.
    pub fn get_session_strdiag(&self) -> String {
        self.need_engine(|e| {
            ok_json(strdiag_json_from_diagram(
                e.current_diagram(),
                e.type_complex(),
            ))
        })
    }

    /// Return the string diagram for the session target diagram (if any).
    pub fn get_target_strdiag(&self) -> String {
        self.need_engine(|e| {
            match e.target_diagram() {
                Some(d) => ok_json(strdiag_json_from_diagram(d, e.type_complex())),
                None => err_json("no target set for this session"),
            }
        })
    }

    /// Return the string diagram for the output of rewrite `choice`.
    ///
    /// This is the diagram that would result from applying the given rewrite.
    pub fn get_rewrite_preview_strdiag(&self, choice: usize) -> String {
        self.need_engine(|e| {
            let rewrites = e.rewrites();
            if choice >= rewrites.len() {
                return err_json(&format!("choice {} out of range", choice));
            }
            let step = &rewrites[choice].step;
            match step_output_strdiag_json(step, e.type_complex()) {
                Ok(data) => ok_json(data),
                Err(msg) => err_json(&msg),
            }
        })
    }

    /// Enable or disable proof view (incremental proof caching).
    pub fn set_proof_view(&mut self, on: bool) -> String {
        let Some(engine) = self.state.engine_mut() else {
            return err_json("no session active; call start_session first");
        };
        if on {
            match engine.enable_proof_cache() {
                Ok(()) => ok_json(serde_json::json!({ "proof_view": true })),
                Err(msg) => err_json(&msg),
            }
        } else {
            engine.disable_proof_cache();
            ok_json(serde_json::json!({ "proof_view": false }))
        }
    }

    /// Return the proof string diagram for the current session state.
    ///
    /// Returns the proof StrDiag at dimension `n + 1` (where `n` is the source
    /// dimension), plus an `output_boundary_map` for wire highlighting.
    ///
    /// At step 0 the proof is the n-dimensional source; extracting at `n + 1`
    /// naturally yields 0 nodes and all n-cells as wires.
    pub fn get_proof_strdiag(&mut self) -> String {
        let Some(engine) = self.state.engine_mut() else {
            return err_json("no session active; call start_session first");
        };

        let proof = match engine.proof_diagram() {
            Ok(d) => d,
            Err(msg) => return err_json(&msg),
        };

        let scope = engine.type_complex();
        let step_count = engine.step_count();
        let n = engine.initial_diagram().top_dim();
        let current = engine.current_diagram().clone();

        let sd = StrDiag::from_diagram_at_dim(&proof, scope, n + 1);

        let current_sign = if engine.backward() { Sign::Input } else { Sign::Output };
        let boundary_map = match Diagram::boundary_correspondence(
            current_sign, n, &proof, &current,
        ) {
            Ok(m) => serde_json::json!(m),
            Err(e) => return err_json(&format!("boundary map: {}", e)),
        };

        ok_json(serde_json::json!({
            "strdiag": strdiag_to_json(&sd),
            "step_count": step_count,
            "output_boundary_map": boundary_map,
        }))
    }

    /// Start a hole-filling session for the 0-based hole `index`.  Requires the
    /// loaded (no-session) state; installs a rewrite engine (m ≥ 1) or a 0-cell
    /// chooser (m = 0).
    fn start_fill_session(&mut self, index: usize, backward: bool) -> String {
        let (store, root_path, source) = match std::mem::replace(&mut self.state, State::Empty) {
            State::Loaded { store, root_path, source } => (store, root_path, source),
            other => { self.state = other; return err_json("session already active; stop it first"); }
        };
        match start_fill(&store, &root_path, &root_path, index, backward) {
            Ok((ctx, FillSession::Rewrite(engine))) => {
                let mut data = build_response(&engine, false);
                data.fill = Some(fill_info(&ctx, ctx.dim));
                self.state = State::Active { store, root_path, source, engine, fill: Some(ctx) };
                ok_json(data)
            }
            Ok((ctx, FillSession::ZeroCell(fill))) => {
                let payload = zero_fill_response(&ctx, &fill);
                self.state = State::ZeroFill { store, root_path, source, ctx, fill };
                payload
            }
            Err(e) => { self.state = State::Loaded { store, root_path, source }; err_json(&e) }
        }
    }

    /// Handle a command while a 0-cell fill is active.  Choosing/undoing/redoing
    /// mirror a rewrite session's `step`/`undo`/`redo`.
    fn handle_zero_fill(&mut self, request: &Request) -> String {
        let State::ZeroFill { ctx, fill, .. } = &mut self.state else {
            return err_json("no active fill");
        };
        let result = match request {
            Request::Show => Ok(()),
            Request::Step { choice } => fill.choose(*choice),
            Request::Undo | Request::UndoTo { .. } => fill.undo(),
            Request::Redo | Request::RedoTo { .. } => fill.redo(),
            _ => return err_json("in a 0-cell fill use 'step', 'undo', 'redo', or 'done'"),
        };
        match result {
            Ok(()) => zero_fill_response(ctx, fill),
            Err(e) => err_json(&e),
        }
    }

    /// Finalise the active fill: build the filler, extend the map's definition,
    /// re-evaluate.  On success the session ends and the updated source is
    /// returned; on an inconsistent fill the session is left intact to retry.
    fn finalize_fill(&mut self) -> String {
        // Build the filler diagram first, borrowing the state read-only so a
        // failure leaves the session untouched.
        let filler = match &self.state {
            State::Active { engine, fill: Some(_), .. } => {
                if !engine.target_reached() { return err_json("target not reached yet"); }
                match engine.assemble_proof() {
                    Ok(d) => d,
                    Err(e) => return err_json(&e),
                }
            }
            State::ZeroFill { fill, .. } => match fill.filler() {
                Ok(d) => d,
                Err(e) => return err_json(&e),
            },
            _ => return err_json("no active fill"),
        };

        let (store, root_path, source, ctx) = match &self.state {
            State::Active { store, root_path, source, fill: Some(ctx), .. }
            | State::ZeroFill { store, root_path, source, ctx, .. } => (store, root_path, source, ctx),
            _ => return err_json("no active fill"),
        };
        // Compose the report (same wording as the CLI) before the store changes.
        let message = filled_report(store, ctx, &filler);
        let root_path_owned = root_path.clone();
        let new_source = match edit_for_fill(store, ctx, &filler, source) {
            Ok(s) => s,
            Err(e) => return err_json(&e),
        };

        // Re-evaluate with the same virtual modules.  An inconsistent fill makes
        // the interpreter error; we report it and keep the session to retry.
        match self.reevaluate(&root_path_owned, &new_source) {
            Ok(new_store) => {
                let types = type_summaries_json(&new_store);
                let payload = ok_json(serde_json::json!({
                    "message": message,
                    "source": new_source.clone(),
                    "types": types,
                }));
                self.state = State::Loaded { store: new_store, root_path: root_path_owned, source: new_source };
                payload
            }
            Err(e) => err_json(&e),
        }
    }

    /// Re-interpret the edited root source with the retained virtual modules.
    fn reevaluate(&self, root_path: &str, new_source: &str) -> Result<Arc<GlobalStore>, String> {
        let mut files = self.modules.clone();
        files.insert(root_path.to_string(), new_source.to_string());
        let loader = Loader::with_virtual_files(files);
        match InterpretedFile::load(&loader, root_path) {
            LoadResult::Loaded(file) => Ok(Arc::clone(&file.state)),
            LoadResult::LoadError(e) => Err(load_error_message(&e)),
            LoadResult::InterpError { errors, source, path } => {
                let msgs: Vec<String> = errors
                    .iter()
                    .map(|e| e.to_diagnostic(&source, Some(path.clone())).message)
                    .collect();
                Err(msgs.join("; "))
            }
        }
    }

    fn need_engine<F: FnOnce(&RewriteEngine) -> String>(&self, f: F) -> String {
        match self.state.engine() {
            Some(e) => f(e),
            None => err_json("no session active; call start_session first"),
        }
    }
}

fn face_tags_json(store: &GlobalStore, tc: &crate::core::complex::Complex, tag: &Tag) -> Vec<serde_json::Value> {
    let Some(data) = store.cell_data_for_tag(tc, tag) else { return Vec::new() };
    let CellData::Boundary { boundary_in, boundary_out } = &data else { return Vec::new() };
    let mut face_tags = Vec::new();
    for bd in [boundary_in, boundary_out] {
        if let Some(labels) = bd.labels_at(bd.top_dim()) {
            for t in labels {
                face_tags.push(tag_to_json(t));
            }
        }
    }
    face_tags
}

fn compute_thin_tags(
    tc: &Complex,
) -> Vec<Tag> {
    let Some(values) = tc.find_index("thin") else { return Vec::new() };
    let mut tags = Vec::new();
    for name in values {
        if let Some((tag, _)) = tc.find_generator(name) {
            tags.push(tag.clone());
        } else if let Some(diag) = tc.find_diagram(name) {
            if let Some(tag) = diag.top_label() {
                tags.push(tag.clone());
            }
        }
    }
    tags
}

fn propagate_thin_through_maps(
    store: &GlobalStore,
    tc: &Complex,
    known_thin: &HashSet<Tag>,
) -> Vec<Tag> {
    let mut tags = Vec::new();
    for (_, pmap, domain) in tc.maps_iter() {
        let Some(dc) = resolve_domain_complex(store, domain) else { continue };
        for (_, gen_tag, _) in dc.generators_iter() {
            if known_thin.contains(gen_tag) {
                if let Ok(image) = pmap.image(gen_tag) {
                    if image.is_cell() {
                        if let Some(tag) = image.top_label() {
                            tags.push(tag.clone());
                        }
                    }
                }
            }
        }
    }
    tags
}

fn type_summaries_json(store: &GlobalStore) -> Vec<serde_json::Value> {
    let norm = store.normalize();
    let type_complexes: HashMap<&str, &crate::core::complex::Complex> = store
        .modules_iter()
        .flat_map(|(_, mc)| {
            mc.generators_iter().filter_map(move |(_, gen_tag, _)| {
                let Tag::Global(gid) = gen_tag else { return None };
                let te = store.find_type(*gid)?;
                let name = mc.find_generator_by_tag(gen_tag)?;
                Some((name.as_str(), &*te.complex))
            })
        })
        .collect();
    let types_with_modules: Vec<_> = norm.modules
        .iter()
        .flat_map(|m| {
            let module_name = m.path.strip_suffix(".ali").unwrap_or(&m.path);
            let module_path = m.path.as_str();
            m.types.iter().map(move |t| (module_name, module_path, t))
        })
        .filter(|(_, _, t)| !t.name.is_empty())
        .collect();
    let mut known_thin: HashSet<Tag> = HashSet::new();
    let mut result = Vec::new();
    for (module_name, module_path, t) in types_with_modules {
        let tc = type_complexes.get(t.name.as_str());
        let generators: Vec<serde_json::Value> = t
            .dims
            .iter()
            .flat_map(|d| {
                let tc = tc.copied();
                d.cells.iter().map(move |c| {
                    let (tag_json, faces_json) = tc
                        .and_then(|tc| {
                            let (tag, _) = tc.find_generator(&c.name)?;
                            Some((tag_to_json(tag), face_tags_json(store, tc, tag)))
                        })
                        .unwrap_or((serde_json::Value::Null, Vec::new()));
                    serde_json::json!({
                        "name": c.name,
                        "dim": d.dim,
                        "input": c.input,
                        "output": c.output,
                        "tag": tag_json,
                        "face_tags": faces_json,
                    })
                })
            })
            .collect();
        let diagrams: Vec<serde_json::Value> = t
            .diagrams
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "input": c.input,
                    "output": c.output,
                })
            })
            .collect();
        let module_complex = store.find_module(module_path);
        let maps = tc
            .and_then(|tc| module_complex.map(|mc| build_map_entries(tc, mc, store)))
            .unwrap_or_default();
        let mut new_thin: Vec<Tag> = tc
            .map(|tc| compute_thin_tags(tc))
            .unwrap_or_default();
        known_thin.extend(new_thin.iter().cloned());
        let propagated = tc
            .map(|tc| propagate_thin_through_maps(store, tc, &known_thin))
            .unwrap_or_default();
        new_thin.extend(propagated.iter().cloned());
        known_thin.extend(propagated);
        let thin_tags: Vec<serde_json::Value> = new_thin.iter().map(tag_to_json).collect();
        result.push(serde_json::json!({
            "name": t.name,
            "module": module_name,
            "generators": generators,
            "diagrams": diagrams,
            "maps": maps,
            "thin_tags": thin_tags,
        }));
    }
    result
}

fn load_error_message(error: &LoadFileError) -> String {
    match error {
        LoadFileError::Load { path, cause } => format!("could not load '{}': {:?}", path, cause),
        LoadFileError::Parse { path, errors, .. } => {
            let msgs: Vec<String> = errors.iter().map(|e| e.message().to_string()).collect();
            format!("parse error in '{}': {}", path, msgs.join("; "))
        }
        LoadFileError::ModuleNotFound { module_name } => {
            format!("module '{}' not found", module_name)
        }
        LoadFileError::ModuleIoError { path, reason } => {
            format!("could not load '{}': {}", path, reason)
        }
        LoadFileError::Cycle { path } => format!("cyclic dependency involving '{}'", path),
    }
}

fn load_error_json(error: &LoadFileError) -> String {
    let message = load_error_message(error);
    if let LoadFileError::Parse { path, source, errors } = error {
        let diagnostics: Vec<Diagnostic> = errors
            .iter()
            .map(|e| e.to_diagnostic(source, Some(path.clone())))
            .collect();
        return serde_json::json!({
            "status": "error",
            "message": message,
            "diagnostics": diagnostics,
        })
        .to_string();
    }
    err_json(&message)
}

fn diagnostics_err_json(diagnostics: &[Diagnostic]) -> String {
    let summary: Vec<String> = diagnostics
        .iter()
        .map(|d| {
            format!(
                "{} error at line {}:{} — {}",
                d.kind, d.start.line, d.start.col, d.message
            )
        })
        .collect();
    serde_json::json!({
        "status": "error",
        "message": summary.join("\n"),
        "diagnostics": diagnostics,
    })
    .to_string()
}

/// JSON for the `holes` response: the numbered open holes of the module.
fn holes_json(store: &GlobalStore, root_path: &str) -> serde_json::Value {
    let holes: Vec<serde_json::Value> = list_open_holes(store, root_path)
        .iter()
        .map(|h| serde_json::json!({
            "index": h.index,
            "type_name": h.type_name,
            "map_name": h.map_name,
            "domain_name": h.domain_name,
            "source_name": h.source_name,
            "dim": h.dim,
            "boundary": h.boundary,
        }))
        .collect();
    serde_json::json!({ "holes": holes })
}

fn fill_info(ctx: &FillContext, dim: usize) -> FillInfo {
    FillInfo {
        type_name: ctx.type_name.clone(),
        map_name: ctx.map_name.clone(),
        domain_name: ctx.domain_name.clone(),
        source_name: ctx.source_name.clone(),
        dim,
    }
}

/// The response for a 0-cell fill: session-like fields plus the candidate 0-cells
/// (a clickable list, offered only while unchosen — there is never a step diagram).
fn zero_fill_response(ctx: &FillContext, fill: &ZeroCellFill) -> String {
    // Candidates are offered only while no cell is chosen — once chosen, the
    // session is "at the target" and (like a normal session) has no moves.
    let candidates: Vec<serde_json::Value> = if fill.chosen.is_some() {
        Vec::new()
    } else {
        fill.choices.iter().enumerate()
            .map(|(i, (_, name))| serde_json::json!({ "index": i, "name": name }))
            .collect()
    };
    ok_json(serde_json::json!({
        "fill": fill_info(ctx, 0),
        "zero_cell": {
            "choices": candidates,
            "chosen": fill.chosen_name(),
            "target_reached": fill.target_reached(),
            "can_undo": fill.chosen.is_some(),
            "can_redo": fill.can_redo(),
        },
    }))
}

fn err_json(msg: &str) -> String {
    serde_json::json!({ "status": "error", "message": msg }).to_string()
}

fn ok_json(data: impl Serialize) -> String {
    serde_json::json!({ "status": "ok", "data": data }).to_string()
}

fn response_err(msg: String) -> String {
    serde_json::to_string(&Response::error(msg)).unwrap()
}
