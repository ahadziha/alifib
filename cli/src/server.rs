//! Threaded HTTP server for the Alifib web GUI and its shared RPC kernel.

use std::borrow::Cow;
use std::convert::Infallible;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{LazyLock, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alifib::interactive::kernel::{CanonicalDir, FsAccess, Kernel};
use alifib::interactive::rpc::Rpc;

use crate::examples::{ExampleSet, index_error_json};
use crate::http::{self, ReadError, Request, Status};

pub use crate::http::Limits;

const INDEX_HTML: &str = include_str!("../../web/frontend/index.html");
const APP_JS: &str = include_str!("../../web/frontend/dist/app.js");
const STYLE_CSS: &str = include_str!("../../web/frontend/style.css");
/// The static page's bundle tag, replaced by the HTTP backend's configuration.
const INDEX_SCRIPT_TAG: &str = r#"  <script type="module" src="dist/app.js"></script>"#;
const HTTP_CONFIG_TAG: &str = r#"  <script>window.ALIFIB_CONFIG = { backend: 'http' };</script>
  <script type="module" src="app.js"></script>"#;
static PAGE: LazyLock<String> =
    LazyLock::new(|| INDEX_HTML.replace(INDEX_SCRIPT_TAG, HTTP_CONFIG_TAG));

const HEADLESS_NOTE: &str =
    "headless alifib server — drive it with `alifib do`, or start `alifib web` for the browser";
const DISK_DENIED: &str = "disk access is disabled on a non-loopback server";
const KERNEL_PANICKED: &str = "the kernel panicked; restart the server";
/// The pause after a failed accept or thread spawn, so exhaustion cannot spin.
const BACKOFF: Duration = Duration::from_millis(100);

type Headers = &'static [(&'static str, &'static str)];
const TEXT: Headers = &[("Content-Type", "text/plain; charset=utf-8")];
const JSON: Headers = &[("Content-Type", "application/json; charset=utf-8")];
const HTML: Headers = &[("Content-Type", "text/html; charset=utf-8")];
const JAVASCRIPT: Headers = &[("Content-Type", "application/javascript; charset=utf-8")];
const CSS: Headers = &[("Content-Type", "text/css; charset=utf-8")];
const NOT_ALLOWED: Headers = &[
    ("Content-Type", "text/plain; charset=utf-8"),
    ("Allow", "GET, POST"),
];

/// What the server offers besides its API.
pub enum Ui {
    /// The browser page, and the examples its picker lists.
    Browser(ExampleSet),
    /// The API alone, for `alifib do`.
    Headless,
}

impl Ui {
    /// The subcommand serving this interface, which names it in messages.
    fn command(&self) -> &'static str {
        match self {
            Self::Browser(_) => "web",
            Self::Headless => "serve",
        }
    }
}

/// The JSON call surface: an answer, or the text of an HTTP 500.
pub type Api = dyn Fn(&str) -> Result<String, String> + Send + Sync;

/// Who may reach the server, from the address it actually bound.
enum Access {
    /// Requests naming one of these hosts; disk access within the directory.
    Loopback(Vec<String>),
    /// Any host; no disk access.
    NonLoopback,
}

fn access(address: SocketAddr) -> Access {
    if !address.ip().is_loopback() {
        return Access::NonLoopback;
    }
    let mut hosts = vec![format!("localhost:{}", address.port()), address.to_string()];
    if address.port() == 80 {
        hosts.push("localhost".to_owned());
        hosts.push(match address {
            SocketAddr::V4(address) => address.ip().to_string(),
            SocketAddr::V6(address) => format!("[{}]", address.ip()),
        });
    }
    Access::Loopback(hosts)
}

impl Access {
    fn fs_access(&self, dir: CanonicalDir) -> FsAccess {
        match self {
            Self::Loopback(_) => FsAccess::Within(dir),
            Self::NonLoopback => FsAccess::Denied(DISK_DENIED),
        }
    }

    /// Host names are case-insensitive; the port must be the bound one.
    fn accepts_host(&self, host: &str) -> bool {
        match self {
            Self::Loopback(hosts) => hosts
                .iter()
                .any(|accepted| accepted.eq_ignore_ascii_case(host)),
            Self::NonLoopback => true,
        }
    }
}

fn bound(listener: &TcpListener) -> Result<SocketAddr, String> {
    listener
        .local_addr()
        .map_err(|error| format!("could not read local address: {error}"))
}

