//! Shared browser-facing API for the web GUI.
//!
//! `WebRepl` is the stateful adapter shared by the web backends — the HTTP
//! server at `web/server/`, the WASM bindings at `web/wasm/`, and the MCP
//! server at `web/mcp/`.  It is a thin
//! wrapper over the shared [`Session`] (with a virtual-module loader): command
//! dispatch is `session.apply`, the same machine the CLI and stdio daemon use.
//! The only web-specific work is the structured-diagnostics load path, the
//! string-diagram queries (`get_*_strdiag`), and JSON framing.

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

use super::engine::RewriteEngine;
use super::protocol::{
    Request, ResponseData, build_cell_response, build_homology_data, build_map_entries,
    build_map_image_strdiag, build_strdiag_response, build_types_from_store,
    build_type_detail_from_store, resolve_domain_complex, step_output_strdiag_json,
    strdiag_json_from_diagram, strdiag_to_json, tag_to_json,
};
use super::richtext::{help, render_kind_for, render_response, RenderKind};
use super::session::{LoadStrategy, Session};

pub const WEB_SOURCE_PATH: &str = "source.ali";

/// Stateful REPL wrapper shared by the browser frontends.
///
/// Lifecycle: `new()` → `load_source(text)` → `start_session`/`fill` → session
/// commands via `run_command`.  All command behaviour lives in [`Session`].
pub struct WebRepl {
    session: Option<Session>,
}

impl Default for WebRepl {
    fn default() -> Self {
        Self::new()
    }
}

impl WebRepl {
    pub fn new() -> Self {
        Self { session: None }
    }

    pub fn reset(&mut self) {
        self.session = None;
    }

