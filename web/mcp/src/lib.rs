//! Model Context Protocol server (stdio JSON-RPC 2.0) wrapping
//! [`alifib::interactive::web::WebRepl`].
//!
//! This is a sibling of `alifib-web-server`: same kernel, different transport.
//! The HTTP server speaks to a browser; this one speaks the MCP wire format
//! that LLM agents (Claude Desktop, Claude Code, Cursor, …) consume.
//!
//! # Wire format
//!
//! Newline-delimited JSON-RPC 2.0 over stdin/stdout, per the MCP spec.  Logs
//! go to stderr — stdout is reserved for protocol traffic.  The handshake is
//! `initialize` (returns server info + capabilities), then any number of
//! `tools/list` and `tools/call` requests.  Notifications have no `id` and
//! are not responded to.
//!
//! # Tool surface
//!
//! Each tool maps 1:1 to a `WebRepl` method.  Successful calls return a
//! single text content block whose body is the same JSON envelope the HTTP
//! API returns; an envelope with `"status":"error"` is forwarded with
//! `isError: true` so MCP clients can branch on it without parsing.  Envelopes
//! are *trimmed* for the wire by [`project_envelope`] — the `rendered`
//! transcript and the heavy boundary-label payloads are dropped unless the call
//! passes `render:true` / `detail:true`.
//!
//! `load_source` (and `load_example`) auto-seed the configured examples
//! directory as virtual `<Name>.ali` modules, so `include` resolves without the
//! agent having to ship example contents itself.  `list_examples` exposes the
//! same set for discovery, and `save_file` writes the running source back to
//! disk — the MCP analog of the browser editor's save.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use alifib::interactive::web::WebRepl;
use alifib_web_shared::ExampleSet;
use serde::Deserialize;
use serde_json::{Map, Value, json};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "alifib-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run the MCP server against stdin/stdout.  Blocks until stdin closes.
pub fn run_mcp_server(examples: ExampleSet) -> Result<(), String> {
    eprintln!("alifib mcp: ready (examples from {})", examples.dir().display());
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(stdin.lock(), stdout.lock(), examples)
}

/// Inner loop — exposed so tests can drive the server with in-memory pipes.
pub fn serve<R: BufRead, W: Write>(
    reader: R,
    mut writer: W,
    examples: ExampleSet,
) -> Result<(), String> {
    let mut repl = WebRepl::new();
    let mut last_loaded_path: Option<String> = None;

    for line in reader.lines() {
        let line = line.map_err(|e| format!("read error: {}", e))?;
        if line.trim().is_empty() {
            continue;
        }

        let msg: JsonRpcMessage = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                // Parse error: id is unknown so per JSON-RPC we send null.
                write_message(
                    &mut writer,
                    &error_response(Value::Null, -32700, &format!("parse error: {e}")),
                )?;
                continue;
            }
        };

        // Notifications have no id and never get a response.
        let Some(id) = msg.id else {
            continue;
        };

        let response = match msg.method.as_deref() {
            Some("initialize") => initialize_response(id),
            Some("tools/list") => tools_list_response(id),
            Some("tools/call") => tools_call_response(
                id,
                msg.params.unwrap_or(Value::Null),
                &mut repl,
                &examples,
                &mut last_loaded_path,
            ),
            Some("ping") => json!({"jsonrpc":"2.0","id":id,"result":{}}),
            Some(other) => error_response(id, -32601, &format!("method not found: {other}")),
            None => error_response(id, -32600, "missing method"),
        };
        write_message(&mut writer, &response)?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct JsonRpcMessage {
    #[allow(dead_code)]
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
}

fn write_message<W: Write>(writer: &mut W, msg: &Value) -> Result<(), String> {
    let line = serde_json::to_string(msg).map_err(|e| e.to_string())?;
    writeln!(writer, "{}", line).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

// ── initialize / tools/list ──────────────────────────────────────────────────

fn initialize_response(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
        },
    })
}

fn tools_list_response(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "tools": tool_descriptors() },
    })
}

/// `render` / `detail` schema fragments — advertised on the read-heavy tools so
/// agents can opt back into the full envelope.  Responses are trimmed by default.
fn render_prop() -> Value {
    json!({ "type": "boolean", "default": false, "description": "Keep the human-readable `rendered` transcript in the response (stripped by default)." })
}

