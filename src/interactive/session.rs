//! The shared, module-only interactive session state machine.
//!
//! Commands add answers; callers read the activity's state separately. Disk
//! policy belongs to the kernel, which supplies an authorized write target.

use std::path::Path;
use std::sync::Arc;

use crate::aux::loader::Loader;
use crate::core::complex::Complex;
use crate::interpreter::{GlobalStore, InterpretedFile, LoadFailure};
use crate::language;

use super::command::Command;
use super::engine::{resolve_type, RewriteEngine, Steps};
use super::fill::{
    edit_for_fill, filled_report, list_constraints, list_open_holes, start_fill,
    FillSession, HoleRef, ZeroCellFill,
};
use super::response::{
    self, build_history, build_homology_data, build_list_rules, rewrite_state,
    Answer, AutoInfo, Body, ConstraintInfo, FillInfo, FillState, HistoryEntry, HoleInfo,
    Outcome, State, StoredInfo, ZeroCellChoice, ZeroCellInfo,
};
use super::type_info::{module_types, type_info, ThinIndex};

const NO_SESSION: &str = "No active session — use 'start' or 'fill'";

/// A loaded module is idle, rewriting freely, or filling exactly one hole.
enum Activity {
    Idle,
    Rewrite(RewriteEngine),
    Fill(HoleRef, FillSession),
}

/// How a rewrite session begins: [`RewriteEngine::from_store`] or
/// [`RewriteEngine::resume`], which take the same arguments.
type Begin = fn(
    Arc<GlobalStore>, Arc<Complex>, &str, Option<&str>, String, String, bool,
) -> Result<RewriteEngine, String>;

/// The single source of truth for loaded-module command behaviour.
pub struct Session {
    /// The running root: loaded, re-evaluated by `done`, or extended by `store`.
    file: InterpretedFile,
    activity: Activity,
    /// How many activities have begun, so a new one is told from its predecessor.
    begun: u64,
    backward: bool,
    loader: Loader,
}

impl Session {
    /// Load a module through `loader`, retaining it for re-evaluation.
    pub fn open(loader: Loader, path: &str) -> Result<Self, LoadFailure> {
        let file = InterpretedFile::load(&loader, path)?;
        Ok(Session { file, activity: Activity::Idle, begun: 0, backward: false, loader })
    }

    pub fn store(&self) -> &Arc<GlobalStore> { &self.file.state }
    pub fn root_path(&self) -> &str { &self.file.path }
    pub fn source(&self) -> &str { &self.file.source }

    /// The engine driving a free rewrite or fill; absent for a 0-cell fill.
    pub fn engine(&self) -> Option<&RewriteEngine> {
        match &self.activity {
            Activity::Rewrite(e) | Activity::Fill(_, FillSession::Rewrite(e)) => Some(e),
            _ => None,
        }
    }

    pub fn engine_mut(&mut self) -> Option<&mut RewriteEngine> {
        match &mut self.activity {
            Activity::Rewrite(e) | Activity::Fill(_, FillSession::Rewrite(e)) => Some(e),
            _ => None,
        }
    }

    /// Perform one module-only command, returning only what it adds.
    pub fn apply(&mut self, cmd: Command) -> Result<Answer, String> {
        use Command::*;
        match cmd {
            Types => Ok(Answer::outcome(Outcome::Types(
                module_types(self.store(), &ThinIndex::of(self.store()), self.root_path()),
            ))),
            Type(name) => Ok(Answer::outcome(Outcome::Type(
                type_info(self.store(), &ThinIndex::of(self.store()), self.root_path(), &name)?,
            ))),
            Homology(name) => Ok(Answer::outcome(Outcome::Homology(
                build_homology_data(self.store(), self.root_path(), &name)?,
            ))),
            Print => Ok(Answer::outcome(Outcome::Source(self.file.source.clone()))),
            Start { type_name, initial, target } =>
                self.begin_rewrite(RewriteEngine::from_store, &type_name, &initial, target.as_deref()),
            Resume { type_name, proof, target } =>
                self.begin_rewrite(RewriteEngine::resume, &type_name, &proof, target.as_deref()),
            Stop => {
                self.activity = Activity::Idle;
                Ok(Answer::message("Session stopped"))
            }
            Backward(on) => Ok(self.set_backward(on)),
            Holes => {
                let (holes, constraints) = self.holes();
                Ok(Answer::outcome(Outcome::Holes { holes, constraints }))
            }
            Fill(index) => self.begin_fill(index),
            Done => self.finalize_fill(),
            Apply(choices) => self.apply_choices(&choices),
            Auto(n) => self.engine_auto(n, false),
            Random(n) => self.engine_auto(n, true),
            Undo(step) => self.travel(step, false),
            Redo(step) => self.travel(step, true),
            Parallel(on) => self.set_parallel(on),
            Show => Ok(Answer::outcome(Outcome::State)),
            Proof => self.proof_response(),
            History => self.history_response(),
            Rules => self.rules_response(),
            Store(name) => self.store_proof(&name),
        }
    }

