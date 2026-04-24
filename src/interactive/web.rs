//! Shared browser-facing API for the web GUI.
//!
//! This mirrors the WASM surface used by `web/frontend/app.js`, but lives in
//! the main crate so it can be reused by both the WASM bindings and a future
//! localhost HTTP server.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use crate::aux::loader::{LoadFileError, Loader};
use crate::interpreter::{GlobalStore, InterpretedFile, LoadResult};

use super::engine::{RewriteEngine, resolve_type};
use super::protocol::{
    Request, Response, build_cell_response, build_homology_response, build_list_rules_response,
    build_response, build_strdiag_response, build_type_info_response, build_types_response,
    step_target_strdiag_json, strdiag_json_from_diagram,
};

pub const WEB_SOURCE_PATH: &str = "source.ali";

/// Stateful REPL wrapper shared by the browser frontends.
///
/// Lifecycle:
/// 1. `new()` — create an empty instance
/// 2. `load_source(text)` — parse and interpret `.ali` source text
/// 3. `init_session(type, src, tgt?)` — start a rewrite session on a type
/// 4. `run_command(json)` — send daemon-protocol commands (step/undo/show/…)
pub struct WebRepl {
    store: Option<Arc<GlobalStore>>,
    engine: Option<RewriteEngine>,
}

impl Default for WebRepl {
    fn default() -> Self {
        Self::new()
    }
}

impl WebRepl {
    pub fn new() -> Self {
        Self {
            store: None,
            engine: None,
        }
    }

