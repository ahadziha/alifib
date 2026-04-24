// WebAssembly bindings for the shared browser-facing Alifib web API.
//
// Build with:
//   wasm-pack build --target web web/wasm --out-dir ../pkg

use alifib::interactive::web::WebRepl;
use wasm_bindgen::prelude::*;

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
    inner: WebRepl,
}

#[wasm_bindgen]
impl WasmRepl {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmRepl {
        WasmRepl {
            inner: WebRepl::new(),
        }
    }

    /// Drop all interpreter state, freeing the `GlobalStore` and any active
    /// session.  In WASM the freed pages stay in linear memory but become
    /// available for reuse by subsequent `load_source` calls.
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    /// Interpret `.ali` source text and return a JSON response with structured
    /// type data (generators with boundaries, diagrams, maps).
    ///
    /// `modules_json` is an optional `{ "<Name>": "<contents>", ... }` object
    /// serialised as JSON.  The frontend populates it from the `.ali` files
    /// it has fetched over HTTP, so `include <Name>` resolves without any
    /// server-side file access.  Pass `null` or an empty object when no
    /// extra modules are needed.
    pub fn load_source(&mut self, source: &str, modules_json: Option<String>) -> String {
        let modules: std::collections::HashMap<String, String> = modules_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        self.inner.load_source_with_modules(source, modules)
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
        self.inner
            .init_session(type_name, source_diagram, target_diagram)
    }

    /// Send a daemon-protocol command and return a JSON response.
    ///
    /// Supported commands: `show`, `step`, `undo`, `undo_to`, `list_rules`,
    /// `history`, `types`, `type`, `cell`, `store`.
    ///
    /// Not supported (file-system commands): `init`, `resume`, `save`, `shutdown`.
    pub fn run_command(&mut self, command_json: &str) -> String {
        self.inner.run_command(command_json)
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
        self.inner
            .get_strdiag(type_name, item_name, boundary_dim, boundary_sign)
    }

    /// Return the current type list for the accordion (same format as load_source).
    pub fn get_types(&self) -> String {
        self.inner.get_types()
    }

    /// Return the string diagram for the current session diagram.
    pub fn get_session_strdiag(&self) -> String {
        self.inner.get_session_strdiag()
    }

    /// Return the string diagram for the target of rewrite `choice`.
    ///
    /// This is the diagram that would result from applying the given rewrite.
    pub fn get_rewrite_preview_strdiag(&self, choice: usize) -> String {
        self.inner.get_rewrite_preview_strdiag(choice)
    }
}
