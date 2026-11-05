//! Path canonicalization and search-path utilities.

use std::path::Path;

/// Return an absolute, canonical path for the given path string.
///
/// Falls back to the original string if the path does not exist or cannot
/// be canonicalized.  Use this only for paths that may not exist yet
/// (e.g., search directories supplied by the user or the environment).
/// For paths that are known to exist, use [`canonicalize_existing`].
pub fn canonicalize(path: &str) -> String {
    Path::new(path)
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_owned())
}

/// Return an absolute, canonical path for a path that is known to exist.
///
/// Unlike [`canonicalize`], this returns an error rather than falling back
/// to the original string.  Use this wherever an inconsistent canonical path
/// would silently produce duplicate entries (e.g., module cache keys).
pub fn canonicalize_existing(path: &str) -> Result<String, std::io::Error> {
    Path::new(path)
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
}
