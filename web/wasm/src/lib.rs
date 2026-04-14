// WebAssembly bindings for the alifib rewrite engine.
//
// Exposes a stateful `WasmRepl` class that mirrors the `alifib serve` daemon
// protocol over direct function calls rather than JSON-lines on stdin/stdout.
//
// Build with:
//   wasm-pack build --target web web/wasm --out-dir ../pkg

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use alifib::aux::loader::Loader;
use alifib::interactive::engine::{RewriteEngine, resolve_type};
use alifib::interactive::protocol::{
    build_cell_response, build_list_rules_response, build_response,
    build_type_info_response, build_types_response, Request, Response,
};
use alifib::interpreter::{GlobalStore, InterpretedFile, LoadResult};

const SOURCE_PATH: &str = "source.ali";

// ── helpers ──────────────────────────────────────────────────────────────────

fn err_json(msg: &str) -> String {
    serde_json::json!({ "status": "error", "message": msg }).to_string()
}

fn ok_json(data: impl Serialize) -> String {
    serde_json::json!({ "status": "ok", "data": data }).to_string()
}

// ── WasmRepl ─────────────────────────────────────────────────────────────────

/// Stateful REPL wrapper for use from JavaScript.
///
/// Lifecycle:
/// 1. `new()` — create an empty instance
/// 2. `load_source(text)` — parse and interpret `.ali` source text
/// 3. `init_session(type, src, tgt?)` — start a rewrite session on a type
/// 4. `run_command(json)` — send daemon-protocol commands (step/undo/show/…)
#[wasm_bindgen]
pub struct WasmRepl {
    store: Option<Arc<GlobalStore>>,
    engine: Option<RewriteEngine>,
}

#[wasm_bindgen]
impl WasmRepl {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmRepl {
        WasmRepl { store: None, engine: None }
    }

    /// Interpret `.ali` source text and return a JSON response with structured
    /// type data (generators with boundaries, diagrams, maps).
    pub fn load_source(&mut self, source: &str) -> String {
        let mut files = HashMap::new();
        files.insert(SOURCE_PATH.to_string(), source.to_string());
        let loader = Loader::with_virtual_files(files);

        match InterpretedFile::load(&loader, SOURCE_PATH) {
            LoadResult::Loaded(file) => {
                self.store = Some(Arc::clone(&file.state));
                self.engine = None;

                let norm = file.state.normalize();
                let types: Vec<serde_json::Value> = norm
                    .modules
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
                    .collect();

                serde_json::json!({
                    "status": "ok",
                    "types": types
                })
                .to_string()
            }
            LoadResult::LoadError(e) => err_json(&format!("{e:?}")),
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
        let store = match self.store.clone() {
            Some(s) => s,
            None => return err_json("no source loaded; call load_source first"),
        };

        let type_complex = match resolve_type(&store, SOURCE_PATH, type_name) {
            Ok(tc) => tc,
            Err(e) => return err_json(&e),
        };

        match RewriteEngine::from_store(
            store,
            type_complex,
            source_diagram,
            target_diagram.as_deref(),
            SOURCE_PATH.to_string(),
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

            Request::Undo => self.need_engine_mut(|e| match e.undo() {
                Ok(()) => ok_json(build_response(e, false)),
                Err(msg) => response_err(msg),
            }),

            Request::UndoTo { step } => self.need_engine_mut(|e| match e.undo_to(step) {
                Ok(()) => ok_json(build_response(e, false)),
                Err(msg) => response_err(msg),
            }),

            Request::ListRules => {
                self.need_engine(|e| ok_json(build_list_rules_response(e)))
            }

            Request::History => {
                self.need_engine(|e| ok_json(build_response(e, true)))
            }

            Request::Types => {
                self.need_engine(|e| ok_json(build_types_response(e)))
            }

            Request::TypeInfo { name } => self.need_engine(|e| {
                match build_type_info_response(e, &name) {
                    Ok(data) => ok_json(data),
                    Err(msg) => response_err(msg),
                }
            }),

            Request::Cell { name } => self.need_engine(|e| {
                match build_cell_response(e, &name) {
                    Ok(data) => ok_json(data),
                    Err(msg) => response_err(msg),
                }
            }),

            Request::Store { name } => {
                match self.engine.as_mut() {
                    None => err_json("no session active; call init_session first"),
                    Some(e) => match e.register_proof(&name) {
                        Ok(_) => ok_json(build_response(e, false)),
                        Err(msg) => response_err(msg),
                    },
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
    /// Return string diagram data for a named item within a type.
    ///
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
        match alifib::interactive::protocol::build_strdiag_response(
            store, SOURCE_PATH, type_name, item_name, boundary,
        ) {
            Ok(data) => ok_json(data),
            Err(msg) => err_json(&msg),
        }
    }

    // ── helpers ───────────────────────────────────────────────────────────────

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

fn response_err(msg: String) -> String {
    serde_json::to_string(&Response::error(msg)).unwrap()
}
