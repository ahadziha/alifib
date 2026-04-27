//! Shared browser-facing API for the web GUI.
//!
//! `WebRepl` is the stateful adapter used by both web backends — the HTTP
//! server at `web/server/` and the WASM bindings at `web/wasm/`.  Command
//! dispatch delegates to [`RewriteEngine::handle`], which is the same
//! surface the stdio daemon uses at `super::daemon`; the only per-backend
//! work is session setup (`init_session`/`reset`) and the commands that
//! bypass the engine (currently just `homology`, which queries the
//! interpreter's global store directly).

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use crate::aux::Tag;
use crate::aux::loader::{LoadFileError, Loader};
use crate::core::complex::{Complex, MapDomain};
use crate::core::diagram::CellData;
use crate::interpreter::{GlobalStore, InterpretedFile, LoadResult};
use crate::language::error::Diagnostic;

use super::engine::{RewriteEngine, resolve_type};
use super::protocol::{
    Request, Response, build_homology_response, build_response, build_strdiag_response,
    build_types_from_store, build_type_detail_from_store,
    step_target_strdiag_json, strdiag_json_from_diagram, tag_to_json,
};

pub const WEB_SOURCE_PATH: &str = "source.ali";

/// Stateful REPL wrapper shared by the browser frontends.
///
/// Lifecycle:
/// 1. `new()` — create an empty instance
/// 2. `load_source(text)` — parse and interpret `.ali` source text
/// 3. `run_command(json)` — non-session commands (`types`, `type`, `homology`)
///    work immediately after loading
/// 4. `init_session(type, src, tgt?)` — start a rewrite session on a type
/// 5. `run_command(json)` — session commands (step/undo/show/…) plus the above
pub struct WebRepl {
    state: State,
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
    Loaded { store: Arc<GlobalStore> },
    Active { store: Arc<GlobalStore>, engine: RewriteEngine },
}

impl State {
    fn store(&self) -> Option<&Arc<GlobalStore>> {
        match self {
            State::Empty => None,
            State::Loaded { store } | State::Active { store, .. } => Some(store),
        }
    }

    fn engine(&self) -> Option<&RewriteEngine> {
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
        Self { state: State::Empty }
    }

    pub fn reset(&mut self) {
        self.state = State::Empty;
    }

    pub fn stop_session(&mut self) {
        if let State::Active { store, .. } = std::mem::replace(&mut self.state, State::Empty) {
            self.state = State::Loaded { store };
        }
    }

    /// Interpret `.ali` source text and return a JSON response with structured
    /// type data (generators with boundaries, diagrams, maps).
    pub fn load_source(&mut self, source: &str) -> String {
        self.load_source_with_modules(source, HashMap::new())
    }

