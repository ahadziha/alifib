use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use super::path;
use crate::language::{self, Program, Error as LangError};

type ReadFileFn = Arc<dyn Fn(&str) -> Result<String, LoadError> + Send + Sync>;

#[derive(Debug, Clone)]
pub enum LoadError {
    NotFound,
    IoError(String),
}

#[derive(Clone)]
struct FileLoader {
    search_paths: Vec<String>,
    read_file: ReadFileFn,
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
    /// Build a child loader whose search paths prepend `file_path`'s parent directory.
    ///
    /// Search path precedence: a module's own directory is checked first, then the
    /// parent's search paths.  Two modules in different directories that both
    /// include a module by the same name may therefore resolve to different files —
    /// the closest directory wins.  Duplicate paths are removed by
    /// `normalize_search_paths` so re-entering the same directory is a no-op.
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

#[derive(Debug)]
pub enum LoadFileError {
    Load { path: String, cause: LoadError },
    Parse { path: String, source: String, errors: Vec<LangError> },
    ModuleNotFound { module_name: String },
    ModuleIoError { path: String, reason: String },
    Cycle { path: String },
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
        let read_file: ReadFileFn = Arc::new(FileLoader::default_read);
        Self { inner: FileLoader { search_paths, read_file } }
    }

    pub fn load(&self, path: &str) -> Result<LoadedFile, LoadFileError> {
        // Strictly canonicalize up front so the path used as a module ID is
        // always the true canonical path, regardless of how the caller spelled it.
        let canonical_path = super::path::canonicalize_existing(path)
            .map_err(|e| LoadFileError::Load {
                path: path.to_owned(),
                cause: if e.kind() == std::io::ErrorKind::NotFound {
                    LoadError::NotFound
                } else {
                    LoadError::IoError(e.to_string())
                },
            })?;
        let source = (self.inner.read_file)(&canonical_path)
            .map_err(|cause| LoadFileError::Load { path: path.to_owned(), cause })?;
        let file_loader = self.inner.with_parent_dir(&canonical_path);
        let program = language::parse(&source)
            .map_err(|errors| LoadFileError::Parse {
                path: canonical_path.clone(),
                source: source.clone(),
                errors,
            })?;
        let modules = resolve_all_modules(&file_loader, &canonical_path, &program)?;
        Ok(LoadedFile { canonical_path, source, program, modules })
    }
}

// ---------------------------------------------------------------------------
// Pre-resolution of module includes
// ---------------------------------------------------------------------------

/// A parsed dependency module together with its source text.
pub struct ResolvedModule {
    pub program: Program,
    pub source: String,
}

/// Resolution mapping from `(parent canonical path, module name)` to the
/// dependency's canonical path.  This is the only part of the pre-resolution
/// data needed during interpretation.
#[derive(Debug)]
pub struct ModuleResolutions {
    resolutions: HashMap<(String, String), String>,
}

impl ModuleResolutions {
    pub fn empty() -> Self {
        ModuleResolutions { resolutions: HashMap::new() }
    }

    pub fn resolve(&self, parent_path: &str, module_name: &str) -> Option<&str> {
        self.resolutions
            .get(&(parent_path.to_owned(), module_name.to_owned()))
            .map(|s| s.as_str())
    }

    fn insert(&mut self, parent_path: &str, module_name: String, canonical_path: String) {
        self.resolutions.insert((parent_path.to_owned(), module_name), canonical_path);
    }
}

/// All pre-resolved dependency modules for a loaded file.
pub struct ModuleStore {
    modules: HashMap<String, ResolvedModule>,
    resolutions: ModuleResolutions,
    /// Canonical paths of dependency modules in the order they must be
    /// interpreted (leaves first, root excluded).  Populated in post-order
    /// by `resolve_recursive`.
    dep_order: Vec<String>,
}

impl ModuleStore {
    fn new() -> Self {
        ModuleStore {
            modules: HashMap::new(),
            resolutions: ModuleResolutions::empty(),
            dep_order: Vec::new(),
        }
    }

