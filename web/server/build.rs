//! Builds the frontend bundle (`web/frontend/dist/app.js`) that `lib.rs`
//! `include_str!`s.  Runs `npm install` (if `node_modules` is missing) and
//! `npm run build` before the Rust crate is compiled.
//!
//! When Node.js is unavailable, or the build fails, we emit a cargo warning
//! and leave a stub `dist/app.js` so the crate still compiles — useful for
//! `cargo test --workspace` on machines without a frontend toolchain.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let frontend = manifest_dir
        .join("..")
        .join("frontend")
        .canonicalize()
        .expect("web/frontend directory must exist");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", frontend.join("package.json").display());
    let lockfile = frontend.join("package-lock.json");
    if lockfile.exists() {
        println!("cargo:rerun-if-changed={}", lockfile.display());
    }
    rerun_dir(&frontend.join("src"));

    if let Err(err) = build_frontend(&frontend, lockfile.exists()) {
        println!(
            "cargo:warning=alifib-web-server: frontend bundle not built ({}); \
             serving a stub — install Node.js and rebuild for the real GUI",
            err
        );
        ensure_stub(&frontend.join("dist").join("app.js"));
    }
}

fn build_frontend(frontend: &Path, has_lockfile: bool) -> Result<(), String> {
    let npm = find_npm()?;

    if !frontend.join("node_modules").is_dir() {
        // Prefer `npm ci` when a lockfile is committed — reproducible and
        // refuses to mutate package.json.  Fall back to `npm install` for the
        // bootstrap case where the lockfile is being generated.
        let install_args: &[&str] = if has_lockfile { &["ci"] } else { &["install"] };
        run(
            npm_command(&npm).args(install_args).current_dir(frontend),
            &format!("npm {}", install_args.join(" ")),
        )?;
    }
    run(
        npm_command(&npm).args(["run", "build"]).current_dir(frontend),
        "npm run build",
    )
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
                 Install Node.js and rebuild `alifib-web-server` to produce the real bundle.\n";
    if let Err(e) = std::fs::write(path, stub) {
        println!("cargo:warning=could not write stub {}: {}", path.display(), e);
    }
}

fn rerun_dir(dir: &Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rerun_dir(&path);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

fn find_npm() -> Result<PathBuf, String> {
    if let Ok(path) = which("npm") {
        return Ok(path);
    }
    // nvm installs aren't on PATH for non-login shells (like the one cargo
    // spawns), so fall back to the highest-versioned ~/.nvm install.
    if let Some(home) = std::env::var_os("HOME") {
        let nvm = PathBuf::from(home).join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(&nvm) {
            let mut versions: Vec<_> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join("bin/npm").is_file())
                .collect();
            versions.sort();
            if let Some(latest) = versions.last() {
                return Ok(latest.join("bin/npm"));
            }
        }
    }
    Err("`npm` not found on PATH or under ~/.nvm/versions/node".to_string())
}

fn which(cmd: &str) -> Result<PathBuf, ()> {
    let path = std::env::var_os("PATH").ok_or(())?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(cmd);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(())
}

/// `npm` shells out to `node`, so its directory must be on PATH.  When npm is
/// found via nvm (rather than PATH), the cargo-spawned environment doesn't
/// have node visible — prepend the bin dir so the child can find it.
fn npm_command(npm: &Path) -> Command {
    let mut cmd = Command::new(npm);
    if let Some(bin_dir) = npm.parent() {
        let existing = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin_dir.to_path_buf()];
        paths.extend(std::env::split_paths(&existing));
        if let Ok(joined) = std::env::join_paths(paths) {
            cmd.env("PATH", joined);
        }
    }
    cmd
}

fn run(cmd: &mut Command, label: &str) -> Result<(), String> {
    let status = cmd
        .status()
        .map_err(|e| format!("failed to spawn {}: {}", label, e))?;
    if !status.success() {
        return Err(format!("{} failed with status {}", label, status));
    }
    Ok(())
}
