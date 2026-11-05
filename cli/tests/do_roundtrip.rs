//! Real `do` processes against bound, in-process servers.

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use alifib::interactive::kernel::{CanonicalDir, FsAccess, Kernel, Reply, Status};
use alifib::interactive::rpc::Rpc;
use alifib_cli::examples::{ExampleSet, mirror};
use alifib_cli::server::{self, Api, Limits, Ui};

const DEADLINE: Duration = Duration::from_secs(5);

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures")
}

struct Server(SocketAddr);

impl Server {
    fn run(dir: &Path, ui: impl FnOnce(CanonicalDir) -> Ui) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let dir = CanonicalDir::new(dir).unwrap();
        let ui = ui(dir.clone());
        // The listener is already bound: no readiness sleeps or probes.
        std::thread::spawn(move || server::run(listener, dir, ui, Limits::DEFAULT).unwrap());
        Self(address)
    }

    fn headless(dir: &Path) -> Self {
        Self::run(dir, |_| Ui::Headless)
    }

    fn browser(dir: &Path) -> Self {
        Self::run(dir, |dir| Ui::Browser(ExampleSet::new(dir)))
    }

    fn gated(api: Arc<Api>, limits: Limits) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || server::serve(listener, &*api, Ui::Headless, limits).unwrap());
        Self(address)
    }

    fn command(&self, line: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_alifib"));
        command.args(["do", "--at", &self.0.to_string(), line]);
        command
    }

    fn get(&self, path: &str) -> (u16, String) {
        let mut stream = TcpStream::connect_timeout(&self.0, DEADLINE).unwrap();
        stream.set_read_timeout(Some(DEADLINE)).unwrap();
        stream.set_write_timeout(Some(DEADLINE)).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {}\r\n\r\n", self.0).unwrap();
        let mut text = String::new();
        // Connection: close EOF is the response-complete condition.
        stream.read_to_string(&mut text).unwrap();
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        let length: usize = head
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(length, body.len());
        (status, body.to_owned())
    }
}

fn successful(output: Output) -> String {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn plain_success(replies: Vec<Reply>) -> String {
    replies
        .into_iter()
        .map(|reply| {
            assert!(reply.status == Status::Ok);
            format!(
                "{}\n",
                reply
                    .rendered
                    .map(|text| text.to_string())
                    .or(reply.message)
                    .unwrap()
            )
        })
        .collect()
}

#[test]
fn do_roundtrip_load_start_apply_and_headless_note() {
    let server = Server::headless(&fixtures());
    let line = "load RewriteFill.ali; start T p q; apply 0";
    let stdout = successful(server.command(line).output().unwrap());
    let mut expected = Kernel::new(FsAccess::Within(CanonicalDir::new(&fixtures()).unwrap()));
    assert_eq!(stdout, plain_success(expected.eval(line)));
    assert!(stdout.contains("Loaded "));
    assert!(stdout.contains("step: 0\ncurrent: p\ntarget: q"));
    assert!(stdout.contains("[0] r"));
    assert!(stdout.contains("step: 1\ncurrent: q\ntarget: q ✓ reached"));
    assert_eq!(server.get("/"), (200, "headless alifib server — drive it with `alifib do`, or start `alifib web` for the browser".to_owned()));
}

#[test]
fn browser_serves_page_and_fixture_index() {
    let server = Server::browser(&fixtures());
    let (status, page) = server.get("/");
    assert_eq!(status, 200);
    assert!(page.contains("<html"));
    assert!(page.contains("window.ALIFIB_CONFIG = { backend: 'http'"));
    let (status, index) = server.get("/examples/index.json");
    assert_eq!(status, 200);
    assert_eq!(
        index,
        ExampleSet::new(CanonicalDir::new(&fixtures()).unwrap())
            .scan()
            .unwrap()
            .index_json()
    );
    let index: serde_json::Value = serde_json::from_str(&index).unwrap();
    assert_eq!(index["modules"]["RewriteFill"], "RewriteFill.ali");
    assert_eq!(index["modules"]["EmbeddedHole"], "EmbeddedHole.ali");
}

#[test]
fn do_syntax_error_prints_plain_ariadne_report() {
    let server = Server::headless(&fixtures());
    let mut kernel = Kernel::new(FsAccess::Within(CanonicalDir::new(&fixtures()).unwrap()));
    let reply = kernel.open("EmbeddedHole.ali");
    assert!(reply.status == Status::Error);
    assert!(!reply.diagnostics.is_empty());
    let expected: String = reply
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.render(false))
        .collect();
    let output = server.command("load EmbeddedHole.ali").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    assert!(expected.contains("EmbeddedHole.ali"));
    assert!(!expected.contains('\u{1b}'));
}