/// Run a shared RPC kernel scoped to the directory and actual bound address.
pub fn run(
    listener: TcpListener,
    dir: CanonicalDir,
    ui: Ui,
    limits: Limits,
) -> Result<Infallible, String> {
    let address = bound(&listener)?;
    let instance = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("could not read server start time: {error}"))?
        .as_nanos()
        .to_string();
    let access = access(address);
    let command = ui.command();
    eprintln!("alifib {command} listening on http://{address}");
    eprintln!(
        "alifib {command} serving directory {}",
        dir.path().display()
    );
    if let Access::NonLoopback = access {
        eprintln!("alifib {command}: {DISK_DENIED}");
    }
    let rpc = Mutex::new(Rpc::new(Kernel::new(access.fs_access(dir)), instance));
    // A panic unwinds through the guard and poisons the mutex, so the call that
    // panicked and every later one are answered 500.
    let api = move |request: &str| {
        panic::catch_unwind(AssertUnwindSafe(|| {
            rpc.lock().ok().map(|mut rpc| rpc.call(request))
        }))
        .ok()
        .flatten()
        .ok_or_else(|| KERNEL_PANICKED.to_owned())
    };
    Ok(accept(&listener, &access, &api, &ui, limits))
}

/// Serve `api` and `ui` on independent connections; static routes never call
/// `api`. Tests can supply a gate in place of the kernel.
pub fn serve(
    listener: TcpListener,
    api: &Api,
    ui: Ui,
    limits: Limits,
) -> Result<Infallible, String> {
    let access = access(bound(&listener)?);
    Ok(accept(&listener, &access, api, &ui, limits))
}

fn accept(
    listener: &TcpListener,
    access: &Access,
    api: &Api,
    ui: &Ui,
    limits: Limits,
) -> Infallible {
    let command = ui.command();
    thread::scope(|scope| {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let spawned = thread::Builder::new().spawn_scoped(scope, move || {
                        if let Err(error) = handle(&stream, access, api, ui, limits) {
                            eprintln!("alifib {command}: connection error: {error}");
                        }
                    });
                    // A connection without a thread is dropped, which closes it.
                    if let Err(error) = spawned {
                        eprintln!("alifib {command}: could not start a connection thread: {error}");
                        thread::sleep(BACKOFF);
                    }
                }
                Err(error) => {
                    eprintln!("alifib {command}: accept error: {error}");
                    thread::sleep(BACKOFF);
                }
            }
        }
    })
}

/// Read one request, answer it, and close. Peers that go before a request
/// arrives, or before its answer is written, are not errors.
fn handle(
    stream: &TcpStream,
    access: &Access,
    api: &Api,
    ui: &Ui,
    limits: Limits,
) -> io::Result<()> {
    stream.set_write_timeout(Some(limits.timeout))?;
    let answer = match http::read_request(stream, limits) {
        Ok(request) => route(&request, access, api, ui),
        Err(ReadError::Idle | ReadError::Reset) => return Ok(()),
        Err(ReadError::Io(error)) => return Err(error),
        Err(error) => {
            let status = match error {
                ReadError::TimedOut => Status::RequestTimeout,
                ReadError::HeadTooLarge => Status::HeaderFieldsTooLarge,
                ReadError::BodyTooLarge => Status::ContentTooLarge,
                _ => Status::BadRequest,
            };
            Answer::new(status, TEXT, error.to_string())
        }
    };
    match http::write_response(stream, answer.status, answer.headers, &answer.body) {
        Err(error) if http::disconnected(error.kind()) => Ok(()),
        result => result,
    }
}

/// One HTTP answer, written whole by the connection handler.
struct Answer {
    status: Status,
    headers: Headers,
    body: Cow<'static, str>,
}

impl Answer {
    fn new(status: Status, headers: Headers, body: impl Into<Cow<'static, str>>) -> Self {
        Self {
            status,
            headers,
            body: body.into(),
        }
    }

    fn not_found() -> Self {
        Self::new(Status::NotFound, TEXT, "not found")
    }
}

/// Answer a request: the Host guard, then the routes `ui` offers.
fn route(request: &Request, access: &Access, api: &Api, ui: &Ui) -> Answer {
    if !access.accepts_host(&request.host) {
        return Answer::new(Status::Forbidden, TEXT, "forbidden Host");
    }
    match (request.method.as_str(), request.path.as_str(), ui) {
        ("POST", "/api", _) => call(request, api),
        ("GET", "/", Ui::Headless) => Answer::new(Status::Ok, TEXT, HEADLESS_NOTE),
        ("GET", "/" | "/index.html", Ui::Browser(_)) => {
            Answer::new(Status::Ok, HTML, PAGE.as_str())
        }
        ("GET", "/app.js", Ui::Browser(_)) => Answer::new(Status::Ok, JAVASCRIPT, APP_JS),
        ("GET", "/style.css", Ui::Browser(_)) => Answer::new(Status::Ok, CSS, STYLE_CSS),
        ("GET", "/favicon.ico", Ui::Browser(_)) => Answer::new(Status::NoContent, &[], ""),
        ("GET", path, Ui::Browser(examples)) => match path.strip_prefix("/examples/") {
            Some("index.json") => index(examples, ui.command()),
            Some(path) => example(examples, path),
            None => Answer::not_found(),
        },
        ("GET" | "POST", _, _) => Answer::not_found(),
        _ => Answer::new(Status::MethodNotAllowed, NOT_ALLOWED, "method not allowed"),
    }
}