fn detail_prop() -> Value {
    json!({ "type": "boolean", "default": false, "description": "Keep heavy payloads — boundary label strings, tags, cells_by_dim, rewrite diagrams (trimmed by default)." })
}

fn tool_descriptors() -> Vec<Value> {
    vec![
        json!({
            "name": "load_source",
            "description": "Parse and interpret alifib (.ali) source. Modules from the configured examples directory are auto-seeded so `include <Name>` works without shipping content. Returns a JSON envelope with the type list (or diagnostics on parse/type error); the list is trimmed by default — pass detail:true for boundary labels, render:true for the rendered transcript. `path` records where save_file should write; `source_name` sets the virtual root filename (so `include` self-references resolve).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source":  { "type": "string", "description": ".ali source text" },
                    "modules": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Optional extra <Name>.ali → contents overrides. Merged on top of the auto-seeded examples directory.",
                    },
                    "source_name": { "type": "string", "description": "Virtual root filename (<Name> → <Name>.ali); defaults to source.ali." },
                    "path":        { "type": "string", "description": "Disk path remembered as save_file's default target." },
                    "render": render_prop(),
                    "detail": detail_prop(),
                },
                "required": ["source"],
            },
        }),
        json!({
            "name": "load_example",
            "description": "Load a bundled example by name (as listed by list_examples). Seeds all examples as virtual modules, loads the named one, and records its on-disk path as save_file's default target. Response is trimmed by default — detail:true / render:true restore the full envelope. Errors list the available names if no match.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name":    { "type": "string", "description": "Example name (without .ali)." },
                    "modules": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Optional extra <Name>.ali → contents overrides.",
                    },
                    "render": render_prop(),
                    "detail": detail_prop(),
                },
                "required": ["name"],
            },
        }),
        json!({
            "name": "save_file",
            "description": "Write the current running source to disk (the MCP analog of the browser's save). `path` defaults to the last load_source/load_example path; errors if neither is set. Uses the path as given (absolute, or relative to the server's CWD).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Target path. Defaults to the last loaded path." },
                },
            },
        }),
        json!({
            "name": "start_session",
            "description": "Begin a rewrite session on the named type. The initial diagram (and optional target) may be a name from the loaded source or an inline expression. Set backward:true for backward rewriting (match output boundaries, advance via input). Requires a prior load_source. Responses are trimmed by default — detail:true / render:true restore the full envelope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type_name":         { "type": "string" },
                    "initial":           { "type": "string" },
                    "target":            { "type": "string" },
                    "backward":          { "type": "boolean", "default": false },
                    "render": render_prop(),
                    "detail": detail_prop(),
                },
                "required": ["type_name", "initial"],
            },
        }),
        json!({
            "name": "resume_session",
            "description": "Reopen a stored proof diagram as a live session, decomposing it back into its steps. `proof` (and optional `target`) may be a name from the loaded source or an inline expression. Set backward:true to resume in backward-rewriting mode. Requires a prior load_source. Responses are trimmed by default — detail:true / render:true restore the full envelope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type_name": { "type": "string" },
                    "proof":     { "type": "string", "description": "Stored proof diagram name or inline expression." },
                    "target":    { "type": "string" },
                    "backward":  { "type": "boolean", "default": false },
                    "render": render_prop(),
                    "detail": detail_prop(),
                },
                "required": ["type_name", "proof"],
            },
        }),
        json!({
            "name": "run_command",
            "description": "Send a daemon-protocol command. The arguments object IS the command — set `command` to one of: step, step_multi, random, auto, undo, undo_to, redo, redo_to, show, proof, list_rules, history, store, save, types, type, cell, homology, parallel, set_target, backward, holes, fill, done, stop — and supply that command's fields alongside (e.g. `{command:'step', choice:0}`). The hole-filling workflow lives here: `{command:'holes'}` lists open `?` holes, `{command:'fill', index:0}` opens a fill session for one, then `{command:'done'}` splices the result back into the map. (start/resume/load are NOT valid here — use the load_source / start_session / resume_session tools instead.) Alternatively pass `{command_json: '<raw>'}` to forward an arbitrary JSON body. Responses are trimmed by default — detail:true / render:true restore the full envelope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command":      { "type": "string", "description": "Command name (snake_case)." },
                    "command_json": { "type": "string", "description": "Escape hatch: raw JSON command. Takes precedence over the structured form." },
                    "choice":       { "type": "integer" },
                    "max_steps":    { "type": "integer" },
                    "step":         { "type": "integer" },
                    "index":        { "type": "integer", "description": "Hole index for `fill` (0-based, as listed by `holes`)." },
                    "name":         { "type": "string" },
                    "path":         { "type": "string" },
                    "render": render_prop(),
                    "detail": detail_prop(),
                },
            },
        }),
        json!({
            "name": "get_types",
            "description": "Return the type list with generators, diagrams and maps. Trimmed to structural names/dims/map-holes by default — pass detail:true for boundary labels. Requires a prior load_source.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "render": render_prop(),
                    "detail": detail_prop(),
                },
            },
        }),
        json!({
            "name": "get_strdiag",
            "description": "Return string-diagram data for a named generator or diagram inside a type. Optionally extract a boundary by dimension and sign ('input' or 'output').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type_name":     { "type": "string" },
                    "item_name":     { "type": "string" },
                    "boundary_dim":  { "type": "integer" },
                    "boundary_sign": { "type": "string", "enum": ["input", "output"] },
                },
                "required": ["type_name", "item_name"],
            },
        }),
        json!({
            "name": "get_session_strdiag",
            "description": "String-diagram data for the current diagram in the active session.",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        json!({
            "name": "get_target_strdiag",
            "description": "String-diagram data for the active session's target diagram, if one is set.",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        json!({
            "name": "get_proof_strdiag",
            "description": "String-diagram data for the proof diagram of the active session (the accumulated rewrite witness).",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        json!({
            "name": "get_rewrite_preview_strdiag",
            "description": "String-diagram data for the diagram that would result from applying rewrite `choice` in the active session, without committing the step.",
            "inputSchema": {
                "type": "object",
                "properties": { "choice": { "type": "integer" } },
                "required": ["choice"],
            },
        }),
        json!({
            "name": "list_examples",
            "description": "List the .ali example modules visible in the configured examples directory. These are auto-seeded as virtual modules for load_source so `include <Name>` resolves directly.",
            "inputSchema": { "type": "object", "properties": {} },
        }),
    ]
}

// ── tools/call ───────────────────────────────────────────────────────────────

fn tools_call_response(
    id: Value,
    params: Value,
    repl: &mut WebRepl,
    examples: &ExampleSet,
    last_loaded_path: &mut Option<String>,
) -> Value {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return error_response(id, -32602, "tools/call: missing 'name'");
    };
    let name = name.to_string();
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let render = args.get("render").and_then(|v| v.as_bool()).unwrap_or(false);
    let detail = args.get("detail").and_then(|v| v.as_bool()).unwrap_or(false);

    let body = dispatch(&name, args, repl, examples, last_loaded_path);
    let body_text = match body {
        Ok(s) => s,
        Err(msg) => return tool_text_result(id, &error_envelope(&msg), true),
    };
    let body_text = project_envelope(&body_text, render, detail);

    let is_error = serde_json::from_str::<Value>(&body_text)
        .ok()
        .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s == "error"))
        .unwrap_or(false);
    tool_text_result(id, &body_text, is_error)
}

/// Trim a successful tool envelope for the wire.  `render` keeps the top-level
/// `rendered` transcript; `detail` keeps the heavy boundary-label payloads.
/// Both default off — the LLM asks for the bloat only when it needs it.  Total:
/// any parse hiccup returns the original string untouched.
fn project_envelope(body: &str, render: bool, detail: bool) -> String {
    let Ok(Value::Object(mut obj)) = serde_json::from_str::<Value>(body) else {
        return body.to_string();
    };
    if !render {
        obj.remove("rendered");
    }
    if !detail {
        if let Some(types) = obj.get_mut("types").and_then(|v| v.as_array_mut()) {
            trim_types(types);
        }
        if let Some(data) = obj.get_mut("data").and_then(|v| v.as_object_mut()) {
            if let Some(types) = data.get_mut("types").and_then(|v| v.as_array_mut()) {
                trim_types(types);
            }
            if let Some(rewrites) = data.get_mut("rewrites").and_then(|v| v.as_array_mut()) {
                for r in rewrites.iter_mut() {
                    *r = trim_rewrite(r);
                }
            }
            for key in ["current", "initial", "target"] {
                if let Some(d) = data.get_mut(key).and_then(|v| v.as_object_mut()) {
                    d.remove("cells_by_dim");
                }
            }
        }
    }
    Value::Object(obj).to_string()
}

/// Strip each type object down to its structural skeleton — names, dims and map
/// holes — dropping the big `input`/`output` label strings, tags and thin_tags.
fn trim_types(types: &mut [Value]) {
    for t in types.iter_mut() {
        let Some(obj) = t.as_object() else { continue };
        let pick = |key: &str| obj.get(key).cloned().unwrap_or(Value::Null);
        let generators = pick("generators")
            .as_array()
            .map(|gs| gs.iter().map(|g| json!({ "name": g.get("name"), "dim": g.get("dim") })).collect())
            .unwrap_or_default();
        let diagrams = pick("diagrams")
            .as_array()
            .map(|ds| ds.iter().map(|d| json!({ "name": d.get("name") })).collect())
            .unwrap_or_default();
        let maps = pick("maps")
            .as_array()
            .map(|ms| {
                ms.iter()
                    .map(|m| json!({ "name": m.get("name"), "domain": m.get("domain"), "holes": m.get("holes") }))
                    .collect()
            })
            .unwrap_or_default();
        *t = json!({
            "name": pick("name"),
            "module": pick("module"),
            "generators": Value::Array(generators),
            "diagrams": Value::Array(diagrams),
            "maps": Value::Array(maps),
        });
    }
}

/// Keep a rewrite's index and match metadata; drop the input/output diagrams.
/// `family` survives only when it is a non-empty array.
fn trim_rewrite(r: &Value) -> Value {
    let mut out = Map::new();
    for key in ["index", "rule_name", "match_positions", "match_display"] {
        if let Some(v) = r.get(key) {
            out.insert(key.to_string(), v.clone());
        }
    }
    if let Some(family) = r.get("family").and_then(|v| v.as_array())
        && !family.is_empty()
    {
        out.insert("family".to_string(), Value::Array(family.clone()));
    }
    Value::Object(out)
}

fn tool_text_result(id: Value, text: &str, is_error: bool) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": text }],
            "isError": is_error,
        },
    })
}