#[test]
fn do_waits_for_work_beyond_the_server_io_timeout() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let rpc = Mutex::new(Rpc::new(
        Kernel::new(FsAccess::Unrestricted),
        "gate".to_owned(),
    ));
    let api: Arc<Api> = Arc::new(move |body| {
        entered_tx
            .send((Instant::now(), body.to_owned()))
            .map_err(|e| e.to_string())?;
        release_rx
            .lock()
            .unwrap()
            .recv_timeout(DEADLINE)
            .map_err(|e| e.to_string())?;
        Ok(rpc.lock().unwrap().call(body))
    });
    let server = Server::gated(
        api,
        Limits {
            timeout: Duration::from_millis(50),
            ..Limits::DEFAULT
        },
    );
    let mut child = server
        .command("help")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let (announced, request) = entered_rx.recv_timeout(DEADLINE).unwrap();
    assert_help_request(&request);
    // Intentional computation hold, measured from the gate announcement, NOT readiness.
    let hold = Duration::from_millis(200);
    std::thread::sleep(hold.saturating_sub(announced.elapsed()));
    assert!(announced.elapsed() >= hold);
    assert!(
        child.try_wait().unwrap().is_none(),
        "do must still be waiting for the answer"
    );
    release_tx.send(()).unwrap();
    let stdout = successful(child.wait_with_output().unwrap());
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    assert_eq!(stdout, plain_success(kernel.eval("help")));
}

fn assert_help_request(request: &str) {
    let request: serde_json::Value = serde_json::from_str(request).unwrap();
    assert_eq!(request["method"], "eval");
    assert_eq!(request["args"]["line"], "help");
}

#[test]
fn do_reports_http_500_status_and_body_on_stderr_and_exits_one() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let api: Arc<Api> = Arc::new(move |body| {
        entered_tx
            .send(body.to_owned())
            .map_err(|e| e.to_string())?;
        Err("test gate failed".to_owned())
    });
    let server = Server::gated(api, Limits::DEFAULT);
    let output = server.command("help").output().unwrap();
    assert_help_request(&entered_rx.recv_timeout(DEADLINE).unwrap());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "HTTP/1.1 500 Internal Server Error\ntest gate failed\n"
    );
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "alifib-do-roundtrip-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn copy_tree(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), dest.join(entry.file_name())).unwrap();
        }
    }
}

fn assert_mirror_index(server: &Server, src: &Path, dest: &Path) {
    let (status, index) = server.get("/examples/index.json");
    assert_eq!(status, 200);
    assert_eq!(fs::read_to_string(dest.join("index.json")).unwrap(), index);
    let index: serde_json::Value = serde_json::from_str(&index).unwrap();
    for path in index["modules"].as_object().unwrap().values() {
        let path = path.as_str().unwrap();
        assert_eq!(
            fs::read(dest.join(path)).unwrap(),
            fs::read(src.join(path)).unwrap()
        );
    }
}

#[test]
fn static_examples_mirror_matches_http_index_and_replaces_stale_output() {
    let temp = TempDir::new();
    let src = temp.0.join("source");
    let dest = temp.0.join("output");
    copy_tree(&fixtures(), &src);
    fs::write(src.join("2Cat.ali"), "@Type\nCat <<= { pt }\n").unwrap();
    fs::write(src.join("map.ali"), "keyword stem must be skipped\n").unwrap();
    let server = Server::browser(&src);
    mirror(&src, &dest).unwrap();
    assert_mirror_index(&server, &src, &dest);
    let index: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dest.join("index.json")).unwrap()).unwrap();
    assert_eq!(index["modules"]["2Cat"], "2Cat.ali");
    assert!(index["modules"].get("map").is_none());
    assert!(!dest.join("map.ali").exists());
    fs::write(dest.join("stale.ali"), "stale output").unwrap();
    fs::remove_file(src.join("2Cat.ali")).unwrap();
    mirror(&src, &dest).unwrap();
    assert!(!dest.join("stale.ali").exists());
    assert!(!dest.join("2Cat.ali").exists());
    assert_mirror_index(&server, &src, &dest);
}

