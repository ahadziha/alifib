//! File loading and dependency resolution for alifib source files.
//!
//! The main entry point is [`Loader`], which reads source files, resolves
//! module includes, and returns [`LoadedFile`] values ready for interpretation.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use super::path;
use crate::language::{self, Program, Error as LangError};

#[derive(Debug, Clone)]
pub enum LoadError {
    NotFound,
    IoError(String),
}

/// Files held in memory, over the disk or over nothing.
#[derive(Clone)]
struct Files {
    /// Held files by canonical name, shadowing the disk or standing alone.
    held: Arc<HashMap<String, String>>,
    over_disk: bool,
}

impl Files {
    fn read(&self, path: &str) -> Result<(String, String), LoadError> {
        let canonical = if self.over_disk {
            path::canonicalize_existing(path).map_err(load_error)?
        } else {
            path.to_owned()
        };
        let source = match self.held.get(&canonical) {
            Some(source) => source.clone(),
            None if self.over_disk => std::fs::read_to_string(&canonical).map_err(load_error)?,
            None => return Err(LoadError::NotFound),
        };
        Ok((canonical, source))
    }

    fn dir(&self, path: &str) -> String {
        if self.over_disk { path::canonicalize(path) } else { path.to_owned() }
    }
}

fn load_error(error: std::io::Error) -> LoadError {
    if error.kind() == std::io::ErrorKind::NotFound {
        LoadError::NotFound
    } else {
        LoadError::IoError(error.to_string())
    }
}

/// Resolves and loads source files and their transitive dependencies.
/// Clone cheaply to adjust search paths during dependency resolution.
#[derive(Clone)]
pub struct Loader {
    search_paths: Vec<String>,
    files: Files,
}

pub struct LoadedFile {
    pub canonical_path: String,
    pub source: String,
    pub program: Program,
    /// Resolution mappings for all transitive dependencies.
    pub resolutions: ModuleResolutions,
    /// Dependency modules in topological order (leaves first, root excluded).
    pub dep_modules: Vec<(String, ResolvedModule)>,
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

    /// Search `dir`, then `ALIFIB_PATH`, then `extra_search_paths`.
    /// Relative search paths are resolved against `dir`.
    pub fn in_dir(dir: &str, extra_search_paths: Vec<String>) -> Self {
        let files = Files { held: Arc::new(HashMap::new()), over_disk: true };
        let dir = files.dir(dir);
        let mut seen = HashSet::new();
        let search_paths = std::iter::once(dir.clone())
            .chain(Self::env_search_paths().into_iter().chain(extra_search_paths).map(|entry| {
                std::path::Path::new(&dir).join(entry).to_string_lossy().into_owned()
            }))
            .map(|entry| files.dir(&entry))
            .filter(|entry| seen.insert(entry.clone()))
            .collect();
        Self { search_paths, files }
    }

