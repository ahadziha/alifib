//! Runtime example directory scanning and static mirroring for the alifib web GUI.
//!
//! The HTTP server points at a root directory via `alifib web [<dir>]` and
//! serves the tree under `/examples/` — the same URL scheme a static WASM
//! deployment (GitHub Pages etc.) uses against its own mirror.
//!
//! # Naming model
//!
//! Each example is identified by its POSIX-relative path minus the `.ali`
//! suffix: `Theory` for `Theory.ali`, `TRS/Aux` for `TRS/Aux.ali`.  This
//! is the key in `/examples/index.json` and the label shown in the dropdown.
//!
//! Multiple files may share the same stem (e.g. `TRS/Aux.ali` and
//! `Bicategory/Aux.ali`) — the subdirectory prefix disambiguates them.
//!
//! Every path segment (directory and file stem) must be a name as the lexer
//! reads it. Non-name directories are skipped silently; non-name `.ali` stems
//! are reported to the caller as skipped, since they cannot be `include`d.
//!
//! # Index
//!
//! The index is `{"modules": {<name>: <path>, …}}`. A failed scan is answered
//! `{"error": <message>}` with HTTP 500, so an error is never read as an example.
//!
//! # Scan behaviour
//!
//! The scan recurses through subdirectories.  It is re-run on every call,
//! so the server picks up filesystem edits without restarting.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use alifib::interactive::kernel::CanonicalDir;
use alifib::language::is_name;
use serde_json::{Map, Value, json};

pub struct ExampleSet {
    dir: CanonicalDir,
}

#[derive(Debug)]
pub struct ExampleEntry {
    /// Display name: relative path without the `.ali` suffix.
    /// E.g. `Theory` for top-level, `TRS/Aux` for subdirectory files.
    pub name: String,
    /// POSIX-separated path relative to the root, e.g. `TRS/Aux.ali`.
    pub path: String,
}

/// A `.ali` file the scan left out, because its stem is not a name.
#[derive(Debug)]
pub struct Skipped(pub PathBuf);

impl fmt::Display for Skipped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "skipping {}: its stem is not a name", self.0.display())
    }
}

/// One scan of the tree: its examples sorted by name, and the files it skipped.
#[derive(Debug)]
pub struct Scan {
    pub entries: Vec<ExampleEntry>,
    pub skipped: Vec<Skipped>,
}

impl Scan {
    /// The index, `{"modules": {<name>: <path>, …}}`.
    pub fn index_json(&self) -> String {
        let modules: Map<String, Value> = self
            .entries
            .iter()
            .map(|entry| (entry.name.clone(), Value::from(entry.path.as_str())))
            .collect();
        json!({ "modules": modules }).to_string()
    }
}

/// The body answering a failed scan, sent with HTTP 500.
pub fn index_error_json(error: &io::Error) -> String {
    json!({ "error": format!("scanning examples directory failed: {error}") }).to_string()
}

impl ExampleSet {
    pub fn new(dir: CanonicalDir) -> Self {
        Self { dir }
    }

    /// Scan the whole tree afresh. Only I/O failures are errors; `.ali` files
    /// whose stems are not names are inert, and are returned as skipped.
    pub fn scan(&self) -> io::Result<Scan> {
        let mut scan = Scan {
            entries: Vec::new(),
            skipped: Vec::new(),
        };
        walk(self.dir.path(), "", &mut scan)?;
        scan.entries.sort_by(|a, b| a.name.cmp(&b.name));
        scan.skipped.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(scan)
    }

    /// Read the example at a POSIX path relative to the root. `NotFound` means
    /// there is no such example: a segment is not a name, the canonical path
    /// leaves the root, or the file is missing. Other errors failed to read one.
    pub fn read_path(&self, path: &str) -> io::Result<String> {
        if !is_example_path(path) {
            return Err(io::ErrorKind::NotFound.into());
        }
        let path = self
            .dir
            .readable(path)
            .map_err(|message| io::Error::new(io::ErrorKind::NotFound, message))?;
        fs::read_to_string(path)
    }
}

