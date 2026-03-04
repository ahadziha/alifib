use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use super::path;
use crate::language::{self, Program, Error as LangError};

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

// ---------------------------------------------------------------------------
// Pre-resolution of module includes
// ---------------------------------------------------------------------------

pub struct ResolvedModule {
    pub source: String,
    pub program: Program,
}

pub struct ModuleStore {
    modules: HashMap<String, ResolvedModule>,
    resolutions: HashMap<(String, String), String>,
}

pub enum ResolveError {
    NotFound { module_name: String },
    IoError { path: String, reason: String },
    ParseError { path: String, source: String, errors: Vec<LangError> },
    Cycle { path: String },
}

impl ModuleStore {
    fn new() -> Self {
        ModuleStore {
            modules: HashMap::new(),
            resolutions: HashMap::new(),
        }
    }

    pub fn resolve(&self, parent_path: &str, module_name: &str) -> Option<&str> {
        self.resolutions
            .get(&(parent_path.to_owned(), module_name.to_owned()))
            .map(|s| s.as_str())
    }

    pub fn get(&self, canonical_path: &str) -> Option<&ResolvedModule> {
        self.modules.get(canonical_path)
    }
}

fn find_file(loader: &FileLoader, module_name: &str) -> Result<(String, String), ResolveError> {
    let filename = format!("{}.ali", module_name);
    for dir in &loader.search_paths {
        let candidate = format!("{}/{}", dir, filename);
        let canonical = path::canonicalize(&candidate);
        match (loader.read_file)(&canonical) {
            Ok(contents) => return Ok((canonical, contents)),
            Err(LoadError::NotFound) => continue,
            Err(LoadError::IoError(reason)) => {
                return Err(ResolveError::IoError { path: canonical, reason });
            }
        }
    }
    Err(ResolveError::NotFound { module_name: module_name.to_owned() })
}

fn collect_includes(program: &Program) -> Vec<String> {
    use crate::language::ast::{Block, TypeInst};
    let mut names = Vec::new();
    for block in &program.blocks {
        if let Block::TypeBlock(body) = &block.inner {
            for instr in body {
                if let TypeInst::IncludeModule(im) = &instr.inner {
                    names.push(im.name.inner.clone());
                }
            }
        }
    }
    names
}

pub fn resolve_all_modules(
    loader: &FileLoader,
    root_path: &str,
    root_program: &Program,
) -> Result<ModuleStore, ResolveError> {
    let mut store = ModuleStore::new();
    let mut resolving = HashSet::new();
    resolving.insert(root_path.to_owned());
    resolve_recursive(loader, root_path, root_program, &mut store, &mut resolving)?;
    Ok(store)
}

fn resolve_recursive(
    loader: &FileLoader,
    parent_path: &str,
    program: &Program,
    store: &mut ModuleStore,
    resolving: &mut HashSet<String>,
) -> Result<(), ResolveError> {
    let includes = collect_includes(program);
    for module_name in includes {
        let (canonical_path, contents) = find_file(loader, &module_name)?;

        store.resolutions.insert(
            (parent_path.to_owned(), module_name),
            canonical_path.clone(),
        );

        if store.modules.contains_key(&canonical_path) {
            continue;
        }

        if !resolving.insert(canonical_path.clone()) {
            return Err(ResolveError::Cycle { path: canonical_path });
        }

        let program = match language::parse(&contents) {
            Ok(p) => p,
            Err(errors) => {
                return Err(ResolveError::ParseError {
                    path: canonical_path,
                    source: contents,
                    errors,
                });
            }
        };

        let child_loader = ensure_root_in_loader(loader, &canonical_path);
        resolve_recursive(&child_loader, &canonical_path, &program, store, resolving)?;

        store.modules.insert(canonical_path, ResolvedModule {
            source: contents,
            program,
        });
    }
    Ok(())
}
