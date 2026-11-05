//! The shared line evaluator and the filesystem policy for interactive commands.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::aux::loader::Loader;
use crate::language::error::Diagnostic;

use super::command::{self, Input, Line};
use super::response::{Answer, Body, ModuleInfo, Outcome, State};
use super::richtext::{self, RichText};
use super::session::Session;
use super::type_info::{store_types, ThinIndex};

/// The interactive kernel every front end drives, with one optional session
/// and one revision clock for the state, last load, and running module.
pub struct Kernel {
    session: Option<Session>,
    last_load: Option<LastLoad>,
    revision: u64,
    generation: u64,
    module_revision: u64,
    fs: FsAccess,
}

/// The last accepted load attempt, including one that failed to interpret.
enum LastLoad {
    Disk(String),
    Memory,
}

/// Canonicalise a file to read whole; missing files are refusals.
fn existing(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve '{}': {}", path.display(), error))
}

/// Resolve existing entries whole, including dangling symlinks. Only an absent
/// entry may use its canonical parent, so writes never follow an unchecked link.
fn target(path: &Path) -> Result<PathBuf, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => existing(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let name = path
                .file_name()
                .ok_or_else(|| format!("no file name in '{}'", path.display()))?;
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            Ok(existing(parent)?.join(name))
        }
        Err(error) => Err(format!("cannot inspect '{}': {}", path.display(), error)),
    }
}

/// A directory canonicalised once, shared by command policy and the file picker.
#[derive(Clone)]
pub struct CanonicalDir(PathBuf);

impl CanonicalDir {
    pub fn new(dir: &Path) -> io::Result<Self> {
        let dir = fs::canonicalize(dir)?;
        if !dir.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a directory",
            ));
        }
        Ok(Self(dir))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Resolve an existing `.ali` path beneath this directory.
    pub fn readable(&self, path: &str) -> Result<PathBuf, String> {
        self.check(existing(&self.0.join(path))?)
    }

    /// Resolve a write target by the same bound, rejecting dangling symlinks.
    pub fn writable(&self, path: &str) -> Result<PathBuf, String> {
        self.check(target(&self.0.join(path))?)
    }

    fn check(&self, path: PathBuf) -> Result<PathBuf, String> {
        if !path.starts_with(&self.0) {
            return Err(format!(
                "path '{}' is outside '{}'",
                path.display(),
                self.0.display()
            ));
        }
        if path.extension().is_none_or(|extension| extension != "ali") {
            return Err(format!("path '{}' is not an .ali file", path.display()));
        }
        Ok(path)
    }
}

/// Which disk paths commands may touch. Includes retain the loader's normal
/// search paths; this is a command policy, not a loader sandbox.
pub enum FsAccess {
    Unrestricted,
    Within(CanonicalDir),
    Denied(&'static str),
}

impl FsAccess {
    /// Authorisation and loader selection are inseparable: a denied scope has
    /// no disk loader that a caller could accidentally use.
    fn load(&self, path: &str) -> Result<(Loader, String), String> {
        let (loader, path) = match self {
            Self::Unrestricted => (Loader::default(vec![]), existing(Path::new(path))?),
            Self::Within(dir) => {
                let path = dir.readable(path)?;
                (Loader::in_dir(&dir.path().to_string_lossy(), vec![]), path)
            }
            Self::Denied(reason) => return Err((*reason).to_owned()),
        };
        Ok((loader, path.to_string_lossy().into_owned()))
    }

    fn save(&self, path: &str) -> Result<String, String> {
        let path = match self {
            Self::Unrestricted => target(Path::new(path))?,
            Self::Within(dir) => dir.writable(path)?,
            Self::Denied(reason) => return Err((*reason).to_owned()),
        };
        Ok(path.to_string_lossy().into_owned())
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Error,
}

/// A rendered transcript answer, with no state or source attached.
#[derive(Serialize, Deserialize)]
pub struct Reply {
    pub input: String,
    pub generation: u64,
    pub status: Status,
    pub message: Option<String>,
    pub rendered: Option<RichText>,
    pub edited: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

impl Reply {
    /// An error for a part that ran nothing, including parse errors and refusals.
    pub fn refusal(input: &str, generation: u64, message: String) -> Self {
        Self {
            input: input.to_owned(),
            generation,
            status: Status::Error,
            message: Some(message),
            rendered: None,
            edited: false,
            diagnostics: vec![],
        }
    }
}

impl Kernel {
    pub fn new(fs: FsAccess) -> Self {
        Self {
            session: None,
            last_load: None,
            revision: 0,
            generation: 0,
            module_revision: 0,
            fs,
        }
    }

