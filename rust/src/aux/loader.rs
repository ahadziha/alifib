use std::sync::Arc;
use super::path;

#[derive(Debug, Clone)]
pub enum LoadError {
    NotFound,
    IoError(String),
}

#[derive(Clone)]
pub struct FileLoader {
    pub search_paths: Vec<String>,
    pub read_file: Arc<dyn Fn(&str) -> Result<String, LoadError> + Send + Sync>,
}

impl FileLoader {
    pub fn default_read(path: &str) -> Result<String, LoadError> {
        if !std::path::Path::new(path).exists() {
            return Err(LoadError::NotFound);
        }
        std::fs::read_to_string(path).map_err(|e| LoadError::IoError(e.to_string()))
    }
}

pub fn ensure_root_in_loader(loader: &FileLoader, canonical_path: &str) -> FileLoader {
    let root = std::path::Path::new(canonical_path)
        .parent()
        .and_then(|p| p.to_str())
        .map(path::canonicalize)
        .unwrap_or_else(|| canonical_path.to_owned());
    let mut desired = vec![root];
    desired.extend(loader.search_paths.iter().cloned());
    let normalized = path::normalize_search_paths(desired);
    if normalized == loader.search_paths {
        loader.clone()
    } else {
        FileLoader {
            search_paths: normalized,
            read_file: loader.read_file.clone(),
        }
    }
}

pub struct Loader {
    inner: FileLoader,
}

impl Loader {
    fn path_separator() -> char {
        if cfg!(windows) { ';' } else { ':' }
    }

    fn split_paths(value: &str) -> Vec<String> {
        value.split(Self::path_separator())
            .filter(|p| !p.is_empty())
            .map(|p| p.to_owned())
            .collect()
    }

    fn env_search_paths() -> Vec<String> {
        match std::env::var("ALIFIB_PATH") {
            Ok(value) if !value.is_empty() => Self::split_paths(&value),
            _ => vec![],
        }
    }

    pub fn default(extra_search_paths: Vec<String>) -> Self {
        let cwd = path::canonicalize(&std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_owned()));
        let env_paths = Self::env_search_paths();
        let combined: Vec<String> = std::iter::once(cwd)
            .chain(env_paths)
            .chain(extra_search_paths)
            .collect();
        let search_paths = path::normalize_search_paths(combined);
        let read_file: Arc<dyn Fn(&str) -> Result<String, LoadError> + Send + Sync> =
            Arc::new(FileLoader::default_read);
        Self { inner: FileLoader { search_paths, read_file } }
    }

    pub fn file_loader(&self) -> &FileLoader {
        &self.inner
    }
}
