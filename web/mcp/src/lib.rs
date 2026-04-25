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
//! `isError: true` so MCP clients can branch on it without parsing.
//!
//! `load_source` auto-seeds the configured examples directory as virtual
//! `<Name>.ali` modules, so `include` resolves without the agent having to
//! ship example contents itself.  `list_examples` exposes the same set for
//! discovery.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use alifib::interactive::web::WebRepl;
use alifib_web_shared::ExampleSet;
use serde::Deserialize;
use serde_json::{Value, json};

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

fn tool_descriptors() -> Vec<Value> {
    vec![
        json!({
            "name": "load_source",
            "description": "Parse and interpret alifib (.ali) source. Modules from the configured examples directory are auto-seeded so `include <Name>` works without shipping content. Returns a JSON envelope with the type list (or diagnostics on parse/type error).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source":  { "type": "string", "description": ".ali source text" },
                    "modules": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Optional extra <Name>.ali → contents overrides. Merged on top of the auto-seeded examples directory.",
                    },
                },
                "required": ["source"],
            },
        }),
        json!({
            "name": "init_session",
            "description": "Begin a rewrite session on the named type. The source diagram (and optional target) may be a name from the loaded source or an inline expression. Requires a prior load_source.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type_name":      { "type": "string" },
                    "source_diagram": { "type": "string" },
                    "target_diagram": { "type": "string" },
                },
                "required": ["type_name", "source_diagram"],
            },
        }),
        json!({
            "name": "run_command",
            "description": "Send a daemon-protocol command. The arguments object IS the command — set `command` to one of: step, auto, undo, undo_to, show, list_rules, history, store, types, type, cell, homology, and supply that command's fields alongside (e.g. `{command:'step', choice:0}`). Alternatively pass `{command_json: '<raw>'}` to forward an arbitrary JSON body.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command":      { "type": "string", "description": "Command name (snake_case)." },
                    "command_json": { "type": "string", "description": "Escape hatch: raw JSON command. Takes precedence over the structured form." },
                    "choice":       { "type": "integer" },
                    "max_steps":    { "type": "integer" },
                    "step":         { "type": "integer" },
                    "name":         { "type": "string" },
                    "path":         { "type": "string" },
                },
            },
        }),
        json!({
            "name": "get_types",
            "description": "Return the type list with generators, diagrams and maps. Requires a prior load_source.",
            "inputSchema": { "type": "object", "properties": {} },
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
) -> Value {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return error_response(id, -32602, "tools/call: missing 'name'");
    };
    let name = name.to_string();
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let body = dispatch(&name, args, repl, examples);
    let body_text = match body {
        Ok(s) => s,
        Err(msg) => return tool_text_result(id, &error_envelope(&msg), true),
    };

    let is_error = serde_json::from_str::<Value>(&body_text)
        .ok()
        .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s == "error"))
        .unwrap_or(false);
    tool_text_result(id, &body_text, is_error)
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

fn dispatch(
    name: &str,
    args: Value,
    repl: &mut WebRepl,
    examples: &ExampleSet,
) -> Result<String, String> {
    match name {
        "load_source" => {
            let source = args
                .get("source")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "load_source: missing 'source'".to_string())?;
            let mut modules: HashMap<String, String> = HashMap::new();
            // Auto-seed the examples dir.  Scan failures (duplicate stems, IO)
            // are non-fatal here — surface them through list_examples instead.
            if let Ok(entries) = examples.scan() {
                for e in entries {
                    modules.insert(format!("{}.ali", e.name), e.content);
                }
            }
            if let Some(extra) = args.get("modules").and_then(|v| v.as_object()) {
                for (k, v) in extra {
                    if let Some(s) = v.as_str() {
                        modules.insert(k.clone(), s.to_string());
                    }
                }
            }
            Ok(repl.load_source_with_modules(source, modules))
        }
        "init_session" => {
            let type_name = args
                .get("type_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "init_session: missing 'type_name'".to_string())?;
            let source_diagram = args
                .get("source_diagram")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "init_session: missing 'source_diagram'".to_string())?;
            let target = args
                .get("target_diagram")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Ok(repl.init_session(type_name, source_diagram, target))
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
