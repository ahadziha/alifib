//! HTTP guards and connection independence, through the public serve port.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use alifib::interactive::kernel::CanonicalDir;
use alifib_cli::server::{Api, Limits, Ui, serve};
use alifib_cli::examples::ExampleSet;

const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);
const BODY: &str = r#"{"request":"guard"}"#;

fn limits() -> Limits {
    Limits {
        timeout: Duration::from_millis(200),
        ..Limits::DEFAULT
    }
}

struct Server {
    address: SocketAddr,
}

impl Server {
    fn new(limits: Limits, api: Arc<Api>) -> Self {
        let examples = ExampleSet::new(
            CanonicalDir::new(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures")).unwrap(),
        );
        Self::with_ui(limits, api, Ui::Browser(examples))
    }

    fn with_ui(limits: Limits, api: Arc<Api>, ui: Ui) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        // Binding before spawning is the readiness condition: no port probes or retries.
        std::thread::spawn(move || serve(listener, &*api, ui, limits).unwrap());
        Self { address }
    }

    fn echo(limits: Limits) -> Self {
        Self::new(limits, Arc::new(|body| Ok(body.to_owned())))
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect_timeout(&self.address, CLIENT_TIMEOUT).unwrap();
        // Client deadlines only fail a stuck test; they are not success conditions.
        stream.set_read_timeout(Some(CLIENT_TIMEOUT)).unwrap();
        stream.set_write_timeout(Some(CLIENT_TIMEOUT)).unwrap();
        stream
    }

    fn post(&self, host: &str, media_type: &str, origin: Option<&str>) -> String {
        let origin = origin
            .map(|origin| format!("Origin: {origin}\r\n"))
            .unwrap_or_default();
        format!(
            "POST /api HTTP/1.1\r\nHost: {host}\r\nContent-Type: {media_type}\r\n{origin}Content-Length: {}\r\n\r\n{BODY}",
            BODY.len()
        )
    }

    fn normal_post(&self) -> String {
        self.post(&self.address.to_string(), "application/json", None)
    }

    fn get(&self, path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: {}\r\n\r\n", self.address)
    }

    fn request(&self, request: &str) -> Response {
        let mut stream = self.connect();
        stream.write_all(request.as_bytes()).unwrap();
        response(&mut stream)
    }
}

struct Response {
    status: u16,
    body: String,
}

fn response(stream: &mut TcpStream) -> Response {
    let mut bytes = Vec::new();
    // EOF, not a guessed read size, marks the end of this Connection: close answer.
    stream.read_to_end(&mut bytes).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").expect("HTTP response head");
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap();
    let mut parts = status_line.split_whitespace();
    assert_eq!(parts.next(), Some("HTTP/1.1"));
    let status = parts.next().unwrap().parse().unwrap();
    let length = lines
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .expect("response Content-Length")
        .parse::<usize>()
        .unwrap();
    assert_eq!(body.len(), length, "complete response body");
    Response {
        status,
        body: body.to_owned(),
    }
}

fn assert_answer(answer: Response) {
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, BODY);
}

#[test]
fn json_media_type_parameters_are_accepted() {
    let server = Server::echo(limits());
    assert_answer(server.request(&server.post(
        &server.address.to_string(),
        "application/json; charset=utf-8",
        None,
    )));
}

#[test]
fn json_media_type_is_case_insensitive() {
    let server = Server::echo(limits());
    assert_answer(server.request(&server.post(
        &server.address.to_string(),
        "ApPlIcAtIoN/JsOn",
        None,
    )));
}

#[test]
fn localhost_with_bound_port_and_matching_origin_are_accepted() {
    let server = Server::echo(limits());
    let host = format!("localhost:{}", server.address.port());
    assert_answer(server.request(&server.post(
        &host,
        "application/json",
        Some(&format!("http://{host}")),
    )));
}

#[test]
fn text_plain_is_forbidden() {
    let server = Server::echo(limits());
    let request = server.post(&server.address.to_string(), "text/plain", None);
    assert_eq!(server.request(&request).status, 403);
}

#[test]
fn foreign_origin_is_forbidden() {
    let server = Server::echo(limits());
    let request = server.post(
        &server.address.to_string(),
        "application/json",
        Some("http://foreign.example"),
    );
    assert_eq!(server.request(&request).status, 403);
}

#[test]
fn origin_must_match_host_not_just_another_loopback_name() {
    let server = Server::echo(limits());
    let request = server.post(
        &format!("localhost:{}", server.address.port()),
        "application/json",
        Some(&format!("http://{}", server.address)),
    );
    assert_eq!(server.request(&request).status, 403);
}

#[test]
fn foreign_host_is_forbidden_on_api() {
    let server = Server::echo(limits());
    let request = server.post("foreign.example", "application/json", None);
    assert_eq!(server.request(&request).status, 403);
}

