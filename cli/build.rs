//! Builds the frontend bundle (`web/frontend/dist/app.js`) that `src/server.rs`
//! `include_str!`s.  Installs dependencies and runs the `build` script with
//! whichever package manager is available — bun first (our default), npm as a
//! fallback — before the Rust crate is compiled.
//!
//! When neither is available, or the build fails, we emit a cargo warning and
//! leave a stub `dist/app.js` so the crate still compiles — useful for
//! `cargo test --workspace` on machines without a frontend toolchain.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let frontend = manifest_dir
        .join("..")
        .join("web/frontend")
        .canonicalize()
        .expect("web/frontend directory must exist");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", frontend.join("package.json").display());
    for lock in ["bun.lock", "package-lock.json"] {
        let path = frontend.join(lock);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    // Cargo scans a directory whole, including files added later.
    println!("cargo:rerun-if-changed={}", frontend.join("src").display());

    if let Err(err) = build_frontend(&frontend) {
        println!(
            "cargo:warning=alifib-cli: frontend bundle not built ({}); \
             serving a stub — install bun (or Node.js) and rebuild for the real GUI",
            err
        );
        ensure_stub(&frontend.join("dist").join("app.js"));
    }
}

fn build_frontend(frontend: &Path) -> Result<(), String> {
    let pm = PackageManager::find()?;
    pm.install(frontend)?;
    pm.run(frontend, &["run", "build"])
}

/// A JavaScript package manager: which tool it is, and where its binary lives.
/// Bun is preferred — it's what we build the frontend with — with npm kept as
/// a fallback for machines without bun.
struct PackageManager {
    kind: Kind,
    bin: PathBuf,
}

enum Kind {
    Bun,
    Npm,
}

use Kind::{Bun, Npm};

impl PackageManager {
    fn find() -> Result<Self, String> {
        // bun installs to ~/.bun/bin; nvm's npm isn't on PATH for the
        // non-login shell cargo spawns — so each tool gets a known fallback.
        if let Some(bin) = which("bun").or_else(|| home_bin(".bun/bin/bun")) {
            return Ok(Self { kind: Bun, bin });
        }
        if let Some(bin) = which("npm").or_else(find_nvm_npm) {
            return Ok(Self { kind: Npm, bin });
        }
        Err("no `bun` or `npm` found on PATH (nor under ~/.bun or ~/.nvm)".to_string())
    }

    /// Install dependencies reproducibly from the committed lockfile.
    fn install(&self, frontend: &Path) -> Result<(), String> {
        match self.kind {
            // bun's frozen install is its own up-to-date check (~20ms when the
            // tree is in sync); plain `install` bootstraps a missing lockfile.
            Bun => {
                let args: &[&str] = if frontend.join("bun.lock").exists() {
                    &["install", "--frozen-lockfile"]
                } else {
                    &["install"]
                };
                self.run(frontend, args)
            }
            // `npm ci` is reproducible but slow, so gate it on a drift check —
            // editing frontend sources shouldn't trigger a reinstall.
            Npm => {
                if !needs_install(frontend) {
                    return Ok(());
                }
                let args: &[&str] = if frontend.join("package-lock.json").exists() {
                    &["ci"]
                } else {
                    &["install"]
                };
                self.run(frontend, args)
            }
        }
    }

    /// Run the tool with the tool's bin dir prepended to PATH — npm shells out
    /// to `node`, so its directory must be visible (bun is self-contained, but
    /// prepending is harmless).
    fn run(&self, frontend: &Path, args: &[&str]) -> Result<(), String> {
        let bin = &self.bin;
        let mut cmd = Command::new(bin);
        if let Some(dir) = bin.parent() {
            let existing = std::env::var_os("PATH").unwrap_or_default();
            let mut paths = vec![dir.to_path_buf()];
            paths.extend(std::env::split_paths(&existing));
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("PATH", joined);
            }
        }
        cmd.args(args).current_dir(frontend);

        let label = format!("{} {}", bin.display(), args.join(" "));
        let status = cmd
            .status()
            .map_err(|e| format!("failed to spawn {}: {}", label, e))?;
        if !status.success() {
            return Err(format!("{} failed with status {}", label, status));
        }
        Ok(())
    }
}

/// True when `node_modules` is missing, or when `package-lock.json` is newer
/// than the `.package-lock.json` marker npm writes on install — the standard
/// signal that committed deps have drifted from the installed tree.
fn needs_install(frontend: &Path) -> bool {
    if !frontend.join("node_modules").is_dir() {
        return true;
    }
    let lockfile = frontend.join("package-lock.json");
    let marker = frontend.join("node_modules").join(".package-lock.json");
    match (mtime(&lockfile), mtime(&marker)) {
        (Some(lock), Some(mark)) => lock > mark,
        (Some(_), None) => true,
        _ => false,
    }
}

fn mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn ensure_stub(path: &Path) {
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        println!("cargo:warning=could not create {}: {}", parent.display(), e);
        return;
    }
    let stub = b"// alifib frontend bundle missing.  \
                 Install bun (or Node.js) and rebuild `alifib-cli` to produce the real bundle.\n";
    if let Err(e) = std::fs::write(path, stub) {
        println!("cargo:warning=could not write stub {}: {}", path.display(), e);
    }
}

fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(cmd))
        .find(|candidate| candidate.is_file())
}

fn home_bin(rel: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(std::env::var_os("HOME")?).join(rel);
    candidate.is_file().then_some(candidate)
}

/// The npm of the highest Node version under `~/.nvm/versions/node`, if any.
fn find_nvm_npm() -> Option<PathBuf> {
    let nvm = PathBuf::from(std::env::var_os("HOME")?).join(".nvm/versions/node");
    std::fs::read_dir(nvm)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let version = version(entry.file_name().to_str()?)?;
            let npm = entry.path().join("bin/npm");
            npm.is_file().then_some((version, npm))
        })
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, npm)| npm)
}

/// A `vMAJOR.MINOR.PATCH` directory name as numbers, so `v22` outranks `v9`.
fn version(name: &str) -> Option<Vec<u64>> {
    name.strip_prefix('v')?
        .split('.')
        .map(|part| part.parse().ok())
        .collect()
}