#[test]
fn unicode_names_are_served() {
    let temp = TempDir::new();
    let delta = "@Type\nOb <<= { ob }\n";
    fs::write(temp.0.join("Δ.ali"), delta).unwrap();
    fs::write(
        temp.0.join("2Cat.ali"),
        "@Type\ninclude Δ,\nCat <<= { attach Ob :: Δ.Ob }\n",
    )
    .unwrap();
    let server = Server::browser(&temp.0);
    let (status, index) = server.get("/examples/index.json");
    assert_eq!(status, 200);
    let index: serde_json::Value = serde_json::from_str(&index).unwrap();
    assert_eq!(
        index,
        serde_json::json!({"modules": {"Δ": "Δ.ali", "2Cat": "2Cat.ali"}})
    );
    assert_eq!(server.get("/examples/%CE%94.ali"), (200, delta.to_owned()));
    assert_eq!(server.get("/examples/%E.ali").0, 400);
    let stdout = successful(server.command("load 2Cat.ali").output().unwrap());
    assert_eq!(
        stdout,
        format!(
            "Loaded {}\n",
            fs::canonicalize(temp.0.join("2Cat.ali")).unwrap().display()
        )
    );
}

fn mirror_source(temp: &TempDir) -> PathBuf {
    let src = temp.0.join("source");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("Keep.ali"), "@Type\nKeep <<= { pt }\n").unwrap();
    src
}

fn assert_mirror_refused(src: &Path, dest: &Path, sentinels: &[PathBuf]) {
    let before: Vec<_> = sentinels
        .iter()
        .map(|path| fs::read(path).unwrap())
        .collect();
    assert!(
        mirror(src, dest).is_err(),
        "must refuse {} -> {}",
        src.display(),
        dest.display()
    );
    for (path, bytes) in sentinels.iter().zip(before) {
        assert_eq!(
            fs::read(path).unwrap(),
            bytes,
            "preserve {}",
            path.display()
        );
    }
}

#[test]
fn static_examples_mirror_refuses_same_directory() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    assert_mirror_refused(&src, &src, &[src.join("Keep.ali")]);
}

#[test]
fn static_examples_mirror_refuses_destination_ancestor() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let sentinel = temp.0.join("destination-sentinel");
    fs::write(&sentinel, "keep ancestor contents").unwrap();
    assert_mirror_refused(&src, &temp.0, &[src.join("Keep.ali"), sentinel]);
}

#[test]
fn static_examples_mirror_refuses_existing_destination_descendant() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let dest = src.join("output");
    fs::create_dir(&dest).unwrap();
    fs::write(dest.join("sentinel"), "keep descendant contents").unwrap();
    assert_mirror_refused(&src, &dest, &[src.join("Keep.ali"), dest.join("sentinel")]);
}

#[test]
fn static_examples_mirror_refuses_missing_nested_destination_descendant() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let dest = src.join("missing/nested/output");
    assert_mirror_refused(&src, &dest, &[src.join("Keep.ali")]);
    assert!(!src.join("missing").exists());
}

#[test]
fn static_examples_mirror_refuses_unresolved_parent_component() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let dest = temp.0.join("output");
    fs::create_dir(&dest).unwrap();
    fs::write(dest.join("sentinel"), "keep disjoint destination").unwrap();
    assert_mirror_refused(
        &src,
        &temp.0.join("missing/../output"),
        &[src.join("Keep.ali"), dest.join("sentinel")],
    );
    assert!(!temp.0.join("missing").exists());
}

#[test]
fn static_examples_mirror_refuses_file_destination_and_nondirectory_ancestor() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let file = temp.0.join("file");
    fs::write(&file, "must remain a file").unwrap();
    for dest in [&file, &file.join("missing/output")] {
        assert_mirror_refused(&src, dest, &[src.join("Keep.ali"), file.clone()]);
    }
}

#[cfg(unix)]
#[test]
fn static_examples_mirror_refuses_source_alias_overlap() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let alias = temp.0.join("source-alias");
    std::os::unix::fs::symlink(&src, &alias).unwrap();
    assert_mirror_refused(&alias, &src, &[src.join("Keep.ali")]);
    assert_eq!(fs::read_link(&alias).unwrap(), src);
}