    /// End the active rewrite session or abandon the active fill (keeps the
    /// loaded source).
    pub fn stop_session(&mut self) {
        if let Some(s) = self.session.as_mut() {
            let _ = s.apply(Request::Stop);
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
    ///
    /// `source_name` optionally overrides the virtual filename for the root
    /// source (e.g. `"Monoidal"` → `"Monoidal.ali"`).
    pub fn load_source_with_modules(
        &mut self,
        source: &str,
        extra_modules: HashMap<String, String>,
        source_name: Option<&str>,
    ) -> String {
        // Free old state before allocating the new store so both don't coexist —
        // in WASM, linear-memory pages from the peak are permanent.
        self.session = None;

        let root_path = source_name
            .filter(|n| !n.is_empty())
            .map(|n| if n.ends_with(".ali") { n.to_string() } else { format!("{}.ali", n) })
            .unwrap_or_else(|| WEB_SOURCE_PATH.to_string());

        let mut files = extra_modules.clone();
        files.insert(root_path.clone(), source.to_string());
        let loader = Loader::with_virtual_files(files);

        // The web does its own load (rather than `Session::from_virtual`) so it
        // can surface structured diagnostics for the editor.
        match InterpretedFile::load(&loader, &root_path) {
            LoadResult::Loaded(file) => {
                let store = Arc::clone(&file.state);
                // Preserve the frontend contract: `types` at the top level.
                let json = serde_json::json!({
                    "status": "ok",
                    "types": type_summaries_json(&store),
                })
                .to_string();
                self.session = Some(Session::from_loaded(
                    store, root_path, source.to_string(), LoadStrategy::Virtual(extra_modules),
                ));
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

    /// Start a rewrite session from an initial diagram (and optional target).
    pub fn start_session(
        &mut self,
        type_name: &str,
        initial: &str,
        target: Option<String>,
        backward: bool,
    ) -> String {
        let Some(s) = self.session.as_mut() else {
            return err_json("no source loaded; call load_source first");
        };
        let req = Request::Start {
            source_file: s.root_path().to_owned(),
            type_name: type_name.to_owned(),
            initial: initial.to_owned(),
            target,
            backward,
        };
        apply_json(s, req)
    }

    /// Resume a session from a proof diagram, decomposing it into its steps.
    pub fn resume_session(
        &mut self,
        type_name: &str,
        proof: &str,
        target: Option<String>,
        backward: bool,
    ) -> String {
        let Some(s) = self.session.as_mut() else {
            return err_json("no source loaded; call load_source first");
        };
        let req = Request::Resume {
            source_file: s.root_path().to_owned(),
            type_name: type_name.to_owned(),
            proof: proof.to_owned(),
            target,
            backward,
        };
        apply_json(s, req)
    }

    /// Parse one typed REPL line with the **shared** parser (the same one the CLI
    /// uses), classifying it for the web front-end.  Returns one of:
    ///
    /// - `{"status":"error","message":…}` — an unknown command or a `Usage:` line,
    ///   worded identically to the CLI;
    /// - `{"status":"action","action":…,…}` — a command the web drives as a UI
    ///   flow (`start`/`resume`/`fill`/`done`/`stop`/`clear`/`holes`/`backward`);
    /// - `{"status":"request","request":{…}}` — a ready [`Request`] the web hands
    ///   to [`run_command`](Self::run_command) (it keys any follow-up on the
    ///   request's own `command` tag).
    ///
    /// No execution happens here — the front-end decides what to do with the
    /// classification, but *parsing and its errors* live in one place.
    pub fn parse_command(&self, line: &str) -> String {
        use crate::interactive::command::{parse, Command, Frontend};

        let cmd = match parse(line, Frontend::Web) {
            Ok(c) => c,
            Err(message) => return serde_json::json!({ "status": "error", "message": message }).to_string(),
        };
        let action = |a: &str, args: serde_json::Value| {
            let mut o = serde_json::Map::new();
            o.insert("status".into(), "action".into());
            o.insert("action".into(), a.into());
            if let serde_json::Value::Object(m) = args { o.extend(m); }
            serde_json::Value::Object(o).to_string()
        };
        let request = |req: Request| serde_json::json!({ "status": "request", "request": req }).to_string();
        match cmd {
            // Commands the web drives as a UI flow rather than a plain request.
            Command::Start { type_name, initial, target } =>
                action("start", serde_json::json!({ "type_name": type_name, "initial": initial, "target": target })),
            Command::Resume { type_name, proof, target } =>
                action("resume", serde_json::json!({ "type_name": type_name, "proof": proof, "target": target })),
            Command::Fill(index) => action("fill", serde_json::json!({ "index": index })),
            Command::Done => action("done", serde_json::json!({})),
            Command::Stop => action("stop", serde_json::json!({})),
            Command::Clear => action("clear", serde_json::json!({})),
            Command::Holes => action("holes", serde_json::json!({})),
            Command::Backward(on) => action("backward", serde_json::json!({ "on": on })),
            Command::Help => request(Request::Help { web: true }),
            // Everything else is a plain backend request via the shared mapping.
            other => match other.to_request(false) {
                Some(req) => request(req),
                None => serde_json::json!({ "status": "error", "message": "command not available in the web REPL" }).to_string(),
            },
        }
    }

    /// Send a daemon-protocol command and return a JSON response.
    ///
    /// Session commands go to [`Session::apply`]; read-only queries are served
    /// from the loaded store; `start`/`resume`/`load`/`shutdown` are driven by
    /// the dedicated methods instead.
    pub fn run_command(&mut self, command_json: &str) -> String {
        let request: Request = match serde_json::from_str(command_json) {
            Ok(r) => r,
            Err(e) => return err_json(&format!("invalid command JSON: {e}")),
        };
        // `help` needs no loaded source — it is the same table the CLI prints,
        // minus the CLI-only commands.
        if let Request::Help { web } = request {
            return serde_json::json!({
                "status": "ok",
                "data": ResponseData::empty(),
                "rendered": help(web),
            }).to_string();
        }
        let Some(s) = self.session.as_mut() else {
            return err_json("no source loaded");
        };
        match request {
            Request::Start { .. } | Request::Resume { .. } | Request::Load { .. } | Request::Shutdown =>
                err_json("command not supported in web mode"),
            Request::Types => {
                let mut data = ResponseData::empty();
                data.types = build_types_from_store(s.store(), s.root_path());
                ok_rendered(&data, Some(RenderKind::Types))
            }
            Request::TypeInfo { name } => match build_type_detail_from_store(s.store(), s.root_path(), &name) {
                Ok(detail) => {
                    let mut data = ResponseData::empty();
                    data.type_detail = Some(detail);
                    ok_rendered(&data, Some(RenderKind::TypeDetail))
                }
                Err(msg) => err_json(&msg),
            },
            Request::Cell { name } => match s.active_engine() {
                Some(e) => match build_cell_response(e, &name) {
                    Ok(data) => ok_json(data),
                    Err(msg) => err_json(&msg),
                },
                None => err_json("no session active — 'cell' needs a session for its type context"),
            },
            Request::Homology { name } => match build_homology_data(s.store(), s.root_path(), &name) {
                Ok(h) => {
                    let mut data = ResponseData::empty();
                    data.homology = Some(h);
                    ok_rendered(&data, Some(RenderKind::Homology))
                }
                Err(msg) => err_json(&msg),
            },
            req => apply_json(s, req),
        }
    }

    /// Return string diagram data for a named item within a type.
    pub fn get_strdiag(
        &self,
        type_name: &str,
        item_name: &str,
        boundary_dim: Option<usize>,
        boundary_sign: Option<String>,
    ) -> String {
        let Some(s) = self.session.as_ref() else {
            return err_json("no source loaded; call load_source first");
        };
        let boundary = boundary_dim.map(|d| (d, boundary_sign.as_deref().unwrap_or("input")));
        match build_strdiag_response(s.store(), s.root_path(), type_name, item_name, boundary) {
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
        let Some(s) = self.session.as_ref() else {
            return err_json("no source loaded; call load_source first");
        };
        let boundary = boundary_dim.map(|d| (d, boundary_sign.as_deref().unwrap_or("input")));
        match build_map_image_strdiag(s.store(), s.root_path(), type_name, map_name, gen_name, boundary) {
            Ok(data) => ok_json(data),
            Err(msg) => err_json(&msg),
        }
    }

    /// Return the current type list for the accordion (same format as load_source).
    pub fn get_types(&self) -> String {
        let Some(s) = self.session.as_ref() else {
            return err_json("no source loaded");
        };
        ok_json(serde_json::json!({ "types": type_summaries_json(s.store()) }))
    }

    /// Return the string diagram for the current session diagram.
    pub fn get_session_strdiag(&self) -> String {
        self.need_engine(|e| ok_json(strdiag_json_from_diagram(e.current_diagram(), e.type_complex())))
    }

    /// Return the string diagram for the session target diagram (if any).
    pub fn get_target_strdiag(&self) -> String {
        self.need_engine(|e| match e.target_diagram() {
            Some(d) => ok_json(strdiag_json_from_diagram(d, e.type_complex())),
            None => err_json("no target set for this session"),
        })
    }

    /// Return the string diagram for the output of rewrite `choice`.
    pub fn get_rewrite_preview_strdiag(&self, choice: usize) -> String {
        self.need_engine(|e| {
            let rewrites = e.rewrites();
            if choice >= rewrites.len() {
                return err_json(&format!("choice {} out of range", choice));
            }
            match step_output_strdiag_json(&rewrites[choice].step, e.type_complex()) {
                Ok(data) => ok_json(data),
                Err(msg) => err_json(&msg),
            }
        })
    }

    /// Enable or disable proof view (incremental proof caching).
    pub fn set_proof_view(&mut self, on: bool) -> String {
        let Some(engine) = self.session.as_mut().and_then(|s| s.active_engine_mut()) else {
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
    pub fn get_proof_strdiag(&mut self) -> String {
        let Some(engine) = self.session.as_mut().and_then(|s| s.active_engine_mut()) else {
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
        let boundary_map = match Diagram::boundary_correspondence(current_sign, n, &proof, &current) {
            Ok(m) => serde_json::json!(m),
            Err(e) => return err_json(&format!("boundary map: {}", e)),
        };

        ok_json(serde_json::json!({
            "strdiag": strdiag_to_json(&sd),
            "step_count": step_count,
            "output_boundary_map": boundary_map,
        }))
    }

    fn need_engine<F: FnOnce(&RewriteEngine) -> String>(&self, f: F) -> String {
        match self.session.as_ref().and_then(|s| s.active_engine()) {
            Some(e) => f(e),
            None => err_json("no session active; call start_session first"),
        }
    }
}

fn apply_json(session: &mut Session, req: Request) -> String {
    let kind = render_kind_for(&req);
    match session.apply(req) {
        Ok(data) => ok_rendered(&data, kind),
        Err(msg) => err_json(&msg),
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

fn err_json(msg: &str) -> String {
    serde_json::json!({ "status": "error", "message": msg }).to_string()
}

fn ok_json(data: impl Serialize) -> String {
    serde_json::json!({ "status": "ok", "data": data }).to_string()
}

/// Like [`ok_json`], but also attaches the shared `RichText` transcript as a
/// sibling `rendered` field when the command has a rendered view, so the
/// frontend styles the same layout the CLI does.  `data` stays the pure payload.
fn ok_rendered(data: &ResponseData, kind: Option<RenderKind>) -> String {
    match kind {
        Some(k) => serde_json::json!({
            "status": "ok",
            "data": data,
            "rendered": render_response(k, data),
        }).to_string(),
        None => ok_json(data),
    }
}
