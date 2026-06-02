//! Stdio JSON-lines daemon for `alifib serve`.
//!
//! Reads one JSON [`Request`] per line from stdin, drives a shared [`Session`],
//! and writes one JSON [`Response`] per line to stdout.  The session machine is
//! the same one the CLI and web REPLs use, so the daemon supports the full
//! command set — including `holes`/`fill`/`done`/`save`/`backward`.
//!
//! This protocol is suitable for editor integration: spawn `alifib serve`
//! as a subprocess and communicate via its stdin/stdout.
//!
//! # Example session
//!
//! ```text
//! → {"command":"start","source_file":"Idem.ali","type_name":"Idem","initial":"lhs"}
//! ← {"status":"ok","data":{"step_count":0,"current":{"label":"id id id",...},...}}
//! → {"command":"step","choice":0}
//! ← {"status":"ok","data":{"step_count":1,...}}
//! → {"command":"shutdown"}
//! ```

use std::io::{BufRead, Write};

use super::protocol::{
    build_cell_response, build_type_detail_from_store, build_types_from_store,
    Request, Response, ResponseData,
};
use super::session::Session;

/// Run the daemon loop: read requests from stdin, write responses to stdout.
///
/// If `initial` is `Some`, its state is emitted before entering the loop.
/// Returns when a [`Request::Shutdown`] is received or stdin is closed.
#[allow(clippy::result_unit_err)]
pub fn run_daemon(initial: Option<Session>) -> Result<(), ()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut session: Option<Session> = None;

    if let Some(s) = initial {
        if s.session_active() {
            emit(&stdout, &Response::Ok { data: s.state() });
        }
        session = Some(s);
    }

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                emit(&stdout, &Response::error(format!("read error: {}", e)));
                return Err(());
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request = match serde_json::from_str::<Request>(line) {
            Ok(r) => r,
            Err(e) => {
                emit(&stdout, &Response::error(format!("invalid request: {}", e)));
                continue;
            }
        };

        match dispatch(&mut session, request) {
            DispatchResult::Respond(resp) => emit(&stdout, &resp),
            DispatchResult::Shutdown => break,
        }
    }

    Ok(())
}

#[allow(clippy::large_enum_variant)]
enum DispatchResult {
    Respond(Response),
    Shutdown,
}

fn dispatch(session: &mut Option<Session>, req: Request) -> DispatchResult {
    let resp = match req {
        Request::Shutdown => return DispatchResult::Shutdown,

        // Load an idle session (so `holes`/`fill` work before any rewrite).
        Request::Load { source_file } => match Session::from_disk(&source_file) {
            Ok(s) => {
                let mut data = ResponseData::empty();
                data.types = build_types_from_store(s.store(), s.root_path());
                *session = Some(s);
                Response::Ok { data }
            }
            Err(e) => Response::error(e),
        },

        // (Re)load from disk, then start/resume the rewrite on the loaded store.
        Request::Start { .. } | Request::Resume { .. } => {
            let source_file = match &req {
                Request::Start { source_file, .. } | Request::Resume { source_file, .. } => source_file.clone(),
                _ => unreachable!(),
            };
            match Session::from_disk(&source_file) {
                Ok(s) => { *session = Some(s); apply(session, req) }
                Err(e) => Response::error(e),
            }
        }

        // Read-only queries, served from the loaded store (rendered per-medium).
        Request::Types | Request::TypeInfo { .. } | Request::Cell { .. } | Request::Homology { .. } =>
            query(session, req),

        // Everything else is a session command.
        other => apply(session, other),
    };
    DispatchResult::Respond(resp)
}

/// Apply a session command to the active session, or report that none exists.
fn apply(session: &mut Option<Session>, req: Request) -> Response {
    match session.as_mut() {
        None => Response::error("no session — send 'start' or 'resume' first"),
        Some(s) => match s.apply(req) {
            Ok(data) => Response::Ok { data },
            Err(msg) => Response::error(msg),
        },
    }
}

/// Serve a read-only query from the loaded store.
fn query(session: &Option<Session>, req: Request) -> Response {
    let Some(s) = session.as_ref() else {
        return Response::error("no source loaded — send 'start' first");
    };
    match req {
        Request::Types => {
            let mut data = ResponseData::empty();
            data.types = build_types_from_store(s.store(), s.root_path());
            Response::Ok { data }
        }
        Request::TypeInfo { name } => match build_type_detail_from_store(s.store(), s.root_path(), &name) {
            Ok(detail) => {
                let mut data = ResponseData::empty();
                data.type_detail = Some(detail);
                Response::Ok { data }
            }
            Err(msg) => Response::error(msg),
        },
        Request::Cell { name } => match s.engine() {
            Some(e) => match build_cell_response(e, &name) {
                Ok(data) => Response::Ok { data },
                Err(msg) => Response::error(msg),
            },
            None => Response::error("no active session — `cell` needs a session for its type context"),
        },
        Request::Homology { .. } => Response::error("homology not supported in daemon mode"),
        _ => unreachable!(),
    }
}

fn emit(stdout: &std::io::Stdout, resp: &Response) {
    match serde_json::to_string(resp) {
        Ok(json) => {
            let mut out = stdout.lock();
            let _ = writeln!(out, "{}", json);
            let _ = out.flush();
        }
        Err(e) => {
            let mut out = stdout.lock();
            let _ = writeln!(out, "{{\"status\":\"error\",\"message\":\"serialization failed: {}\"}}", e);
            let _ = out.flush();
        }
    }
}
