//! Session file format: the persisted move log for an interactive rewrite session.

use serde::{Deserialize, Serialize};

/// The contents of a session file, persisted as JSON on disk.
///
/// Each CLI call reads this file, re-interprets the source file, replays
/// the moves, and optionally appends a new move before writing back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFile {
    /// Path to the `.ali` source file (absolute or relative to the working directory).
    pub source_file: String,
    /// Name of the type whose generators serve as rewrite rules.
    pub type_name: String,
    /// Name of the source n-diagram within the type's complex.
    pub source_diagram: String,
    /// Optional name of the target n-diagram (the goal to reach).
    pub target_diagram: Option<String>,
    /// Ordered list of moves applied so far.
    pub moves: Vec<Move>,
}

/// A single rewrite move in the session history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Move {
    /// Index into the rewrites list at the time of this move (manual steps).
    /// `None` for parallel auto steps, which are replayed by re-running the
    /// greedy algorithm rather than indexing into a list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<usize>,
    /// The name of the rule that was applied (for human readability and sanity checking).
    /// For parallel families, this is a comma-separated list of rule names.
    pub rule_name: String,
    /// Whether this move was made with parallel mode enabled.
    #[serde(default)]
    pub parallel: bool,
}

impl SessionFile {
    /// Deserialise a session from a JSON file at `path`.
    pub fn read(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read session file '{}': {}", path, e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("invalid session file '{}': {}", path, e))
    }

    /// Serialise the session as pretty-printed JSON and write it to `path`.
    pub fn write(&self, path: &str) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize session: {}", e))?;
        std::fs::write(path, content)
            .map_err(|e| format!("cannot write session file '{}': {}", path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_round_trip_empty() {
        let s = SessionFile {
            source_file: "foo.ali".into(),
            type_name: "T".into(),
            source_diagram: "lhs".into(),
            target_diagram: Some("rhs".into()),
            moves: vec![],
        };
        let json = serde_json::to_string(&s).unwrap();
        let s2: SessionFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s2.source_file, s.source_file);
        assert_eq!(s2.type_name, s.type_name);
        assert_eq!(s2.moves.len(), 0);
    }

    #[test]
    fn session_round_trip_with_moves() {
        let s = SessionFile {
            source_file: "bar.ali".into(),
            type_name: "Cat".into(),
            source_diagram: "d".into(),
            target_diagram: None,
            moves: vec![
                Move { choice: Some(0), rule_name: "assoc".into(), parallel: false },
                Move { choice: Some(1), rule_name: "unit".into(), parallel: false },
            ],
        };
        let json = serde_json::to_string_pretty(&s).unwrap();
        let s2: SessionFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s2.moves.len(), 2);
        assert_eq!(s2.moves[0].rule_name, "assoc");
        assert_eq!(s2.moves[1].choice, Some(1));
        assert!(s2.target_diagram.is_none());
    }
}
