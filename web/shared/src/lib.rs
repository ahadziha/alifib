//! Runtime example directory for the alifib web GUI.
//!
//! The HTTP server points at a root directory via `alifib web [<dir>]` and
//! serves the tree under `/examples/` — the same URL scheme a static WASM
//! deployment (GitHub Pages etc.) uses against its own mirror.
//!
//! # Naming model
//!
//! A file's **stem** (e.g. `Theory` for `Theory.ali`) is its canonical
//! identifier — that's what `include <name>` sees.  Subdirectories are pure
//! UI/organization: `topics/braided/YangBaxter.ali` and `YangBaxter.ali`
//! would both be the single module `YangBaxter`.
//!
//! **Stems are globally unique within the root.**  If two `.ali` files share
//! a stem (case-insensitively), [`ExampleSet::scan`] returns an error listing
//! every offender — the server serves an error for `/examples/index.json`,
//! the deploy workflow fails the build.  No silent shadowing.
//!
//! Every path segment (directory and file stem) must match the language's
//! identifier rule `[A-Za-z_][A-Za-z0-9_]*`.  Anything else is skipped with
//! a warning — it could never be `include`d anyway.
//!
//! # Scan behaviour
//!
//! The scan recurses through subdirectories.  It is re-run on every call,
//! so the server picks up filesystem edits without restarting.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct ExampleSet {
    dir: PathBuf,
}

#[derive(Debug)]
pub struct ExampleEntry {
    /// Bare stem — the value of `include <name>`.
    pub name: String,
    /// POSIX-separated path relative to the root, e.g. `topics/braided/YangBaxter.ali`.
    /// This is what the frontend `fetch`es under `/examples/`.
    pub path: String,
    pub content: String,
}

#[derive(Debug)]
pub enum ScanError {
    /// Two or more files share a stem (case-insensitive).  `paths` is sorted.
    DuplicateStem { name: String, paths: Vec<String> },
    /// Filesystem I/O failure while walking the tree.
    Io(String),
}

impl ExampleSet {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        ExampleSet { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Full recursive scan.  Returns `Err` iff duplicate stems are present or
    /// the root cannot be read; individual `.ali` files with invalid segments
    /// are *skipped* (not errored), since they're inert — they can never be
    /// referenced by `include`.
    pub fn scan(&self) -> Result<Vec<ExampleEntry>, ScanError> {
        // Group path lists by case-folded stem so we can detect duplicates.
        let mut by_stem: HashMap<String, Vec<ExampleEntry>> = HashMap::new();
        walk(&self.dir, &self.dir, &mut |entry| {
            by_stem
                .entry(entry.name.to_ascii_lowercase())
                .or_default()
                .push(entry);
        })
        .map_err(|e| ScanError::Io(e.to_string()))?;

        // Sort stems deterministically; sort each group's paths so the error
        // output (and the success order) is stable across runs/filesystems.
        let mut sorted: Vec<(String, Vec<ExampleEntry>)> = by_stem.into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, entries) in sorted.iter_mut() {
            entries.sort_by(|a, b| a.path.cmp(&b.path));
        }

        // Find any stem with >1 file and report the first one we see.
        if let Some((_, entries)) = sorted.iter().find(|(_, v)| v.len() > 1) {
            return Err(ScanError::DuplicateStem {
                name: entries[0].name.clone(),
                paths: entries.iter().map(|e| e.path.clone()).collect(),
            });
        }

        // Unique stems only — flatten and sort by name.
        let mut out: Vec<ExampleEntry> = sorted.into_iter().flat_map(|(_, v)| v).collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// JSON payload for `GET /examples/index.json`.
    ///
    /// Success shape: `{ "Theory": "Theory.ali", "YangBaxter": "topics/YangBaxter.ali", ... }`.
    /// Error shape:   `{ "error": "..." }`.
    pub fn index_json(&self) -> String {
        match self.scan() {
            Ok(entries) => {
                // Emit as a sorted object so the frontend dropdown is stable.
                let mut map = serde_json::Map::new();
                for e in entries {
                    map.insert(e.name, serde_json::Value::String(e.path));
                }
                serde_json::Value::Object(map).to_string()
            }
            Err(err) => serde_json::json!({ "error": format_scan_error(&err) }).to_string(),
        }
    }