    /// The open holes and constraints of maps the root's program defines.
    pub(super) fn holes(&self) -> (Vec<HoleInfo>, Vec<ConstraintInfo>) {
        let (store, program, root) = (self.store(), &self.file.program, self.root_path());
        let holes = list_open_holes(store, program, root).into_iter().map(|h| HoleInfo {
            module: root.to_owned(),
            index: h.index,
            type_name: h.type_name,
            map_name: h.map_name,
            domain_name: h.domain_name,
            source_name: h.source_name,
            dim: h.dim,
            boundary: h.boundary,
        }).collect();
        (holes, list_constraints(store, program, root))
    }

    fn ensure_idle(&self) -> Result<(), String> {
        match self.activity {
            Activity::Idle => Ok(()),
            _ => Err("Session already active — stop it first".to_owned()),
        }
    }

    fn begin(&mut self, activity: Activity) -> Answer {
        self.activity = activity;
        self.begun += 1;
        Answer::outcome(Outcome::State)
    }

    fn begin_rewrite(&mut self, begin: Begin, type_name: &str, diagram: &str, target: Option<&str>)
        -> Result<Answer, String>
    {
        self.ensure_idle()?;
        let tc = resolve_type(self.store(), self.root_path(), type_name)?;
        let engine = begin(
            Arc::clone(self.store()), tc, diagram, target,
            self.root_path().to_owned(), type_name.to_owned(), self.backward,
        )?;
        Ok(self.begin(Activity::Rewrite(engine)))
    }

    /// Mid-session the mode is fixed and only reported; idle, it is set.
    fn set_backward(&mut self, on: Option<bool>) -> Answer {
        if let (Activity::Idle, Some(on)) = (&self.activity, on) {
            self.backward = on;
        }
        Answer::message(format!("Backward mode {}", on_off(self.session_backward())))
    }

    fn session_backward(&self) -> bool {
        self.engine().map_or(self.backward, RewriteEngine::backward)
    }

    fn begin_fill(&mut self, index: usize) -> Result<Answer, String> {
        self.ensure_idle()?;
        let (hole, session) =
            start_fill(self.store(), &self.file.program, self.root_path(), index, self.backward)?;
        Ok(self.begin(Activity::Fill(hole, session)))
    }

    fn finalize_fill(&mut self) -> Result<Answer, String> {
        let Activity::Fill(hole, session) = &self.activity else {
            return Err("No active hole-filling session — use 'fill <n>'".to_owned());
        };
        let filler = session.filler()?;
        let message = filled_report(self.store(), hole, &filler);
        let source = edit_for_fill(self.store(), hole, &filler, self.source())?;
        // Keep the fill available for retry when re-evaluation fails.
        self.file = InterpretedFile::load(&self.loader.with_file(self.root_path(), source), self.root_path())
            .map_err(|failure| failure.message)?;
        self.activity = Activity::Idle;
        Ok(Answer { body: Body::Message(message), edited: true })
    }

    /// The root's disk path; in-memory sessions have no disk target.
    pub(super) fn disk_root(&self) -> Option<&str> {
        if self.loader.is_in_memory() { None } else { Some(self.root_path()) }
    }

    /// Write where the kernel directs, without applying filesystem policy.
    pub(super) fn write(&self, to: &Path) -> Result<Answer, String> {
        std::fs::write(to, format!("{}\n", self.source().trim_end()))
            .map_err(|e| format!("cannot write '{}': {}", to.display(), e))?;
        Ok(Answer::message(format!("Saved to '{}'", to.display())))
    }

