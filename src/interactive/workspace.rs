//! A mutable proof workspace: loads an .ali file and supports incremental
//! addition of `let` bindings and generator declarations (via goal construction)
//! within a chosen type block.
//!
//! Each addition is validated by re-interpreting a temp copy of the source with
//! the new text injected into the type block. The temp file is written next to
//! the original so that relative `include` paths still resolve.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::aux::loader::Loader;
use crate::core::complex::Complex;
use crate::core::diagram::Diagram;
use crate::interpreter::{GlobalStore, InterpretedFile};
use super::session::Move;

// ── Types ─────────────────────────────────────────────────────────────────────

/// A single definition added to the workspace during the session.
#[derive(Debug, Clone)]
pub enum Addition {
    /// A `let name = expr` binding (stored as the raw alifib source text of the expr).
    Let { name: String, text: String },
    /// A generator cell `name : source -> target` added after a completed goal.
    Generator { name: String, source: String, target: String },
}

impl Addition {
    /// Return the name bound by this addition.
    pub fn name(&self) -> &str {
        match self {
            Addition::Let { name, .. } | Addition::Generator { name, .. } => name,
        }
    }

    /// Format as a line to be injected into the type body (without trailing newline).
    fn as_type_body_line(&self) -> String {
        match self {
            Addition::Let { text, .. } => format!("  {}", text.trim()),
            Addition::Generator { name, source, target } => {
                format!("  {} : {} -> {},", name, source, target)
            }
        }
    }
}

/// A completed interactive proof: the (n+1)-dim diagram and the move log.
#[derive(Debug)]
pub struct ProofRecord {
    pub name: String,
    pub source_name: String,
    pub target_name: String,
    pub proof: Diagram,
    pub moves: Vec<Move>,
}

// ── Workspace ─────────────────────────────────────────────────────────────────

/// A mutable workspace for incremental proof construction within an .ali type.
pub struct Workspace {
    /// Absolute path to the original .ali source file.
    source_file: String,
    /// The type being worked in.
    type_name: String,
    /// Content of the original file (unmodified).
    original_source: String,
    /// Additions accumulated this session.
    additions: Vec<Addition>,
    /// The current re-interpreted store (original + all additions).
    current_store: Arc<GlobalStore>,
    /// Completed proof records.
    pub proofs: Vec<ProofRecord>,
}