    /// Read an `.ali` file at a POSIX-style relative path.  Validates each
    /// segment against the identifier rule (plus the final `.ali` suffix)
    /// and canonicalizes to ensure the result is inside the root — so path
    /// traversal via `..` or absolute paths is rejected even if the segment
    /// check is bypassed by some exotic OS behavior.
    pub fn read_path(&self, rel: &str) -> Option<String> {
        if !validate_rel_path(rel) {
            return None;
        }
        let abs = self.dir.join(rel);
        // Canonicalize and verify it's still inside the root.  If the root
        // itself can't be canonicalized (doesn't exist yet) we fall back to
        // the lexical check, which `validate_rel_path` already did.
        let root_canon = self.dir.canonicalize().ok();
        let abs_canon = abs.canonicalize().ok();
        if let (Some(r), Some(a)) = (&root_canon, &abs_canon)
            && !a.starts_with(r)
        {
            return None;
        }
        std::fs::read_to_string(&abs).ok()
    }
}

fn format_scan_error(err: &ScanError) -> String {
    match err {
        ScanError::DuplicateStem { name, paths } => format!(
            "duplicate example stem `{}`: {} — rename one of them",
            name,
            paths.join(", ")
        ),
        ScanError::Io(msg) => format!("scanning examples directory failed: {}", msg),
    }
}

fn walk(
    root: &Path,
    dir: &Path,
    visit: &mut dyn FnMut(ExampleEntry),
) -> std::io::Result<()> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        // Missing directory is fine — scan yields an empty index.  Only the
        // root's absence matters (the caller sees an empty object) and isn't
        // worth a hard error: users running `alifib web` in a fresh repo can
        // still load source text from the editor.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && dir == root => return Ok(()),
        Err(e) => return Err(e),
    };
    for dirent in rd {
        let dirent = dirent?;
        let path = dirent.path();
        let ty = dirent.file_type()?;
        if ty.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !is_valid_segment(name) {
                eprintln!(
                    "alifib web: skipping directory {} — segment {:?} is not a valid identifier",
                    path.display(),
                    name,
                );
                continue;
            }
            walk(root, &path, visit)?;
        } else if ty.is_file() {
            if path.extension().and_then(|e| e.to_str()) != Some("ali") {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            if !is_valid_segment(stem) {
                eprintln!(
                    "alifib web: skipping {} — stem {:?} is not a valid identifier",
                    path.display(),
                    stem,
                );
                continue;
            }
            let rel = relative_posix(root, &path);
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("alifib web: skipping {} — {}", path.display(), e);
                    continue;
                }
            };
            visit(ExampleEntry {
                name: stem.to_owned(),
                path: rel,
                content,
            });
        }
        // Symlinks and other file types: ignored.  If a symlink points inside
        // the tree its target is hit directly by the walk; if it points out,
        // ignoring it avoids an unintended escape without an explicit
        // canonicalize-in-root check.
    }
    Ok(())
}

fn relative_posix(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn is_valid_segment(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn validate_rel_path(rel: &str) -> bool {
    if rel.is_empty() {
        return false;
    }
    let Some(without_ali) = rel.strip_suffix(".ali") else {
        return false;
    };
    if without_ali.is_empty() {
        return false;
    }
    for seg in without_ali.split('/') {
        if !is_valid_segment(seg) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("alifib-shared-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn flat_scan_unchanged() {
        let dir = tempdir("flat");
        std::fs::write(dir.join("Theory.ali"), "@Type\nTheory <<= { pt }").unwrap();
        std::fs::write(dir.join("Foo.ali"), "@Type\nFoo <<= { pt }").unwrap();

        let set = ExampleSet::new(&dir);
        let entries = set.scan().unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Foo", "Theory"]);
        // Top-level: path is just the filename.
        let theory = entries.iter().find(|e| e.name == "Theory").unwrap();
        assert_eq!(theory.path, "Theory.ali");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn recursive_scan_with_subdirs() {
        let dir = tempdir("nested");
        std::fs::create_dir_all(dir.join("topics/braided")).unwrap();
        std::fs::write(dir.join("Theory.ali"), "a").unwrap();
        std::fs::write(dir.join("topics/Frobenius.ali"), "b").unwrap();
        std::fs::write(dir.join("topics/braided/YangBaxter.ali"), "c").unwrap();

        let set = ExampleSet::new(&dir);
        let entries = set.scan().unwrap();
        let by_name: HashMap<_, _> = entries.iter().map(|e| (e.name.as_str(), e.path.as_str())).collect();
        assert_eq!(by_name["Theory"], "Theory.ali");
        assert_eq!(by_name["Frobenius"], "topics/Frobenius.ali");
        assert_eq!(by_name["YangBaxter"], "topics/braided/YangBaxter.ali");

        // Server can read each entry by its relative path.
        assert_eq!(set.read_path("Theory.ali").as_deref(), Some("a"));
        assert_eq!(set.read_path("topics/braided/YangBaxter.ali").as_deref(), Some("c"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_stems_error_loudly() {
        let dir = tempdir("dup");
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        std::fs::write(dir.join("a/Foo.ali"), "x").unwrap();
        std::fs::write(dir.join("b/Foo.ali"), "y").unwrap();

        let err = ExampleSet::new(&dir).scan().unwrap_err();
        match err {
            ScanError::DuplicateStem { name, paths } => {
                assert_eq!(name, "Foo");
                assert_eq!(paths, vec!["a/Foo.ali".to_string(), "b/Foo.ali".to_string()]);
            }
            e => panic!("unexpected error: {:?}", e),
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn case_insensitive_dups_caught() {
        let dir = tempdir("case");
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        std::fs::write(dir.join("a/Foo.ali"), "").unwrap();
        std::fs::write(dir.join("b/foo.ali"), "").unwrap();

        let err = ExampleSet::new(&dir).scan().unwrap_err();
        assert!(matches!(err, ScanError::DuplicateStem { .. }));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_segments_skipped_not_errored() {
        let dir = tempdir("invalid");
        std::fs::create_dir_all(dir.join("9weird")).unwrap();
        std::fs::write(dir.join("Good.ali"), "ok").unwrap();
        std::fs::write(dir.join("9weird/Hidden.ali"), "x").unwrap();
        std::fs::write(dir.join("has space.ali"), "x").unwrap();

        let entries = ExampleSet::new(&dir).scan().unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Good"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_path_rejects_traversal() {
        let dir = tempdir("traverse");
        std::fs::write(dir.join("Theory.ali"), "ok").unwrap();
        let set = ExampleSet::new(&dir);
        assert!(set.read_path("../Cargo.toml").is_none());
        assert!(set.read_path("..ali").is_none());
        assert!(set.read_path("/etc/passwd.ali").is_none());
        assert!(set.read_path("Theory").is_none());
        assert!(set.read_path(".ali").is_none());
        assert_eq!(set.read_path("Theory.ali").as_deref(), Some("ok"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_root_is_empty_not_error() {
        let set = ExampleSet::new("/nonexistent/path/for/alifib/test");
        let entries = set.scan().unwrap();
        assert!(entries.is_empty());
    }
}