    fn apply_choices(&mut self, choices: &[usize]) -> Result<Answer, String> {
        match &mut self.activity {
            Activity::Fill(_, FillSession::ZeroCell(zc)) => match choices {
                [k] => zc.choose(*k)?,
                _ => return Err("A 0-cell fill takes one choice".to_owned()),
            },
            Activity::Rewrite(e) | Activity::Fill(_, FillSession::Rewrite(e)) => match choices {
                [k] => { e.step(*k)?; }
                _ if e.parallel() => { e.step_multi(choices)?; }
                _ => return Err("Multi-apply requires parallel mode".to_owned()),
            },
            Activity::Idle => return Err(NO_SESSION.to_owned()),
        }
        Ok(Answer::outcome(Outcome::State))
    }

    fn engine_auto(&mut self, n: usize, random: bool) -> Result<Answer, String> {
        let e = self.engine_mut().ok_or(NO_SESSION)?;
        let (applied, stop_reason) = if random { e.random(n) } else { e.auto(n) }?;
        Ok(Answer::outcome(Outcome::Auto(AutoInfo { applied, stop_reason })))
    }

    /// Undo or redo, one step or to a step, through either kind of history.
    fn travel(&mut self, step: Option<usize>, redo: bool) -> Result<Answer, String> {
        let steps: &mut dyn Steps = match &mut self.activity {
            Activity::Rewrite(e) | Activity::Fill(_, FillSession::Rewrite(e)) => e,
            Activity::Fill(_, FillSession::ZeroCell(zc)) => zc,
            Activity::Idle => return Err(NO_SESSION.to_owned()),
        };
        match (redo, step) {
            (false, None) => steps.undo(),
            (false, Some(step)) => steps.undo_to(step),
            (true, None) => steps.redo(),
            (true, Some(step)) => steps.redo_to(step),
        }?;
        Ok(Answer::outcome(Outcome::State))
    }

    fn set_parallel(&mut self, on: Option<bool>) -> Result<Answer, String> {
        let e = self.engine_mut().ok_or(NO_SESSION)?;
        if let Some(on) = on { e.set_parallel(on); }
        Ok(Answer::message(format!("Parallel mode {}", on_off(e.parallel()))))
    }

    /// The expression that `store` would persist, together with its boundary.
    fn proof_response(&self) -> Result<Answer, String> {
        let proof = match &self.activity {
            Activity::Rewrite(e) | Activity::Fill(_, FillSession::Rewrite(e)) => Some(response::proof(e)),
            Activity::Fill(_, FillSession::ZeroCell(zc)) => zc.chosen_name()
                .map(|name| response::Proof { boundary: None, expr: name.to_owned() }),
            Activity::Idle => return Err(NO_SESSION.to_owned()),
        };
        Ok(Answer::outcome(Outcome::Proof(proof)))
    }

    fn history_response(&self) -> Result<Answer, String> {
        let history = match &self.activity {
            Activity::Rewrite(e) | Activity::Fill(_, FillSession::Rewrite(e)) => build_history(e),
            Activity::Fill(_, FillSession::ZeroCell(zc)) => zc.chosen_name().zip(zc.chosen_index())
                .map(|(name, i)| HistoryEntry { step: 1, rule_name: name.to_owned(), choice: Some(vec![i]) })
                .into_iter().collect(),
            Activity::Idle => return Err(NO_SESSION.to_owned()),
        };
        Ok(Answer::outcome(Outcome::History(history)))
    }

    fn rules_response(&self) -> Result<Answer, String> {
        let rules = match &self.activity {
            Activity::Rewrite(e) | Activity::Fill(_, FillSession::Rewrite(e)) => build_list_rules(e),
            Activity::Fill(_, FillSession::ZeroCell(_)) => Vec::new(),
            Activity::Idle => return Err(NO_SESSION.to_owned()),
        };
        Ok(Answer::outcome(Outcome::Rules(rules)))
    }