    /// Like [`Self::in_dir`], with the current working directory as the base.
    pub fn default(extra_search_paths: Vec<String>) -> Self {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_owned());
        Self::in_dir(&cwd, extra_search_paths)
    }

    /// Hold files over nothing, inheriting the top level as a search path.
    pub fn in_memory(files: HashMap<String, String>) -> Self {
        Self {
            search_paths: vec![String::new()],
            files: Files { held: Arc::new(files), over_disk: false },
        }
    }

    /// Hold `source` at its canonical `path`, shadowing any previous contents.
    /// Over disk, the file must still exist for canonicalisation.
    pub fn with_file(&self, path: &str, source: String) -> Self {
        let mut loader = self.clone();
        Arc::make_mut(&mut loader.files.held).insert(path.to_owned(), source);
        loader
    }

    /// Whether nothing lies beneath the held files.
    pub fn is_in_memory(&self) -> bool {
        !self.files.over_disk
    }

    /// Build a child loader whose search paths prepend `file_path`'s parent
    /// directory and its same-named subdirectory.
    ///
    /// Search path precedence:
    /// 1. The file's own directory (e.g. `/path/to/` for `/path/to/Foo.ali`)
    /// 2. A same-named subdirectory (e.g. `/path/to/Foo/`) — this lets a module
    ///    keep private submodules in a directory that shares its name
    /// 3. Inherited search paths (ALIFIB_PATH, etc.)
    ///
    /// Two modules in different directories that both include a module by the
    /// same name may therefore resolve to different files — the closest directory
    /// wins.  Duplicate paths are removed in order.
    fn with_parent_dir(&self, file_path: &str) -> Loader {
        let file = std::path::Path::new(file_path);
        let parent = file.parent()
            .and_then(|p| p.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| file_path.to_owned());
        let mut desired = vec![parent];
        if let Some(stem) = file.file_stem().and_then(|s| s.to_str()) {
            let subdir = file.parent()
                .map(|p| p.join(stem))
                .and_then(|p| p.to_str().map(|s| s.to_owned()));
            if let Some(subdir) = subdir {
                desired.push(subdir);
            }
        }
        desired.extend(self.search_paths.iter().cloned());
        let mut seen = HashSet::new();
        let normalized = desired.into_iter()
            .map(|p| self.files.dir(&p))
            .filter(|p| seen.insert(p.clone())).collect();
        if normalized == self.search_paths {
            self.clone()
        } else {
            Loader { search_paths: normalized, files: self.files.clone() }
        }
    }

    /// Read a file by canonical name, mapping errors to `LoadFileError`.
    fn read_file_at(&self, path: &str) -> Result<(String, String), LoadFileError> {
        self.files.read(path)
            .map_err(|cause| LoadFileError::Load { path: path.to_owned(), cause })
    }

    /// Read and parse the root file only, without resolving any dependencies.
    /// Use this when only the AST of a single file is needed.
    pub fn load_only_root(&self, path: &str) -> Result<Program, LoadFileError> {
        let (canonical_path, source) = self.read_file_at(path)?;
        language::parse(&source).map_err(|errors| LoadFileError::Parse {
            path: canonical_path,
            source,
            errors,
        })
    }

    pub fn load(&self, path: &str) -> Result<LoadedFile, LoadFileError> {
        // Strictly canonicalize up front so the path used as a module ID is
        // always the true canonical path, regardless of how the caller spelled it.
        let (canonical_path, source) = self.read_file_at(path)?;
        let file_loader = self.with_parent_dir(&canonical_path);
        let program = language::parse(&source)
            .map_err(|errors| LoadFileError::Parse {
                path: canonical_path.clone(),
                source: source.clone(),
                errors,
            })?;
        let (resolutions, dep_modules) = resolve_all_modules(&file_loader, &canonical_path, &program)?;
        Ok(LoadedFile { canonical_path, source, program, resolutions, dep_modules })
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
///
/// Stored as a nested map `parent -> (module_name -> canonical_path)` so that
/// both levels can be looked up by `&str` without any allocation.
#[derive(Debug)]
pub struct ModuleResolutions {
    resolutions: HashMap<String, HashMap<String, String>>,
}

impl ModuleResolutions {
    pub fn empty() -> Self {
        ModuleResolutions { resolutions: HashMap::new() }
    }

    pub fn resolve(&self, parent_path: &str, module_name: &str) -> Option<&str> {
        self.resolutions.get(parent_path)?.get(module_name).map(|s| s.as_str())
    }

    fn insert(&mut self, parent_path: &str, module_name: String, canonical_path: String) {
        self.resolutions.entry(parent_path.to_owned()).or_default().insert(module_name, canonical_path);
    }
}

struct ModuleStore {
    modules: HashMap<String, ResolvedModule>,
    resolutions: ModuleResolutions,
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

    fn into_parts(self) -> (ModuleResolutions, Vec<(String, ResolvedModule)>) {
        let ModuleStore { mut modules, resolutions, dep_order } = self;
        let ordered = dep_order.into_iter()
            .map(|path| { let m = modules.remove(&path).unwrap(); (path, m) })
            .collect();
        (resolutions, ordered)
    }
}

fn find_file(loader: &Loader, module_name: &str) -> Result<(String, String), LoadFileError> {
    let filename = format!("{}.ali", module_name);
    for dir in &loader.search_paths {
        let candidate = if dir.is_empty() { filename.clone() } else { format!("{}/{}", dir, filename) };
        match loader.files.read(&candidate) {
            Ok(file) => return Ok(file),
            Err(LoadError::NotFound) => continue,
            Err(LoadError::IoError(reason)) => {
                return Err(LoadFileError::ModuleIoError { path: candidate, reason });
            }
        }
    }
    Err(LoadFileError::ModuleNotFound { module_name: module_name.to_owned() })
}

fn resolve_all_modules(
    loader: &Loader,
    root_path: &str,
    root_program: &Program,
) -> Result<(ModuleResolutions, Vec<(String, ResolvedModule)>), LoadFileError> {
    let mut store = ModuleStore::new();
    // `visited` tracks all paths encountered in the DFS.  The root is seeded
    // here but never inserted into the store (it is handled separately).  Any
    // path that is in `visited` but not yet in the store is either the root or
    // a module currently on the recursion stack; encountering such a path again
    // is a dependency cycle.
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(root_path.to_owned());
    resolve_recursive(loader, root_path, root_program, &mut store, &mut visited)?;
    Ok(store.into_parts())
}

fn resolve_recursive(
    loader: &Loader,
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
