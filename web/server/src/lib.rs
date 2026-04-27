//! Localhost-only HTTP server for the Alifib web GUI.
//!
//! Serves the browser assets from `web/frontend/` and exposes a small
//! same-origin JSON API backed by a single long-lived
//! [`alifib::interactive::web::WebRepl`], in the spirit of a local notebook
//! kernel.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use alifib::interactive::web::WebRepl;
use alifib_web_shared::ExampleSet;
use serde::Deserialize;
use serde::de::DeserializeOwned;

const INDEX_HTML: &str = include_str!("../../frontend/index.html");
const APP_JS: &str = include_str!("../../frontend/dist/app.js");
const STYLE_CSS: &str = include_str!("../../frontend/style.css");
const INDEX_SCRIPT_TAG: &str = r#"  <script type="module" src="dist/app.js"></script>"#;
const HTTP_CONFIG_TAG: &str = r#"  <script>window.ALIFIB_CONFIG = { backend: 'http', apiBase: '' };</script>
  <script type="module" src="app.js"></script>"#;

pub fn run_web_server(bind_addr: &str, examples: ExampleSet) -> Result<(), String> {
    let listener =
        TcpListener::bind(bind_addr).map_err(|e| format!("could not bind {}: {}", bind_addr, e))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("could not read local address: {}", e))?;
    eprintln!("alifib web listening on http://{}", local_addr);
    eprintln!("alifib web serving examples from {}", examples.dir().display());

    let mut repl = WebRepl::new();
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                if let Err(err) = handle_connection(&mut stream, &mut repl, &examples) {
                    let _ = write_text_response(
                        &mut stream,
                        500,
                        "text/plain; charset=utf-8",
                        &format!("internal server error: {}", err),
                    );
                }
            }
            Err(err) => eprintln!("web: accept error: {}", err),
        }
    }

    Ok(())
}