    /// Load a disk file only after the scope authorises it.
    pub fn open(&mut self, path: &str) -> Reply {
        match self.fs.load(path) {
            Ok((loader, path)) => {
                let last_load = LastLoad::Disk(path.clone());
                self.load(loader, &path, last_load)
            }
            Err(message) => Reply::refusal("", self.generation, message),
        }
    }

    /// Evaluate the browser's source and module tabs without reaching the disk.
    pub fn open_text(
        &mut self,
        source: String,
        mut modules: HashMap<String, String>,
        name: Option<&str>,
    ) -> Reply {
        let name = name.filter(|name| !name.is_empty()).unwrap_or("source.ali");
        let root = if name.ends_with(".ali") {
            name.to_owned()
        } else {
            format!("{}.ali", name)
        };
        modules.insert(root.clone(), source);
        self.load(Loader::in_memory(modules), &root, LastLoad::Memory)
    }

    fn load(&mut self, loader: Loader, path: &str, last_load: LastLoad) -> Reply {
        // Release the old store before interpreting the new one: WASM must not
        // hold two loaded sessions at once, even when the next load fails.
        self.session = None;
        self.last_load = Some(last_load);
        self.revision += 1;
        self.generation = self.revision;
        self.module_revision = self.revision;
        match Session::open(loader, path) {
            Ok(session) => {
                self.session = Some(session);
                self.answer_reply(Answer::message(format!("Loaded {}", path)))
            }
            Err(failure) => {
                let mut reply = Reply::refusal("", self.generation, failure.message);
                reply.diagnostics = failure.diagnostics;
                reply
            }
        }
    }

    /// Run exactly the parts already parsed, stopping at the first error.
    pub fn run(&mut self, line: Line) -> Vec<Reply> {
        let parts = match line.parts {
            Ok(parts) => parts,
            Err(message) => return vec![Reply::refusal(&line.text, self.generation, message)],
        };
        let mut replies = Vec::new();
        for part in parts {
            let mut reply = match part.input {
                Ok(input) => self.execute(input),
                Err(message) => Reply::refusal("", self.generation, message),
            };
            reply.input = part.text;
            let failed = reply.status == Status::Error;
            replies.push(reply);
            if failed {
                break;
            }
        }
        replies
    }

    pub fn eval(&mut self, line: &str) -> Vec<Reply> {
        self.run(command::parse_line(line))
    }

    fn execute(&mut self, input: Input) -> Reply {
        match input {
            Input::Help => self.answer_reply(Answer::outcome(Outcome::Help)),
            Input::Load(Some(path)) => self.open(&path),
            Input::Load(None) => match &self.last_load {
                Some(LastLoad::Disk(path)) => self.open(&path.clone()),
                Some(LastLoad::Memory) => Reply::refusal(
                    "",
                    self.generation,
                    "in the browser, press Evaluate".to_owned(),
                ),
                None => {
                    Reply::refusal("", self.generation, "no previous load to repeat".to_owned())
                }
            },
            Input::Save(path) => {
                let result = self.save(path.as_deref());
                self.result_reply(result)
            }
            Input::Session(command) => {
                let Some(session) = self.session.as_mut() else {
                    return Reply::refusal("", self.generation, "no source loaded".to_owned());
                };
                if command.mutates() {
                    self.revision += 1;
                }
                let result = session.apply(command);
                self.result_reply(result)
            }
        }
    }

    fn save(&self, path: Option<&str>) -> Result<Answer, String> {
        let session = self.session.as_ref().ok_or("no source loaded")?;
        let root = session
            .disk_root()
            .ok_or("save: this source belongs to the browser's editor")?;
        let path = self.fs.save(path.unwrap_or(root))?;
        session.write(Path::new(&path))
    }

    fn result_reply(&mut self, result: Result<Answer, String>) -> Reply {
        match result {
            Ok(answer) => {
                if answer.edited {
                    self.module_revision = self.revision;
                }
                self.answer_reply(answer)
            }
            Err(message) => Reply::refusal("", self.generation, message),
        }
    }