impl Workspace {
    /// Load a source file and find the given type block, returning a fresh workspace.
    pub fn load(source_file: &str, type_name: &str) -> Result<Self, String> {
        let canonical = canonicalize(source_file)?;
        let original_source = std::fs::read_to_string(&canonical)
            .map_err(|e| format!("cannot read '{}': {}", canonical, e))?;

        // Verify the type block exists.
        if !source_contains_type(&original_source, type_name) {
            return Err(format!("type '{}' not found in '{}'", type_name, canonical));
        }

        // Initial interpretation (no additions yet).
        let loader = Loader::default(vec![]);
        let file = InterpretedFile::load(&loader, &canonical)
            .into_result()
            .map_err(|_| format!("failed to interpret '{}'", canonical))?;

        Ok(Self {
            source_file: canonical,
            type_name: type_name.to_owned(),
            original_source,
            additions: Vec::new(),
            current_store: Arc::clone(&file.state),
            proofs: Vec::new(),
        })
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    pub fn type_name(&self) -> &str { &self.type_name }
    pub fn source_file(&self) -> &str { &self.source_file }
    pub fn store(&self) -> &Arc<GlobalStore> { &self.current_store }
    pub fn additions(&self) -> &[Addition] { &self.additions }

    /// Return the current Complex for the working type.
    pub fn type_complex(&self) -> Result<Arc<Complex>, String> {
        // Find the module by the source file path (canonical).
        let module_complex = self.current_store
            .find_module(&self.source_file)
            .ok_or_else(|| format!("module '{}' not found in store", self.source_file))?;

        let (type_tag, _) = module_complex
            .find_generator(&self.type_name)
            .ok_or_else(|| format!("type '{}' not found in module", self.type_name))?;

        let gid = match type_tag {
            crate::aux::Tag::Global(gid) => *gid,
            crate::aux::Tag::Local(_) => {
                return Err(format!("'{}' is a local cell, not a type", self.type_name));
            }
        };

        let entry = self.current_store
            .find_type(gid)
            .ok_or_else(|| format!("type entry for '{}' not found", self.type_name))?;

        Ok(Arc::clone(&entry.complex))
    }

    // ── Mutation ──────────────────────────────────────────────────────────────

    /// Add a `let` binding. `text` should be the full binding, e.g. `let f = id id`.
    ///
    /// The addition is validated by re-interpretation. Returns an error (and
    /// does not add) if the binding is invalid.
    pub fn eval_let(&mut self, text: &str) -> Result<(), String> {
        let trimmed = text.trim().to_owned();
        // Ensure it starts with "let"
        if !trimmed.starts_with("let ") {
            return Err(format!("expected a 'let' binding, got: {}", trimmed));
        }
        // Extract the name from "let name = ..."
        let name = extract_let_name(&trimmed)?;
        let addition = Addition::Let { name, text: trimmed };
        self.try_add(addition)
    }

    /// Register the result of a completed goal as a generator declaration.
    ///
    /// Adds `name : source_name -> target_name` to the type body and validates.
    pub fn add_goal_result(
        &mut self,
        name: &str,
        source_name: &str,
        target_name: &str,
        proof: Diagram,
        moves: Vec<Move>,
    ) -> Result<(), String> {
        let addition = Addition::Generator {
            name: name.to_owned(),
            source: source_name.to_owned(),
            target: target_name.to_owned(),
        };
        self.try_add(addition)?;
        self.proofs.push(ProofRecord {
            name: name.to_owned(),
            source_name: source_name.to_owned(),
            target_name: target_name.to_owned(),
            proof,
            moves,
        });
        Ok(())
    }

    /// Try adding an addition: inject, write temp file, re-interpret. Rolls back on error.
    fn try_add(&mut self, addition: Addition) -> Result<(), String> {
        self.additions.push(addition);
        match self.reinterpret() {
            Ok(store) => {
                self.current_store = store;
                Ok(())
            }
            Err(e) => {
                self.additions.pop();
                Err(e)
            }
        }
    }

    // ── Temp file management ──────────────────────────────────────────────────

    /// Write the current session source (original + additions) to a temp file
    /// next to the original and return its path.
    ///
    /// Caller is responsible for deleting the temp file when done.
    pub fn write_temp_file(&self) -> Result<String, String> {
        let source = self.build_source();
        let path = self.temp_path();
        std::fs::write(&path, &source)
            .map_err(|e| format!("cannot write temp file '{}': {}", path, e))?;
        Ok(path)
    }

    /// Path for the session temp file (sibling of the original, hidden name).
    pub fn temp_path(&self) -> String {
        let original = Path::new(&self.source_file);
        let dir = original.parent().unwrap_or(Path::new("."));
        let stem = original
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("session");
        let temp = dir.join(format!(".{}_{}_session.ali", stem, std::process::id()));
        temp.to_string_lossy().into_owned()
    }

    fn reinterpret(&self) -> Result<Arc<GlobalStore>, String> {
        let temp = self.write_temp_file()?;
        let result = {
            let loader = Loader::default(vec![]);
            InterpretedFile::load(&loader, &temp)
                .into_result()
                .map(|f| Arc::clone(&f.state))
                .map_err(|_| "interpretation error — check your definition".to_string())
        };
        let _ = std::fs::remove_file(&temp);
        result
    }

    /// Build the full source: original file with additions injected into the type block.
    fn build_source(&self) -> String {
        inject_into_type_block(&self.original_source, &self.type_name, &self.additions)
    }

    // ── Export ────────────────────────────────────────────────────────────────

    /// Return just the addition lines as an alifib source snippet (for pasting
    /// into the original file's type block).
    pub fn export_additions(&self) -> String {
        if self.additions.is_empty() {
            return "(* no session additions *)".to_owned();
        }
        let mut out = String::from("  (* --- session additions --- *)\n");
        for a in &self.additions {
            out.push_str(&a.as_type_body_line());
            out.push('\n');
        }
        out
    }

    /// Return the full modified source (original + additions) for writing to a file.
    pub fn export_full_source(&self) -> String {
        self.build_source()
    }
}

// ── Text manipulation ─────────────────────────────────────────────────────────

/// Check whether `source` contains a type block for `type_name` in the form
/// `type_name <<= {`.
fn source_contains_type(source: &str, type_name: &str) -> bool {
    // Accept `TypeName <<=` anywhere in the source.
    let needle = format!("{} <<=", type_name);
    source.contains(&needle)
}

/// Inject `additions` before the closing `}` of the `type_name <<= { ... }` block.
///
/// Uses brace counting to handle nested blocks correctly. Returns the original
/// source unchanged if the type block cannot be located.
fn inject_into_type_block(source: &str, type_name: &str, additions: &[Addition]) -> String {
    if additions.is_empty() {
        return source.to_owned();
    }

    let needle = format!("{} <<=", type_name);
    let Some(start) = source.find(&needle) else {
        return source.to_owned();
    };

    // Find the first '{' at or after `start + needle.len()`
    let search_from = start + needle.len();
    let Some(rel_brace) = source[search_from..].find('{') else {
        return source.to_owned();
    };
    let brace_open = search_from + rel_brace;

    // Walk forward counting braces to find the matching '}'
    let mut depth: usize = 0;
    let mut inject_pos: Option<usize> = None;
    for (i, ch) in source[brace_open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    inject_pos = Some(brace_open + i);
                    break;
                }
            }
            _ => {}
        }
    }

    let Some(pos) = inject_pos else {
        return source.to_owned();
    };

    let addition_text: String = additions
        .iter()
        .map(|a| format!("{}\n", a.as_type_body_line()))
        .collect();

    let mut result = source.to_owned();
    // Ensure there's a comma after the last existing item if needed:
    // For simplicity, just inject with a leading newline so it's syntactically safe.
    result.insert_str(pos, &format!("\n  (* --- session additions --- *)\n{}", addition_text));
    result
}

/// Extract the binding name from a `let name = ...` string.
fn extract_let_name(text: &str) -> Result<String, String> {
    // text starts with "let "
    let rest = text.trim_start_matches("let ").trim();
    let name = rest.split_whitespace().next()
        .ok_or_else(|| "let binding has no name".to_string())?;
    Ok(name.to_owned())
}

/// Canonicalize a file path, returning an error string on failure.
fn canonicalize(path: &str) -> Result<String, String> {
    let p = PathBuf::from(path);
    // Try canonicalization; fall back to the given path if it fails
    // (e.g. the file doesn't exist yet — though that would fail at read time).
    let canonical = std::fs::canonicalize(&p)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.to_string_lossy().into_owned());
    Ok(canonical)
}