#[test]
fn foreign_host_is_forbidden_on_example_index() {
    let server = Server::echo(limits());
    assert_eq!(
        server
            .request("GET /examples/index.json HTTP/1.1\r\nHost: foreign.example\r\n\r\n")
            .status,
        403
    );
}

#[test]
fn oversized_header_is_431_before_the_head_finishes() {
    let limits = Limits {
        head: 256,
        ..limits()
    };
    let server = Server::echo(limits);
    let mut request = format!("GET / HTTP/1.1\r\nHost: {}\r\nX-Padding: ", server.address);
    request.push_str(&"x".repeat(limits.head - request.len()));
    assert_eq!(request.len(), limits.head);
    assert_eq!(server.request(&request).status, 431);
}

#[test]
fn oversized_content_length_is_413_before_any_body_arrives() {
    let server = Server::echo(Limits {
        body: 8,
        ..limits()
    });
    let request = format!(
        "POST /api HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: 9\r\n\r\n",
        server.address
    );
    assert_eq!(server.request(&request).status, 413);
}

#[test]
fn post_without_content_length_is_400() {
    let server = Server::echo(limits());
    let request = format!(
        "POST /api HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\r\n",
        server.address
    );
    assert_eq!(server.request(&request).status, 400);
}

#[test]
fn transfer_encoding_is_400() {
    let server = Server::echo(limits());
    let request = format!(
        "POST /api HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nContent-Length: 2\r\n\r\n",
        server.address
    );
    assert_eq!(server.request(&request).status, 400);
}

#[test]
fn silent_and_partial_clients_do_not_block_another_post_and_remain_open() {
    let server = Server::echo(Limits {
        timeout: Duration::from_secs(60),
        ..limits()
    });
    let mut silent = server.connect();
    let mut partial = server.connect();
    let prefix = "POST /api "; // Half of the request line, without its HTTP version.
    partial.write_all(prefix.as_bytes()).unwrap();

    let request = server.normal_post();
    assert_answer(server.request(&request));

    // Each slow connection must still answer: dropping it on a timeout is not independence.
    silent.write_all(request.as_bytes()).unwrap();
    assert_answer(response(&mut silent));
    partial
        .write_all(request.strip_prefix(prefix).unwrap().as_bytes())
        .unwrap();
    assert_answer(response(&mut partial));
}

#[test]
fn idle_connection_read_ends_under_the_server_timeout() {
    let server = Server::echo(limits());
    let mut idle = server.connect();
    let mut bytes = Vec::new();
    // An error/timeout on the client fails; only server EOF satisfies this condition.
    idle.read_to_end(&mut bytes).unwrap();
}

#[test]
fn page_and_example_index_respond_while_an_announced_api_call_is_held() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (permission_tx, permission_rx) = mpsc::channel();
    let permission_rx = Mutex::new(permission_rx);
    let api: Arc<Api> = Arc::new(move |body| {
        entered_tx
            .send(body.to_owned())
            .map_err(|e| e.to_string())?;
        permission_rx
            .lock()
            .unwrap()
            .recv_timeout(CLIENT_TIMEOUT)
            .map_err(|e| e.to_string())?;
        Ok(body.to_owned())
    });
    let server = Server::new(limits(), api);
    let mut held = server.connect();
    held.write_all(server.normal_post().as_bytes()).unwrap();
    assert_eq!(entered_rx.recv_timeout(CLIENT_TIMEOUT).unwrap(), BODY);

    let page = server.request(&server.get("/"));
    assert_eq!(page.status, 200);
    assert!(page.body.contains("<html"));
    let index = server.request(&server.get("/examples/index.json"));
    assert_eq!(index.status, 200);
    let index: serde_json::Value = serde_json::from_str(&index.body).unwrap();
    assert_eq!(index["modules"]["Theory"], "Theory.ali");

    permission_tx.send(()).unwrap();
    assert_answer(response(&mut held));
}

#[test]
fn headless_root_still_checks_host() {
    let server = Server::with_ui(limits(), Arc::new(|body| Ok(body.to_owned())), Ui::Headless);
    let root = server.request(&server.get("/"));
    assert_eq!(root.status, 200);
    assert!(root.body.contains("headless alifib server"));
    assert_eq!(
        server.request("GET / HTTP/1.1\r\nHost: foreign.example\r\n\r\n").status,
        403
    );
}

#[test]
fn headless_example_index_is_not_found() {
    let server = Server::with_ui(limits(), Arc::new(|body| Ok(body.to_owned())), Ui::Headless);
    let index = server.request(&server.get("/examples/index.json"));
    assert_eq!(index.status, 404);
    assert_eq!(index.body, "not found");
}