fn error_envelope(message: &str) -> String {
    json!({ "status": "error", "message": message }).to_string()
}

/// Auto-seed every example as a `<Name>.ali` virtual module, merge any caller
/// overrides, and load `source` under `source_name`.  Shared by `load_source`
/// and `load_example` — the two differ only in where `source` comes from.
fn seed_and_load(
    repl: &mut WebRepl,
    examples: &ExampleSet,
    source: &str,
    extra: Option<&Map<String, Value>>,
    source_name: Option<&str>,
) -> String {
    let mut modules: HashMap<String, String> = HashMap::new();
    // Scan failures (duplicate stems, IO) are non-fatal here — surface them
    // through list_examples instead.
    if let Ok(entries) = examples.scan() {
        for e in entries {
            modules.insert(format!("{}.ali", e.name), e.content);
        }
    }
    if let Some(extra) = extra {
        for (k, v) in extra {
            if let Some(s) = v.as_str() {
                modules.insert(k.clone(), s.to_string());
            }
        }
    }
    repl.load_source_with_modules(source, modules, source_name)
}

fn dispatch(
    name: &str,
    args: Value,
    repl: &mut WebRepl,
    examples: &ExampleSet,
    last_loaded_path: &mut Option<String>,
) -> Result<String, String> {
    match name {
        "load_source" => {
            let source = args
                .get("source")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "load_source: missing 'source'".to_string())?;
            let extra = args.get("modules").and_then(|v| v.as_object());
            let source_name = args.get("source_name").and_then(|v| v.as_str());
            if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                *last_loaded_path = Some(path.to_string());
            }
            Ok(seed_and_load(repl, examples, source, extra, source_name))
        }
        "load_example" => {
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "load_example: missing 'name'".to_string())?;
            let entries = examples.scan().map_err(|e| format!("{:?}", e))?;
            let Some(entry) = entries.iter().find(|e| e.name == name) else {
                let available: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
                return Ok(error_envelope(&format!(
                    "no example named '{}'; available: {}",
                    name,
                    available.join(", ")
                )));
            };
            let extra = args.get("modules").and_then(|v| v.as_object());
            *last_loaded_path =
                Some(examples.dir().join(&entry.path).to_string_lossy().into_owned());
            Ok(seed_and_load(repl, examples, &entry.content, extra, Some(name)))
        }
        "save_file" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| last_loaded_path.clone())
                .ok_or_else(|| {
                    "save_file: no 'path' given and no prior load to default to".to_string()
                })?;
            let env: Value = serde_json::from_str(&repl.run_command(r#"{"command":"save"}"#))
                .map_err(|e| format!("save: malformed engine response: {e}"))?;
            let Some(source) = env.pointer("/data/source").and_then(|v| v.as_str()) else {
                return Ok(error_envelope("save: no running source to write"));
            };
            match std::fs::write(&path, source) {
                Ok(()) => Ok(json!({
                    "status": "ok",
                    "data": { "saved": path, "bytes": source.len() },
                })
                .to_string()),
                Err(e) => Ok(error_envelope(&format!("cannot write '{}': {}", path, e))),
            }
        }
        "start_session" => {
            let type_name = args
                .get("type_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "start_session: missing 'type_name'".to_string())?;
            let initial = args
                .get("initial")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "start_session: missing 'initial'".to_string())?;
            let target = args
                .get("target")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let backward = args
                .get("backward")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Ok(repl.start_session(type_name, initial, target, backward))
        }
        "resume_session" => {
            let type_name = args
                .get("type_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "resume_session: missing 'type_name'".to_string())?;
            let proof = args
                .get("proof")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "resume_session: missing 'proof'".to_string())?;
            let target = args
                .get("target")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let backward = args
                .get("backward")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Ok(repl.resume_session(type_name, proof, target, backward))
        }
        "run_command" => {
            // command_json wins if both are given — explicit raw form.
            let cmd_json = if let Some(s) = args.get("command_json").and_then(|v| v.as_str()) {
                s.to_string()
            } else if args.get("command").is_some() {
                // Strip command_json (if any) and serialise the rest as the command body.
                let mut cleaned = args.clone();
                if let Some(obj) = cleaned.as_object_mut() {
                    obj.remove("command_json");
                }
                cleaned.to_string()
            } else {
                return Err(
                    "run_command: provide either 'command' or 'command_json'".to_string(),
                );
            };
            Ok(repl.run_command(&cmd_json))
        }
        "get_types" => Ok(repl.get_types()),
        "get_strdiag" => {
            let type_name = args
                .get("type_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "get_strdiag: missing 'type_name'".to_string())?;
            let item_name = args
                .get("item_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "get_strdiag: missing 'item_name'".to_string())?;
            let boundary_dim = args
                .get("boundary_dim")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            let boundary_sign = args
                .get("boundary_sign")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Ok(repl.get_strdiag(type_name, item_name, boundary_dim, boundary_sign))
        }
        "get_session_strdiag" => Ok(repl.get_session_strdiag()),
        "get_target_strdiag" => Ok(repl.get_target_strdiag()),
        "get_proof_strdiag" => Ok(repl.get_proof_strdiag()),
        "get_rewrite_preview_strdiag" => {
            let choice = args
                .get("choice")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| "get_rewrite_preview_strdiag: missing 'choice'".to_string())?
                as usize;
            Ok(repl.get_rewrite_preview_strdiag(choice))
        }
        "list_examples" => match examples.scan() {
            Ok(entries) => {
                let listed: Vec<Value> = entries
                    .iter()
                    .map(|e| json!({ "name": e.name, "path": e.path }))
                    .collect();
                Ok(json!({
                    "status": "ok",
                    "data": {
                        "dir": examples.dir().to_string_lossy(),
                        "examples": listed,
                    },
                })
                .to_string())
            }
            Err(e) => Ok(error_envelope(&format!("{:?}", e))),
        },
        other => Err(format!("unknown tool '{}'", other)),
    }
}