/// The API route: a JSON POST, from no page or a page of the same origin.
fn call(request: &Request, api: &Api) -> Answer {
    let json = request.header("content-type").is_some_and(|value| {
        let media_type = value
            .split_once(';')
            .map_or(value, |(media_type, _)| media_type);
        media_type.trim().eq_ignore_ascii_case("application/json")
    });
    if !json {
        return Answer::new(
            Status::Forbidden,
            TEXT,
            "Content-Type must be application/json",
        );
    }
    if request
        .header("origin")
        .is_some_and(|origin| !origin.eq_ignore_ascii_case(&format!("http://{}", request.host)))
    {
        return Answer::new(Status::Forbidden, TEXT, "forbidden Origin");
    }
    match api(&request.body) {
        Ok(body) => Answer::new(Status::Ok, JSON, body),
        Err(error) => Answer::new(Status::InternalServerError, TEXT, error),
    }
}

/// The example index of a fresh scan, reporting the files it skipped.
fn index(examples: &ExampleSet, command: &str) -> Answer {
    match examples.scan() {
        Ok(scan) => {
            for skipped in &scan.skipped {
                eprintln!("alifib {command}: {skipped}");
            }
            Answer::new(Status::Ok, JSON, scan.index_json())
        }
        Err(error) => Answer::new(Status::InternalServerError, JSON, index_error_json(&error)),
    }
}

fn example(examples: &ExampleSet, path: &str) -> Answer {
    match examples.read_path(path) {
        Ok(content) => Answer::new(Status::Ok, TEXT, content),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Answer::not_found(),
        Err(error) => Answer::new(
            Status::InternalServerError,
            TEXT,
            format!("could not read example {path}: {error}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn access_scope_follows_the_bound_address() {
        let dir = CanonicalDir::new(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        for address in ["127.0.0.1:32123", "[::1]:32123"] {
            let FsAccess::Within(actual) = access(address.parse().unwrap()).fs_access(dir.clone())
            else {
                panic!("{address} must confine disk access to the served directory");
            };
            assert_eq!(actual.path(), dir.path());
        }
        for address in ["0.0.0.0:32123", "192.168.1.10:32123"] {
            let FsAccess::Denied(reason) = access(address.parse().unwrap()).fs_access(dir.clone())
            else {
                panic!("{address} must deny disk access");
            };
            assert_eq!(reason, "disk access is disabled on a non-loopback server");
        }
    }

    #[test]
    fn access_host_rule_matches_the_bound_loopback_address_and_port() {
        for (address, accepted, rejected) in [
            (
                "127.0.0.1:32123",
                ["localhost:32123", "127.0.0.1:32123", "LocalHost:32123"],
                ["localhost", "127.0.0.1", "[::1]:32123", "localhost:32124"],
            ),
            (
                "[::1]:32123",
                ["localhost:32123", "[::1]:32123", "LOCALHOST:32123"],
                ["localhost", "[::1]", "::1:32123", "127.0.0.1:32123"],
            ),
        ] {
            let rule = access(address.parse().unwrap());
            for host in accepted {
                assert!(rule.accepts_host(host), "{address} should accept {host}");
            }
            for host in rejected.into_iter().chain(["foreign.example", ""]) {
                assert!(!rule.accepts_host(host), "{address} should reject {host}");
            }
        }
    }

    #[test]
    fn access_non_loopback_host_rule_does_not_restrict_hosts() {
        for address in ["0.0.0.0:32123", "192.168.1.10:32123"] {
            let rule = access(address.parse().unwrap());
            for host in ["foreign.example", "localhost:32123", "192.168.1.10:32123"] {
                assert!(rule.accepts_host(host), "{address} should accept {host}");
            }
        }
    }

    #[test]
    fn access_host_rule_allows_an_omitted_port_only_on_port_80() {
        for (address, ip_host) in [("127.0.0.1:80", "127.0.0.1"), ("[::1]:80", "[::1]")] {
            let rule = access(address.parse().unwrap());
            for host in [
                "localhost".to_owned(),
                ip_host.to_owned(),
                address.to_owned(),
                "localhost:80".to_owned(),
            ] {
                assert!(rule.accepts_host(&host), "{address} should accept {host}");
            }
            assert!(!rule.accepts_host("localhost:81"));
        }
        for address in ["127.0.0.1:81", "[::1]:81"] {
            let rule = access(address.parse().unwrap());
            for host in ["localhost", "127.0.0.1", "[::1]"] {
                assert!(!rule.accepts_host(host), "{address} should reject {host}");
            }
        }
    }
}
