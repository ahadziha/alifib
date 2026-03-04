use std::path::Path;

/// Return an absolute, canonical path for the given path string.
/// Falls back to the original string on error.
pub fn canonicalize(path: &str) -> String {
    Path::new(path)
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_owned())
}

/// Canonicalize and deduplicate search paths, preserving order.
pub fn normalize_search_paths(paths: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for path in paths {
        let canonical = canonicalize(&path);
        if seen.insert(canonical.clone()) {
            result.push(canonical);
        }
    }
    result
}