    fn has_module(&self, canonical_path: &str) -> bool {
        self.modules.contains_key(canonical_path)
    }

    fn register_resolution(&mut self, parent_path: &str, module_name: String, canonical_path: String) {
        self.resolutions.insert(parent_path, module_name, canonical_path);
    }

    fn insert_module(&mut self, canonical_path: String, program: Program, source: String) {
        self.dep_order.push(canonical_path.clone());
        self.modules.insert(canonical_path, ResolvedModule { program, source });
    }

    /// Consume this store and split it into the resolution mappings (needed
    /// during interpretation) and an ordered list of dependency modules
    /// (needed for the pre-interpretation loop).
    pub fn into_parts(self) -> (ModuleResolutions, Vec<(String, ResolvedModule)>) {
        let ModuleStore { mut modules, resolutions, dep_order } = self;
        let dep_order_modules = dep_order.into_iter()
            .filter_map(|path| {
                let module = modules.remove(&path)?;
                Some((path, module))
            })
            .collect();
        (resolutions, dep_order_modules)
    }
}

fn find_file(loader: &FileLoader, module_name: &str) -> Result<(String, String), LoadFileError> {
    let filename = format!("{}.ali", module_name);
    for dir in &loader.search_paths {
        let candidate = format!("{}/{}", dir, filename);
        match (loader.read_file)(&candidate) {
            Ok(contents) => {
                // File confirmed readable; strict canonicalization must succeed.
                let canonical = path::canonicalize_existing(&candidate)
                    .map_err(|e| LoadFileError::ModuleIoError {
                        path: candidate,
                        reason: e.to_string(),
                    })?;
                return Ok((canonical, contents));
            }
            Err(LoadError::NotFound) => continue,
            Err(LoadError::IoError(reason)) => {
                return Err(LoadFileError::ModuleIoError { path: candidate, reason });
            }
        }
    }
    Err(LoadFileError::ModuleNotFound { module_name: module_name.to_owned() })
}

fn resolve_all_modules(
    loader: &FileLoader,
    root_path: &str,
    root_program: &Program,
) -> Result<ModuleStore, LoadFileError> {
    let mut store = ModuleStore::new();
    // `visited` tracks all paths encountered in the DFS.  The root is seeded
    // here but never inserted into the store (it is handled separately).  Any
    // path that is in `visited` but not yet in the store is either the root or
    // a module currently on the recursion stack; encountering such a path again
    // is a dependency cycle.
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(root_path.to_owned());
    resolve_recursive(loader, root_path, root_program, &mut store, &mut visited)?;
    Ok(store)
}

fn resolve_recursive(
    loader: &FileLoader,
    parent_path: &str,
    program: &Program,
    store: &mut ModuleStore,
    visited: &mut HashSet<String>,
) -> Result<(), LoadFileError> {
    let includes = language::collect_includes(program);
    for module_name in includes {
        let (canonical_path, contents) = find_file(loader, &module_name)?;

        if store.has_module(&canonical_path) {
            // Already fully resolved; just record the resolution for this parent.
            store.register_resolution(parent_path, module_name, canonical_path);
            continue;
        }

        if !visited.insert(canonical_path.clone()) {
            return Err(LoadFileError::Cycle { path: canonical_path });
        }

        let program = language::parse(&contents).map_err(|errors| LoadFileError::Parse {
            path: canonical_path.clone(),
            source: contents.clone(),
            errors,
        })?;

        let child_loader = loader.with_parent_dir(&canonical_path);
        resolve_recursive(&child_loader, &canonical_path, &program, store, &mut *visited)?;

        // Register resolution only after the module and all its deps are stored,
        // so a stale mapping can never point to a missing program.
        store.insert_module(canonical_path.clone(), program, contents);
        store.register_resolution(parent_path, module_name, canonical_path);
    }
    Ok(())
}
