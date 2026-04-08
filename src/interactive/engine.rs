//! The replay engine: loads a source file, replays the move log, and
//! returns the current session state with available rewrites pre-computed.

use crate::aux::loader::Loader;
use crate::core::complex::Complex;
use crate::core::diagram::Diagram;
use crate::interpreter::{GlobalStore, InterpretedFile};
use super::rewrite::{CandidateRewrite, apply_rewrite, find_candidate_rewrites};
use super::session::SessionFile;
use std::sync::Arc;

/// The in-memory session state produced by replaying a session file.
pub struct SessionState {
    pub session: SessionFile,
    pub store: Arc<GlobalStore>,
    pub type_complex: Arc<Complex>,
    pub source_diagram: Diagram,
    pub target_diagram: Option<Diagram>,
    pub current_diagram: Diagram,
    pub available_rewrites: Vec<CandidateRewrite>,
}

impl SessionState {
    pub fn target_reached(&self) -> bool {
        self.target_diagram.as_ref()
            .map(|t| Diagram::equal(&self.current_diagram, t))
            .unwrap_or(false)
    }
}

/// Load and interpret the source file, look up the type and diagrams, replay
/// all moves in the session, and compute available rewrites for the current state.
pub fn replay_session(session: SessionFile) -> Result<SessionState, String> {
    // 1. Interpret the source file.
    let loader = Loader::default(vec![]);
    let file = InterpretedFile::load(&loader, &session.source_file)
        .into_result()
        .map_err(|_| format!("failed to interpret '{}'", session.source_file))?;

    let store = Arc::clone(&file.state);

    // 2. Find the root module's complex (keyed by the canonical path).
    let module_complex = store
        .find_module(&file.path)
        .ok_or_else(|| format!("module '{}' not found in store", file.path))?;

    // 3. Find the named type generator in the module complex.
    let (type_tag, _) = module_complex
        .find_generator(&session.type_name)
        .ok_or_else(|| format!("type '{}' not found in module", session.type_name))?;

    let type_gid = match type_tag {
        crate::aux::Tag::Global(gid) => *gid,
        crate::aux::Tag::Local(_) => {
            return Err(format!("'{}' is a local cell, not a type", session.type_name));
        }
    };

    // 4. Get the type entry and its complex.
    let type_entry = store
        .find_type(type_gid)
        .ok_or_else(|| format!("type entry for '{}' not found", session.type_name))?;
    let type_complex = Arc::clone(&type_entry.complex);

    // 5. Look up source and target diagrams.
    // Check the type complex first (let bindings inside the type body),
    // then fall back to the module complex (top-level let bindings).
    let find_diagram = |name: &str| -> Option<Diagram> {
        type_complex.find_diagram(name).cloned()
            .or_else(|| module_complex.find_diagram(name).cloned())
    };

    let source_diagram = find_diagram(&session.source_diagram)
        .ok_or_else(|| format!(
            "diagram '{}' not found in type '{}' or module",
            session.source_diagram, session.type_name
        ))?;

    let target_diagram = session.target_diagram.as_ref()
        .map(|name| {
            find_diagram(name).ok_or_else(|| format!(
                "target diagram '{}' not found in type '{}' or module",
                name, session.type_name
            ))
        })
        .transpose()?;

    // 6. Replay moves.
    let mut current = source_diagram.clone();
    for (step_idx, mov) in session.moves.iter().enumerate() {
        let candidates = find_candidate_rewrites(&store, &type_complex, &current);

        let candidate = candidates.get(mov.choice).ok_or_else(|| {
            format!(
                "replay failed at step {}: choice {} out of range ({} candidate(s) available)",
                step_idx + 1,
                mov.choice,
                candidates.len(),
            )
        })?;

        // Sanity check: rule name should match what was recorded.
        if candidate.rule_name != mov.rule_name {
            return Err(format!(
                "replay sanity check failed at step {}: expected rule '{}' at choice {}, found '{}'",
                step_idx + 1,
                mov.rule_name,
                mov.choice,
                candidate.rule_name,
            ));
        }

        current = apply_rewrite(candidate);
    }

    // 7. Compute available rewrites at the final state.
    let available_rewrites = find_candidate_rewrites(&store, &type_complex, &current);

    Ok(SessionState {
        session,
        store,
        type_complex,
        source_diagram,
        target_diagram,
        current_diagram: current,
        available_rewrites,
    })
}