    /// Like [`load_source`], but seeds additional virtual module files into the
    /// loader.  `extra_modules` is a `<Name>.ali → contents` map — any
    /// `include <Name>` in the root source resolves to the matching entry.
    /// The frontend crates use this to bundle `examples/` as library modules,
    /// keeping the main crate free of web-specific data.
    pub fn load_source_with_modules(
        &mut self,
        source: &str,
        extra_modules: HashMap<String, String>,
    ) -> String {
        // Free old state before allocating the new store so that both don't
        // coexist — in WASM, linear memory pages from the peak are permanent.
        self.state = State::Empty;

        let mut files = extra_modules;
        files.insert(WEB_SOURCE_PATH.to_string(), source.to_string());
        let loader = Loader::with_virtual_files(files);

        match InterpretedFile::load(&loader, WEB_SOURCE_PATH) {
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
                self.state = State::Loaded { store };
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

    /// Start a rewrite session for the named type.
    ///
    /// `source_diagram` — name or expression for the starting diagram.
    /// `target_diagram` — optional goal diagram (name or expression).
    ///
    /// Returns a daemon-protocol JSON response (same shape as `show`).
    pub fn init_session(
        &mut self,
        type_name: &str,
        source_diagram: &str,
        target_diagram: Option<String>,
    ) -> String {
        // Collapse any existing session back to Loaded so a failed init
        // leaves the caller with at least the store they already had.
        let store = match std::mem::replace(&mut self.state, State::Empty) {
            State::Empty => return err_json("no source loaded; call load_source first"),
            State::Loaded { store } | State::Active { store, .. } => store,
        };
        self.state = State::Loaded { store: Arc::clone(&store) };

        let type_complex = match resolve_type(&store, WEB_SOURCE_PATH, type_name) {
            Ok(tc) => tc,
            Err(e) => return err_json(&e),
        };

        match RewriteEngine::from_store(
            Arc::clone(&store),
            type_complex,
            source_diagram,
            target_diagram.as_deref(),
            WEB_SOURCE_PATH.to_string(),
            type_name.to_string(),
        ) {
            Ok(engine) => {
                let data = build_response(&engine, false);
                self.state = State::Active { store, engine };
                ok_json(data)
            }
            Err(e) => err_json(&e),
        }
    }

    /// Send a daemon-protocol command and return a JSON response.
    ///
    /// Engine-level commands (`step`, `auto`, `undo`, `undo_to`, `show`,
    /// `history`, `list_rules`, `types`, `type`, `cell`, `store`) are
    /// delegated to [`RewriteEngine::handle`], shared with the daemon.
    ///
    /// `homology` goes through the stored `GlobalStore` directly because it
    /// can be queried without an active session.
    ///
    /// Session-level commands (`init`, `resume`, `save`, `shutdown`) are
    /// not applicable in web mode — creation/destruction of the engine is
    /// driven by [`WebRepl::init_session`] + [`WebRepl::reset`].
    pub fn run_command(&mut self, command_json: &str) -> String {
        let request: Request = match serde_json::from_str(command_json) {
            Ok(r) => r,
            Err(e) => return err_json(&format!("invalid command JSON: {e}")),
        };

        match &request {
            Request::Init { .. }
            | Request::Resume { .. }
            | Request::Save { .. }
            | Request::Shutdown => return err_json("command not supported in web mode"),
            Request::Homology { name } => {
                let name = name.clone();
                let Some(store) = self.state.store() else {
                    return err_json("no source loaded");
                };
                return match build_homology_response(store, WEB_SOURCE_PATH, &name) {
                    Ok(data) => ok_json(data),
                    Err(msg) => err_json(&msg),
                };
            }
            _ => {}
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
                        "types": build_types_from_store(store, WEB_SOURCE_PATH),
                    }));
                }
                Request::TypeInfo { name } => {
                    return match build_type_detail_from_store(store, WEB_SOURCE_PATH, name) {
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
        let State::Active { store, engine } = &mut self.state else {
            return err_json("no session active — use 'start' to begin");
        };
        match engine.handle(&request) {
            Some(Ok(data)) => {
                // `store` updates the engine's store via `Arc::make_mut`; keep
                // the adapter's cached handle in lockstep so subsequent
                // `get_types` and friends see the new let-binding.
                if matches!(request, Request::Store { .. }) {
                    *store = engine.store_arc();
                }
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
        match build_strdiag_response(store, WEB_SOURCE_PATH, type_name, item_name, boundary) {
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

    /// Return the string diagram for the target of rewrite `choice`.
    ///
    /// This is the diagram that would result from applying the given rewrite.
    pub fn get_rewrite_preview_strdiag(&self, choice: usize) -> String {
        self.need_engine(|e| {
            let rewrites = e.rewrites();
            if choice >= rewrites.len() {
                return err_json(&format!("choice {} out of range", choice));
            }
            let step = &rewrites[choice].step;
            match step_target_strdiag_json(step, e.type_complex()) {
                Ok(data) => ok_json(data),
                Err(msg) => err_json(&msg),
            }
        })
    }

    fn need_engine<F: FnOnce(&RewriteEngine) -> String>(&self, f: F) -> String {
        match self.state.engine() {
            Some(e) => f(e),
            None => err_json("no session active; call init_session first"),
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

fn compute_thin_tags(store: &GlobalStore, tc: &Complex) -> Vec<serde_json::Value> {
    let Some(values) = tc.find_index("thin") else { return Vec::new() };
    let mut tags = Vec::new();
    for name in values {
        if let Some((tag, _)) = tc.find_generator(name) {
            tags.push(tag_to_json(tag));
        } else if let Some(diag) = tc.find_diagram(name) {
            if let Some(tag) = diag.top_label() {
                tags.push(tag_to_json(tag));
            }
        } else if let Some((pmap, domain)) = tc.find_map(name) {
            let dc = match domain {
                MapDomain::Type(gid) => store.find_type(*gid).map(|e| &*e.complex),
                MapDomain::Module(mid) => store.find_module(mid),
            };
            if let Some(dc) = dc {
                for (_, gen_tag, _) in dc.generators_iter() {
                    if let Ok(image) = pmap.image(gen_tag) {
                        if let Some(tag) = image.top_label() {
                            tags.push(tag_to_json(tag));
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
    norm.modules
        .iter()
        .flat_map(|m| {
            let module_name = std::path::Path::new(&m.path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&m.path);
            m.types.iter().map(move |t| (module_name, t))
        })
        .filter(|(_, t)| !t.name.is_empty())
        .map(|(module_name, t)| {
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
                            "src": c.src,
                            "tgt": c.tgt,
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
                        "src": c.src,
                        "tgt": c.tgt,
                    })
                })
                .collect();
            let maps: Vec<serde_json::Value> = t
                .maps
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "name": m.name,
                        "domain": m.domain,
                    })
                })
                .collect();
            let thin_tags: Vec<serde_json::Value> = tc
                .map(|tc| compute_thin_tags(store, tc))
                .unwrap_or_default();
            serde_json::json!({
                "name": t.name,
                "module": module_name,
                "generators": generators,
                "diagrams": diagrams,
                "maps": maps,
                "thin_tags": thin_tags,
            })
        })
        .collect()
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

fn err_json(msg: &str) -> String {
    serde_json::json!({ "status": "error", "message": msg }).to_string()
}

fn ok_json(data: impl Serialize) -> String {
    serde_json::json!({ "status": "ok", "data": data }).to_string()
}

fn response_err(msg: String) -> String {
    serde_json::to_string(&Response::error(msg)).unwrap()
}