/// Collect the `.ali` files beneath `dir`, whose POSIX path from the root is
/// `prefix`. Symlinks are not followed: one into the tree is reached directly,
/// and one out of it would escape the root.
fn walk(dir: &Path, prefix: &str, scan: &mut Scan) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            if let Some(name) = path
                .file_name()
                .and_then(OsStr::to_str)
                .filter(|name| is_name(name))
            {
                walk(&path, &format!("{prefix}{name}/"), scan)?;
            }
        } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "ali") {
            match path
                .file_stem()
                .and_then(OsStr::to_str)
                .filter(|stem| is_name(stem))
            {
                Some(stem) => {
                    let name = format!("{prefix}{stem}");
                    scan.entries.push(ExampleEntry {
                        path: format!("{name}.ali"),
                        name,
                    });
                }
                None => scan.skipped.push(Skipped(path)),
            }
        }
    }
    Ok(())
}

/// Whether `path` names an example: name segments, then `.ali`.
fn is_example_path(path: &str) -> bool {
    path.strip_suffix(".ali")
        .is_some_and(|stem| stem.split('/').all(is_name))
}

// Resolve before any mutation: neither tree may contain the other. This does
// not guard against concurrent local filesystem replacement after validation.
fn validate_destination(src: &CanonicalDir, dest: &Path) -> io::Result<PathBuf> {
    let invalid = |message| io::Error::new(io::ErrorKind::InvalidInput, message);
    let absolute = if dest.is_absolute() {
        dest.to_owned()
    } else {
        std::env::current_dir()?.join(dest)
    };
    // Remove trailing separators/`.` without resolving `..`, so `link/` cannot
    // disguise a destination that is itself a symlink as a real directory.
    let absolute: PathBuf = absolute.components().collect();
    let resolved = match fs::symlink_metadata(&absolute) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(invalid(
                    "destination must be a real directory, not a symlink",
                ));
            }
            fs::canonicalize(&absolute)?
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut ancestor = absolute;
            let mut missing = Vec::new();
            loop {
                match fs::symlink_metadata(&ancestor) {
                    Ok(_) => {
                        let mut resolved = fs::canonicalize(&ancestor)?;
                        if !resolved.is_dir() {
                            return Err(invalid("destination ancestor must be a directory"));
                        }
                        for name in missing.into_iter().rev() {
                            resolved.push(name);
                        }
                        break resolved;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        let Some(Component::Normal(name)) = ancestor.components().next_back()
                        else {
                            return Err(invalid(
                                "unresolved destination components must be ordinary names",
                            ));
                        };
                        missing.push(name.to_owned());
                        ancestor.pop();
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Err(error) => return Err(error),
    };
    if resolved.starts_with(src.path()) || src.path().starts_with(&resolved) {
        return Err(invalid("source and destination trees must be disjoint"));
    }
    Ok(resolved)
}

/// Whether `dest` may be replaced: it is missing, or holds nothing a mirror
/// would not write, which covers an empty directory and an earlier mirror.
fn replaceable(dest: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(dest) {
        Ok(_) => holds_only_mirror(dest, true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

/// Whether `dir` holds only `.ali` files, directories of them, and, at the
/// top, the index: what [`mirror`] writes. Symlinks are never a mirror's.
fn holds_only_mirror(dir: &Path, top: bool) -> io::Result<bool> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        let mirrored = if kind.is_dir() {
            holds_only_mirror(&path, false)?
        } else {
            kind.is_file()
                && (path.extension().is_some_and(|extension| extension == "ali")
                    || (top && entry.file_name() == "index.json"))
        };
        if !mirrored {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Replace `dest` with a mirror of `src`'s examples and the index of that same
/// scan, which is returned. `dest` must be disjoint from `src`, and missing,
/// empty, or an earlier mirror; a failed scan leaves it untouched.
pub fn mirror(src: &Path, dest: &Path) -> io::Result<Scan> {
    let src = CanonicalDir::new(src)?;
    let dest = validate_destination(&src, dest)?;
    if !replaceable(&dest)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination must be missing, empty, or an earlier mirror \
             (only index.json and .ali files)",
        ));
    }
    let examples = ExampleSet::new(src);
    let scan = examples.scan()?;
    match fs::remove_dir_all(&dest) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    fs::create_dir_all(&dest)?;
    for entry in &scan.entries {
        let target = dest.join(&entry.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(examples.dir.path().join(&entry.path), target)?;
    }
    fs::write(dest.join("index.json"), scan.index_json())?;
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tempdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "alifib-cli-examples-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn flat_scan_unchanged() {
        let dir = tempdir("flat");
        std::fs::write(dir.join("Theory.ali"), "@Type\nTheory <<= { pt }").unwrap();
        std::fs::write(dir.join("Foo.ali"), "@Type\nFoo <<= { pt }").unwrap();

        let set = ExampleSet::new(CanonicalDir::new(&dir).unwrap());
        let entries = set.scan().unwrap().entries;
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

        let set = ExampleSet::new(CanonicalDir::new(&dir).unwrap());
        let entries = set.scan().unwrap().entries;
        let by_name: std::collections::HashMap<_, _> = entries
            .iter()
            .map(|e| (e.name.as_str(), e.path.as_str()))
            .collect();
        assert_eq!(by_name["Theory"], "Theory.ali");
        assert_eq!(by_name["topics/Frobenius"], "topics/Frobenius.ali");
        assert_eq!(
            by_name["topics/braided/YangBaxter"],
            "topics/braided/YangBaxter.ali"
        );

        assert_eq!(set.read_path("Theory.ali").unwrap(), "a");
        assert_eq!(set.read_path("topics/braided/YangBaxter.ali").unwrap(), "c");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_stems_in_different_dirs_allowed() {
        let dir = tempdir("dup");
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        std::fs::write(dir.join("a/Foo.ali"), "x").unwrap();
        std::fs::write(dir.join("b/Foo.ali"), "y").unwrap();

        let entries = ExampleSet::new(CanonicalDir::new(&dir).unwrap())
            .scan()
            .unwrap()
            .entries;
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a/Foo", "b/Foo"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_segments_skipped_not_errored() {
        let dir = tempdir("invalid");
        std::fs::create_dir_all(dir.join("9weird")).unwrap();
        std::fs::write(dir.join("Good.ali"), "ok").unwrap();
        std::fs::write(dir.join("9weird/Hidden.ali"), "x").unwrap();
        std::fs::write(dir.join("has space.ali"), "x").unwrap();
        std::fs::create_dir_all(dir.join("map")).unwrap();
        std::fs::write(dir.join("map/Hidden.ali"), "x").unwrap();

        let scan = ExampleSet::new(CanonicalDir::new(&dir).unwrap())
            .scan()
            .unwrap();
        let names: Vec<_> = scan.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["9weird/Hidden", "Good"]);
        // A non-name stem is reported; a non-name directory is skipped silently.
        let skipped: Vec<_> = scan
            .skipped
            .iter()
            .map(|skipped| skipped.0.clone())
            .collect();
        assert_eq!(
            skipped,
            vec![
                CanonicalDir::new(&dir)
                    .unwrap()
                    .path()
                    .join("has space.ali")
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_path_rejects_traversal() {
        let dir = tempdir("traverse");
        std::fs::write(dir.join("Theory.ali"), "ok").unwrap();
        let set = ExampleSet::new(CanonicalDir::new(&dir).unwrap());
        for path in [
            "../Cargo.toml",
            "..ali",
            "/etc/passwd.ali",
            "Theory",
            ".ali",
            "Missing.ali",
        ] {
            let error = set.read_path(path).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{path}");
        }
        assert_eq!(set.read_path("Theory.ali").unwrap(), "ok");
        std::fs::remove_dir_all(&dir).ok();
    }
}