    fn store_proof(&mut self, name: &str) -> Result<Answer, String> {
        let e = match &self.activity {
            Activity::Rewrite(e) | Activity::Fill(_, FillSession::Rewrite(e)) => e,
            Activity::Fill(_, FillSession::ZeroCell(_)) =>
                return Err("Nothing to store in a 0-cell hole-filling session".to_owned()),
            Activity::Idle => return Err(NO_SESSION.to_owned()),
        };
        let expr = e.proof_expr();
        let type_name = e.type_name().to_owned();
        let source = format!("{}\n\n@{}\nlet {} = {}\n", self.source().trim_end(), type_name, name, expr);
        let program = language::parse(&source)
            .map_err(|errs| format!("Cannot store proof: resulting source does not parse ({} error(s))", errs.len()))?;
        let store = self.engine_mut().ok_or(NO_SESSION)?.register_proof(name)?;
        self.file.state = store;
        self.file.source = source;
        self.file.program = program;
        Ok(Answer {
            body: Body::Outcome(Outcome::Stored(StoredInfo { type_name, def_name: name.to_owned(), expr })),
            edited: true,
        })
    }

    /// The activity's image. A loaded session never returns `Unloaded`.
    pub fn state(&self) -> State {
        let backward = self.session_backward();
        let activity = self.begun;
        match &self.activity {
            Activity::Idle => State::Idle { module: self.root_path().to_owned(), backward },
            Activity::Rewrite(e) => State::Rewrite { activity, backward, rewrite: rewrite_state(e) },
            Activity::Fill(hole, session) => State::Fill {
                activity, backward, hole: fill_info(hole),
                by: match session {
                    FillSession::Rewrite(e) => FillState::Rewrite(rewrite_state(e)),
                    FillSession::ZeroCell(zc) => FillState::ZeroCell(zero_cell_info(zc)),
                },
            },
        }
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn fill_info(hole: &HoleRef) -> FillInfo {
    FillInfo {
        type_name: hole.type_name.clone(),
        map_name: hole.map_name.clone(),
        domain_name: hole.domain_name.clone(),
        source_name: hole.source_name.clone(),
        dim: hole.dim,
    }
}

fn zero_cell_info(zc: &ZeroCellFill) -> ZeroCellInfo {
    let choices = if zc.chosen.is_some() {
        Vec::new()
    } else {
        zc.choices.iter().enumerate()
            .map(|(i, (_, name))| ZeroCellChoice { index: i, name: name.clone() })
            .collect()
    };
    ZeroCellInfo {
        choices,
        chosen: zc.chosen_name().map(str::to_owned),
        target_reached: zc.target_reached(),
        can_undo: zc.applied() > 0,
        can_redo: zc.can_redo(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_proof_refuses_unparseable_source_without_mutation() {
        let loader = Loader::in_memory([(
            "Idem.ali".to_owned(),
            include_str!("../../tests/fixtures/Idem.ali").to_owned(),
        )].into_iter().collect());
        let mut session = Session::open(loader, "Idem.ali").expect("load Idem");
        session.apply(Command::Start {
            type_name: "Idem".to_owned(),
            initial: "lhs".to_owned(),
            target: Some("rhs".to_owned()),
        }).expect("start Idem");
        session.apply(Command::Apply(vec![0])).expect("apply first rewrite");

        let source_before = session.source().as_bytes().to_vec();
        let store_before = Arc::clone(session.store());
        let engine_store_before = session.engine().unwrap().store_arc();
        assert!(Arc::ptr_eq(&store_before, &engine_store_before));
        let state_before = serde_json::to_value(session.state()).unwrap();
        let proof_before = session.engine().unwrap().proof_expr();

        // Bypass command parsing to exercise the source commit boundary directly.
        let result = session.apply(Command::Store("bad(".to_owned()));
        assert!(result.is_err());
        assert_eq!(session.source().as_bytes(), source_before);
        assert!(language::parse(session.source()).is_ok());
        assert!(Arc::ptr_eq(session.store(), &store_before));
        let engine = session.engine().unwrap();
        assert!(Arc::ptr_eq(&engine.store_arc(), &engine_store_before));
        assert_eq!(serde_json::to_value(session.state()).unwrap(), state_before);
        assert_eq!(engine.proof_expr(), proof_before);
        assert!(!engine.type_complex().name_in_use("bad("));
        assert!(engine.type_complex().find_generator("bad(").is_none());

        assert!(session.apply(Command::Holes).is_ok());
        let answer = session.apply(Command::Store("p".to_owned())).expect("valid store after refusal");
        assert!(answer.edited);
        assert!(language::parse(session.source()).is_ok());
        assert!(session.engine().unwrap().type_complex().name_in_use("p"));
        assert!(Arc::ptr_eq(session.store(), &session.engine().unwrap().store_arc()));
    }
}