    fn answer_reply(&self, answer: Answer) -> Reply {
        let (message, rendered) = match answer.body {
            Body::Message(message) => (Some(message), None),
            Body::Outcome(outcome) => (None, Some(richtext::render(&outcome, || self.state()))),
        };
        Reply {
            input: String::new(),
            generation: self.generation,
            status: Status::Ok,
            message,
            rendered,
            edited: answer.edited,
            diagnostics: vec![],
        }
    }

    pub fn state(&self) -> State {
        self.session
            .as_ref()
            .map_or(State::Unloaded, Session::state)
    }

    /// The loaded module, or `None` with nothing loaded.
    pub fn module(&self) -> Option<ModuleInfo> {
        let session = self.session.as_ref()?;
        let (holes, constraints) = session.holes();
        Some(ModuleInfo {
            base: match &self.fs {
                FsAccess::Within(dir) => Some(dir.path().to_string_lossy().into_owned()),
                FsAccess::Unrestricted | FsAccess::Denied(_) => None,
            },
            root: session.root_path().to_owned(),
            types: store_types(session.store(), &ThinIndex::of(session.store())),
            holes,
            constraints,
            source: session.source().to_owned(),
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn module_revision(&self) -> u64 {
        self.module_revision
    }

    pub(super) fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }
    pub(super) fn session_mut(&mut self) -> Option<&mut Session> {
        self.session.as_mut()
    }
}

// Private access keeps the native command answer and module snapshot in one store.
#[cfg(test)]
mod cross_module_thin_tags {
    use super::{FsAccess, Kernel, Status};
    use crate::interactive::command::Command;
    use crate::interactive::response::{Body, Outcome};

    #[test]
    fn thin_tags_cross_module() {
        let path =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/RootT.ali");
        let mut kernel = Kernel::new(FsAccess::Unrestricted);
        let loaded = kernel.open(path.to_str().unwrap());
        assert!(loaded.status == Status::Ok, "{:?}", loaded.message);

        // Do not go through Reply: it intentionally retains only rendered text.
        let answer = kernel
            .session
            .as_mut()
            .unwrap()
            .apply(Command::Type("T".into()))
            .expect("type T succeeds");
        assert!(!answer.edited);
        let Body::Outcome(Outcome::Type(detail)) = answer.body else {
            panic!("type T must return native TypeInfo");
        };
        let module = kernel.module().unwrap();
        assert_eq!(module.root, path.canonicalize().unwrap().to_str().unwrap());
        let summary = module
            .types
            .iter()
            .find(|t| t.module == module.root && t.name == "T")
            .expect("root T in module snapshot");
        let origin_path = path
            .with_file_name("ThinOrigin.ali")
            .canonicalize()
            .unwrap();
        let origin = module
            .types
            .iter()
            .find(|t| t.module == origin_path.to_str().unwrap() && t.name == "S")
            .expect("included S in its full-path module");
        let u = origin.generators.iter().find(|g| g.name == "u").unwrap();
        let v = summary.generators.iter().find(|g| g.name == "v").unwrap();
        let u_tag = u.tag.expect("global u tag");
        let v_tag = v.tag.expect("global v tag");
        assert_ne!(u_tag, v_tag, "the map is not an identity inclusion");
        assert!(origin.thin_tags.contains(&u_tag));
        assert_eq!(summary.thin_tags, vec![v_tag], "u => v propagates thinness");
        assert!(v.face_tags.is_empty(), "v is a zero-cell");

        // Both values belong to this very kernel, so compare actual IDs, not
        // names, stripped tags, or IDs allocated by a second load.
        assert_eq!(detail.module, module.root);
        assert_eq!(detail.name, summary.name);
        assert_eq!(detail.thin_tags, summary.thin_tags);
        assert_eq!(detail.generators.len(), summary.generators.len());
        for (a, b) in detail.generators.iter().zip(&summary.generators) {
            assert_eq!(
                (&a.name, a.dim, a.tag, &a.face_tags),
                (&b.name, b.dim, b.tag, &b.face_tags)
            );
        }
        assert_eq!(
            summary
                .maps
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["FromS", "T"]
        );
        let from_s = summary.maps.iter().find(|m| m.name == "FromS").unwrap();
        assert_eq!(from_s.domain, "ThinOrigin.S");
        assert!(from_s.holes.is_empty());
    }
}