fn handle_connection(
    stream: &mut TcpStream,
    repl: &mut WebRepl,
    examples: &ExampleSet,
) -> Result<(), String> {
    let request = read_request(stream)?;
    let path = request.path.split('?').next().unwrap_or(&request.path);

    match (request.method.as_str(), path) {
        ("GET", "/") | ("GET", "/index.html") => {
            write_text_response(stream, 200, "text/html; charset=utf-8", &index_html())
        }
        ("GET", "/app.js") => {
            write_text_response(stream, 200, "application/javascript; charset=utf-8", APP_JS)
        }
        ("GET", "/style.css") => {
            write_text_response(stream, 200, "text/css; charset=utf-8", STYLE_CSS)
        }
        ("GET", "/favicon.ico") => {
            write_text_response(stream, 204, "text/plain; charset=utf-8", "")
        }

        ("POST", "/api/load_source") => {
            let body: LoadSourceBody = parse_json_body(&request.body)?;
            let modules = body.modules.unwrap_or_default();
            write_json_response(
                stream,
                200,
                repl.load_source_with_modules(&body.source, modules),
            )
        }
        ("POST", "/api/init_session") => {
            let body: InitSessionBody = parse_json_body(&request.body)?;
            write_json_response(
                stream,
                200,
                repl.init_session(&body.type_name, &body.source_diagram, body.target_diagram),
            )
        }
        ("POST", "/api/run_command") => {
            let body: RunCommandBody = parse_json_body(&request.body)?;
            write_json_response(stream, 200, repl.run_command(&body.command_json))
        }
        ("POST", "/api/stop_session") => {
            repl.stop_session();
            write_json_response(stream, 200, r#"{"status":"ok"}"#.to_owned())
        }
        ("POST", "/api/get_types") => write_json_response(stream, 200, repl.get_types()),

        ("GET", "/examples/index.json") => {
            write_json_response(stream, 200, examples.index_json())
        }
        ("GET", p) if p.starts_with("/examples/") && p.ends_with(".ali") => {
            let rel = &p["/examples/".len()..];
            // `read_path` validates each segment against the identifier rule
            // and canonicalises-in-root — anything dubious returns None and
            // we hand back 404 rather than leaking the reason.
            match examples.read_path(rel) {
                Some(content) => {
                    write_text_response(stream, 200, "text/plain; charset=utf-8", &content)
                }
                None => write_text_response(stream, 404, "text/plain; charset=utf-8", "not found"),
            }
        }

        ("POST", "/api/get_strdiag") => {
            let body: GetStrdiagBody = parse_json_body(&request.body)?;
            write_json_response(
                stream,
                200,
                repl.get_strdiag(
                    &body.type_name,
                    &body.item_name,
                    body.boundary_dim,
                    body.boundary_sign,
                ),
            )
        }
        ("POST", "/api/get_session_strdiag") => {
            write_json_response(stream, 200, repl.get_session_strdiag())
        }
        ("POST", "/api/get_rewrite_preview_strdiag") => {
            let body: RewritePreviewBody = parse_json_body(&request.body)?;
            write_json_response(stream, 200, repl.get_rewrite_preview_strdiag(body.choice))
        }

        ("POST", _) if path.starts_with("/api/") => write_json_response(
            stream,
            404,
            api_error_json(&format!("unknown API route '{}'", path)),
        ),
        ("GET", _) => write_text_response(stream, 404, "text/plain; charset=utf-8", "not found"),
        _ => write_text_response(
            stream,
            405,
            "text/plain; charset=utf-8",
            "method not allowed",
        ),
    }
    .map_err(|e| e.to_string())
}

fn index_html() -> String {
    INDEX_HTML.replace(INDEX_SCRIPT_TAG, HTTP_CONFIG_TAG)
}

fn parse_json_body<T: DeserializeOwned>(body: &str) -> Result<T, String> {
    serde_json::from_str(body).map_err(|e| format!("invalid JSON body: {}", e))
}

fn api_error_json(message: &str) -> String {
    serde_json::json!({
        "status": "error",
        "message": message,
    })
    .to_string()
}

fn write_json_response(stream: &mut TcpStream, status: u16, body: String) -> std::io::Result<()> {
    write_text_response(stream, status, "application/json; charset=utf-8", &body)
}

fn write_text_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let bytes = body.as_bytes();
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        reason,
        content_type,
        bytes.len()
    )?;
    stream.write_all(bytes)?;
    stream.flush()
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);

    let mut request_line = String::new();
    if reader
        .read_line(&mut request_line)
        .map_err(|e| e.to_string())?
        == 0
    {
        return Err("empty request".to_string());
    }

    let request_line = request_line.trim_end();
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "invalid request line".to_string())?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| "invalid request line".to_string())?
        .to_string();

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }

        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            content_length = value
                .trim()
                .parse::<usize>()
                .map_err(|e| format!("invalid content-length: {}", e))?;
        }
    }

    let mut body = vec![0; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|e| format!("could not read request body: {}", e))?;

    let body = String::from_utf8(body).map_err(|e| format!("request body is not UTF-8: {}", e))?;
    Ok(HttpRequest { method, path, body })
}

struct HttpRequest {
    method: String,
    path: String,
    body: String,
}

#[derive(Deserialize)]
struct LoadSourceBody {
    source: String,
    /// Optional `<Name>.ali → contents` map: lets the frontend seed the virtual
    /// loader with whatever examples it has fetched over HTTP, so `include`
    /// statements in the user's source resolve without any server-side file
    /// access.
    #[serde(default)]
    modules: Option<std::collections::HashMap<String, String>>,
}

#[derive(Deserialize)]
struct InitSessionBody {
    type_name: String,
    source_diagram: String,
    #[serde(default)]
    target_diagram: Option<String>,
}

#[derive(Deserialize)]
struct RunCommandBody {
    command_json: String,
}

#[derive(Deserialize)]
struct GetStrdiagBody {
    type_name: String,
    item_name: String,
    #[serde(default)]
    boundary_dim: Option<usize>,
    #[serde(default)]
    boundary_sign: Option<String>,
}

#[derive(Deserialize)]
struct RewritePreviewBody {
    choice: usize,
}
