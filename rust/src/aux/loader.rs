use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use super::path;
use crate::language::{self, Program, Error as LangError};

#[derive(Debug, Clone)]
pub(crate) enum LoadError {
    NotFound,
    IoError(String),
}

#[derive(Clone)]
struct FileLoader {
    search_paths: Vec<String>,
    read_file: Arc<dyn Fn(&str) -> Result<String, LoadError> + Send + Sync>,
}

impl FileLoader {
    fn default_read(path: &str) -> Result<String, LoadError> {
        if !std::path::Path::new(path).exists() {
            return Err(LoadError::NotFound);
        }
        std::fs::read_to_string(path).map_err(|e| LoadError::IoError(e.to_string()))
    }
}

impl FileLoader {
    fn with_parent_dir(&self, file_path: &str) -> FileLoader {
        let parent = std::path::Path::new(file_path)
            .parent()
            .and_then(|p| p.to_str())
            .map(path::canonicalize)
            .unwrap_or_else(|| file_path.to_owned());
        let mut desired = vec![parent];
        desired.extend(self.search_paths.iter().cloned());
        let normalized = path::normalize_search_paths(desired);
        if normalized == self.search_paths {
            self.clone()
        } else {
            FileLoader {
                search_paths: normalized,
                read_file: self.read_file.clone(),
            }
        }
    }
}

pub struct Loader {
    inner: FileLoader,
}

pub struct LoadedFile {
    pub canonical_path: String,
    pub source: String,
    pub program: Program,
    pub modules: ModuleStore,
}

pub(crate) enum LoadFileError {
    Load { path: String, cause: LoadError },
    Parse { path: String, source: String, errors: Vec<LangError> },
    Resolve(ResolveError),
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

    pub fn load(&self, path: &str) -> Result<LoadedFile, LoadFileError> {
        let canonical_path = super::path::canonicalize(path);
        let source = (self.inner.read_file)(&canonical_path)
            .map_err(|cause| LoadFileError::Load { path: path.to_owned(), cause })?;
        let file_loader = self.inner.with_parent_dir(&canonical_path);
        let program = language::parse(&source)
            .map_err(|errors| LoadFileError::Parse {
                path: canonical_path.clone(),
                source: source.clone(),
                errors,
            })?;
        let modules = resolve_all_modules(&file_loader, &canonical_path, &program)
            .map_err(LoadFileError::Resolve)?;
        Ok(LoadedFile { canonical_path, source, program, modules })
    }
}

// ---------------------------------------------------------------------------
// Pre-resolution of module includes
// ---------------------------------------------------------------------------

pub struct ResolvedModule {
    pub program: Program,
}

pub struct ModuleStore {
    modules: HashMap<String, ResolvedModule>,
    resolutions: HashMap<(String, String), String>,
}

pub(crate) enum ResolveError {
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

    fn has_module(&self, canonical_path: &str) -> bool {
        self.modules.contains_key(canonical_path)
    }

    fn register_resolution(&mut self, parent_path: &str, module_name: String, canonical_path: String) {
        self.resolutions.insert((parent_path.to_owned(), module_name), canonical_path);
    }

    fn insert_module(&mut self, canonical_path: String, program: Program) {
        self.modules.insert(canonical_path, ResolvedModule { program });
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
    program.blocks.iter()
        .filter_map(|b| match &b.inner { Block::TypeBlock(body) => Some(body), _ => None })
        .flat_map(|body| body.iter())
        .filter_map(|i| match &i.inner { TypeInst::IncludeModule(im) => Some(im.name.inner.clone()), _ => None })
        .collect()
}

fn resolve_all_modules(
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

        store.register_resolution(parent_path, module_name, canonical_path.clone());

        if store.has_module(&canonical_path) {
            continue;
        }

        if !resolving.insert(canonical_path.clone()) {
            return Err(ResolveError::Cycle { path: canonical_path });
        }

        let program = language::parse(&contents).map_err(|errors| ResolveError::ParseError {
            path: canonical_path.clone(),
            source: contents,
            errors,
        })?;

        let child_loader = loader.with_parent_dir(&canonical_path);
        resolve_recursive(&child_loader, &canonical_path, &program, store, resolving)?;

        store.insert_module(canonical_path, program);
    }
    Ok(())
}