#[cfg(unix)]
#[test]
fn static_examples_mirror_refuses_destination_parent_alias_overlap() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let alias = temp.0.join("parent-alias");
    std::os::unix::fs::symlink(&src, &alias).unwrap();
    assert_mirror_refused(&src, &alias.join("missing/output"), &[src.join("Keep.ali")]);
    assert_eq!(fs::read_link(&alias).unwrap(), src);
    assert!(!src.join("missing").exists());
}

#[cfg(unix)]
#[test]
fn static_examples_mirror_refuses_destination_symlink() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let target = temp.0.join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("sentinel"), "keep symlink target").unwrap();
    let dest = temp.0.join("destination-link");
    std::os::unix::fs::symlink(&target, &dest).unwrap();
    for path in [
        dest.clone(),
        dest.join("."),
        PathBuf::from(format!("{}/", dest.display())),
    ] {
        assert_mirror_refused(
            &src,
            &path,
            &[src.join("Keep.ali"), target.join("sentinel")],
        );
        assert_eq!(fs::read_link(&dest).unwrap(), target);
    }
}

#[cfg(unix)]
#[test]
fn static_examples_mirror_refuses_dangling_destination_and_ancestor_symlinks() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let target = temp.0.join("missing-target");
    let dest = temp.0.join("dangling-link");
    std::os::unix::fs::symlink(&target, &dest).unwrap();
    for path in [&dest, &dest.join("nested/output")] {
        assert_mirror_refused(&src, path, &[src.join("Keep.ali")]);
        assert_eq!(fs::read_link(&dest).unwrap(), target);
        assert!(!target.exists());
    }
}

#[test]
fn static_examples_mirror_accepts_disjoint_missing_nested_destination() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let dest = temp.0.join("missing/nested/output");
    mirror(&src, &dest).unwrap();
    assert_eq!(
        fs::read(dest.join("Keep.ali")).unwrap(),
        fs::read(src.join("Keep.ali")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(dest.join("index.json")).unwrap(),
        r#"{"modules":{"Keep":"Keep.ali"}}"#
    );
}

#[test]
fn static_examples_mirror_refuses_nonempty_destination_that_is_not_a_mirror() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let dest = temp.0.join("unrelated");
    fs::create_dir(&dest).unwrap();
    fs::write(dest.join("sentinel"), "keep unrelated contents").unwrap();
    assert_mirror_refused(&src, &dest, &[src.join("Keep.ali"), dest.join("sentinel")]);
    // An index alone does not make a mirror of a directory holding other files.
    fs::write(dest.join("index.json"), r#"{"modules":{}}"#).unwrap();
    assert_mirror_refused(&src, &dest, &[src.join("Keep.ali"), dest.join("sentinel")]);
}

#[test]
fn static_examples_mirror_accepts_sibling_prefix_and_replaces_an_earlier_mirror() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let dest = temp.0.join("source-output");
    fs::create_dir(&dest).unwrap();
    fs::write(dest.join("index.json"), r#"{"modules":{}}"#).unwrap();
    fs::create_dir(dest.join("Old")).unwrap();
    fs::write(dest.join("Old/Stale.ali"), "must disappear").unwrap();
    mirror(&src, &dest).unwrap();
    assert!(!dest.join("Old").exists());
    assert_eq!(
        fs::read(dest.join("Keep.ali")).unwrap(),
        fs::read(src.join("Keep.ali")).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn static_examples_mirror_accepts_disjoint_source_and_parent_aliases() {
    let temp = TempDir::new();
    let src = mirror_source(&temp);
    let src_alias = temp.0.join("source-alias");
    std::os::unix::fs::symlink(&src, &src_alias).unwrap();
    let parent = temp.0.join("disjoint");
    fs::create_dir(&parent).unwrap();
    let parent_alias = temp.0.join("parent-alias");
    std::os::unix::fs::symlink(&parent, &parent_alias).unwrap();
    let dest = parent_alias.join("missing/output");
    mirror(&src_alias, &dest).unwrap();
    assert_eq!(
        fs::read(parent.join("missing/output/Keep.ali")).unwrap(),
        fs::read(src.join("Keep.ali")).unwrap()
    );
    assert_eq!(fs::read_link(&parent_alias).unwrap(), parent);
    assert_eq!(fs::read_link(&src_alias).unwrap(), src);
}