    pub fn reset(&mut self) {
        self.engine = None;
        self.store = None;
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
        self.engine = None;
        self.store = None;

        let mut files = extra_modules;
        files.insert(WEB_SOURCE_PATH.to_string(), source.to_string());
        let loader = Loader::with_virtual_files(files);

        match InterpretedFile::load(&loader, WEB_SOURCE_PATH) {
            LoadResult::Loaded(file) => {
                self.store = Some(Arc::clone(&file.state));
                // Preserve the original frontend contract for `load_source`,
                // which returns `types` at the top level instead of under
                // `data`.
                serde_json::json!({
                    "status": "ok",
                    "types": type_summaries_json(&file.state),
                })
                .to_string()
            }
            LoadResult::LoadError(e) => err_json(&load_error_message(&e)),
            LoadResult::InterpError { errors, .. } => {
                let msgs: Vec<String> = errors.iter().map(|e| format!("{e}")).collect();
                err_json(&msgs.join("\n"))
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
        self.engine = None;

        let store = match self.store.clone() {
            Some(s) => s,
            None => return err_json("no source loaded; call load_source first"),
        };

        let type_complex = match resolve_type(&store, WEB_SOURCE_PATH, type_name) {
            Ok(tc) => tc,
            Err(e) => return err_json(&e),
        };

        match RewriteEngine::from_store(
            store,
            type_complex,
            source_diagram,
            target_diagram.as_deref(),
            WEB_SOURCE_PATH.to_string(),
            type_name.to_string(),
        ) {
            Ok(engine) => {
                let data = build_response(&engine, false);
                self.engine = Some(engine);
                ok_json(data)
            }
            Err(e) => err_json(&e),
        }
    }

    /// Send a daemon-protocol command and return a JSON response.
    ///
    /// Supported commands: `show`, `step`, `undo`, `undo_to`, `list_rules`,
    /// `history`, `types`, `type`, `cell`, `store`.
    ///
    /// Not supported (file-system commands): `init`, `resume`, `save`, `shutdown`.
    pub fn run_command(&mut self, command_json: &str) -> String {
        let request: Request = match serde_json::from_str(command_json) {
            Ok(r) => r,
            Err(e) => return err_json(&format!("invalid command JSON: {e}")),
        };

        match request {
            Request::Show => self.need_engine(|e| ok_json(build_response(e, false))),

            Request::Step { choice } => self.need_engine_mut(|e| match e.step(choice) {
                Ok(_) => ok_json(build_response(e, false)),
                Err(msg) => response_err(msg),
            }),

            Request::Auto { max_steps } => self.need_engine_mut(|e| match e.auto(max_steps) {
                Ok((applied, stop_reason)) => {
                    let data = build_response(e, false);
                    let mut val = serde_json::to_value(&data).unwrap();
                    val.as_object_mut().unwrap().insert(
                        "auto".to_string(),
                        serde_json::json!({
                            "applied": applied,
                            "stop_reason": stop_reason,
                        }),
                    );
                    ok_json(val)
                }
                Err(msg) => response_err(msg),
            }),

            Request::Undo => self.need_engine_mut(|e| match e.undo() {
                Ok(()) => ok_json(build_response(e, false)),
                Err(msg) => response_err(msg),
            }),

            Request::UndoTo { step } => self.need_engine_mut(|e| match e.undo_to(step) {
                Ok(()) => ok_json(build_response(e, false)),
                Err(msg) => response_err(msg),
            }),

            Request::ListRules => self.need_engine(|e| ok_json(build_list_rules_response(e))),

            Request::History => self.need_engine(|e| ok_json(build_response(e, true))),

            Request::Types => self.need_engine(|e| ok_json(build_types_response(e))),

            Request::TypeInfo { name } => {
                self.need_engine(|e| match build_type_info_response(e, &name) {
                    Ok(data) => ok_json(data),
                    Err(msg) => response_err(msg),
                })
            }

            Request::Cell { name } => self.need_engine(|e| match build_cell_response(e, &name) {
                Ok(data) => ok_json(data),
                Err(msg) => response_err(msg),
            }),

            Request::Store { name } => {
                let Some(e) = self.engine.as_mut() else {
                    return err_json("no session active; call init_session first");
                };
                let proof_expr = if e.steps().is_empty() {
                    None
                } else {
                    let n = e.source_diagram().top_dim();
                    let scope = e.type_complex();
                    let mut steps = e.steps().iter();
                    let first = crate::output::render_diagram(steps.next().unwrap(), scope);
                    let rest: String = steps
                        .map(|s| format!("\n#{} {}", n, crate::output::render_diagram(s, scope)))
                        .collect();
                    Some(format!("{}{}", first, rest))
                };
                let type_name = e.type_name().to_owned();
                match e.register_proof(&name) {
                    Ok((new_store, _)) => {
                        self.store = Some(new_store);
                        let data = build_response(e, false);
                        let store_info = proof_expr.map(|expr| {
                            serde_json::json!({
                                "type_name": type_name,
                                "def_name": name,
                                "expr": expr,
                            })
                        });
                        let mut val = serde_json::to_value(&data).unwrap();
                        if let Some(info) = store_info {
                            val.as_object_mut()
                                .unwrap()
                                .insert("stored".to_string(), info);
                        }
                        ok_json(val)
                    }
                    Err(msg) => response_err(msg),
                }
            }

            Request::Homology { name } => {
                let store = match self.store.as_ref() {
                    Some(s) => s,
                    None => return err_json("no source loaded"),
                };
                match build_homology_response(store, WEB_SOURCE_PATH, &name) {
                    Ok(data) => ok_json(data),
                    Err(msg) => err_json(&msg),
                }
            }

            Request::Init { .. }
            | Request::Resume { .. }
            | Request::Save { .. }
            | Request::Shutdown => err_json("command not supported in web mode"),
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
        let store = match self.store.as_ref() {
            Some(s) => s,
            None => return err_json("no source loaded; call load_source first"),
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
        let store = match self.store.as_ref() {
            Some(s) => s,
            None => return err_json("no source loaded"),
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
            let rewrites = e.available_rewrites();
            let Some(m) = rewrites.get(choice) else {
                return err_json(&format!("choice {} out of range", choice));
            };
            match step_target_strdiag_json(&m.step, e.type_complex()) {
                Ok(data) => ok_json(data),
                Err(msg) => err_json(&msg),
            }
        })
    }

    fn need_engine<F: FnOnce(&RewriteEngine) -> String>(&self, f: F) -> String {
        match self.engine.as_ref() {
            Some(e) => f(e),
            None => err_json("no session active; call init_session first"),
        }
    }

    fn need_engine_mut<F: FnOnce(&mut RewriteEngine) -> String>(&mut self, f: F) -> String {
        match self.engine.as_mut() {
            Some(e) => f(e),
            None => err_json("no session active; call init_session first"),
        }
    }
}

fn type_summaries_json(store: &GlobalStore) -> Vec<serde_json::Value> {
    let norm = store.normalize();
    norm.modules
        .iter()
        .flat_map(|m| &m.types)
        .filter(|t| !t.name.is_empty())
        .map(|t| {
            let generators: Vec<serde_json::Value> = t
                .dims
                .iter()
                .flat_map(|d| {
                    d.cells.iter().map(move |c| {
                        serde_json::json!({
                            "name": c.name,
                            "dim": d.dim,
                            "src": c.src,
                            "tgt": c.tgt,
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
            serde_json::json!({
                "name": t.name,
                "generators": generators,
                "diagrams": diagrams,
                "maps": maps,
            })
        })
        .collect()
}

fn load_error_message(error: &LoadFileError) -> String {
    match error {
        LoadFileError::Load { path, cause } => format!("could not load '{}': {:?}", path, cause),
        LoadFileError::Parse { path, errors, .. } => {
            let msgs: Vec<String> = errors.iter().map(|e| format!("{e}")).collect();
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

fn err_json(msg: &str) -> String {
    serde_json::json!({ "status": "error", "message": msg }).to_string()
}

fn ok_json(data: impl Serialize) -> String {
    serde_json::json!({ "status": "ok", "data": data }).to_string()
}

fn response_err(msg: String) -> String {
    serde_json::to_string(&Response::error(msg)).unwrap()
}
