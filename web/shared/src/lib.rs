//! Runtime example directory for the alifib web GUI.
//!
//! The HTTP server passes its positional `<dir>` to [`ExampleSet::new`] and
//! serves its contents under `/examples/` — the same URL scheme that a static
//! WASM deployment (e.g. GitHub Pages) uses against its own `examples/`
//! folder next to `index.html`.  The frontend therefore fetches examples the
//! same way in both modes.
//!
//! The scan is re-done on every `entries()` call, so files dropped into the
//! directory show up without restarting the server.

use std::path::{Path, PathBuf};

pub struct ExampleSet {
    dir: PathBuf,
}

pub struct ExampleEntry {
    pub name: String,
    pub content: String,
}

impl ExampleSet {
    /// Treat `dir` as the source of `.ali` files.  The directory need not
    /// exist at construction time; scans return empty for missing dirs so
    /// the server can still come up and report the problem lazily.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        ExampleSet { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Fresh scan of the directory.  Returns empty on I/O error or missing
    /// directory (the server keeps running; the dropdown just shows nothing).
    pub fn entries(&self) -> Vec<ExampleEntry> {
        scan_dir(&self.dir).unwrap_or_default()
    }

    /// Just the names (file stems), sorted alphabetically.  Used by
    /// `GET /examples/index.json`.
    pub fn names(&self) -> Vec<String> {
        self.entries().into_iter().map(|e| e.name).collect()
    }

    /// Read a single example by name.  `None` if missing or an I/O error.
    pub fn read(&self, name: &str) -> Option<String> {
        let path = self.dir.join(format!("{}.ali", name));
        std::fs::read_to_string(path).ok()
    }
}

fn scan_dir(dir: &Path) -> std::io::Result<Vec<ExampleEntry>> {
    let mut entries = Vec::new();
    for dirent in std::fs::read_dir(dir)? {
        let path = dirent?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ali") {
            continue;
        }
        let name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_owned(),
            None => continue,
        };
        let content = std::fs::read_to_string(&path)?;
        entries.push(ExampleEntry { name, content });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
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
    fn scan_picks_up_ali_files_only() {
        let dir = tempdir("scan");
        std::fs::write(dir.join("Foo.ali"), "@Type\nFoo <<= { pt }").unwrap();
        std::fs::write(dir.join("ignore.txt"), "not an ali").unwrap();

        let set = ExampleSet::new(&dir);
        let names = set.names();
        assert_eq!(names, vec!["Foo".to_string()]);
        assert!(set.read("Foo").unwrap().contains("Foo <<="));
        assert!(set.read("ignore").is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_directory_is_empty() {
        let set = ExampleSet::new("/nonexistent/path/for/alifib/test");
        assert!(set.entries().is_empty());
        assert!(set.read("anything").is_none());
    }
}
