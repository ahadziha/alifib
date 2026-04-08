//! JSON request/response types for the `alifib serve` daemon protocol.
//!
//! Communication is via stdin/stdout JSON-lines: one JSON object per line
//! in each direction. The daemon processes requests sequentially.
//!
//! # Request format
//!
//! ```json
//! {"command":"init","source_file":"Idem.ali","type_name":"Idem","source_diagram":"lhs"}
//! {"command":"step","choice":0}
//! {"command":"undo"}
//! {"command":"show"}
//! ```
//!
//! # Response format
//!
//! Every response is either `{"status":"ok","data":{...}}` or
//! `{"status":"error","message":"..."}`.

use serde::{Deserialize, Serialize};

use crate::core::complex::Complex;
use crate::core::diagram::{CellData, Diagram};
use crate::core::rewrite::CandidateRewrite;
use crate::output::render_diagram;
use super::engine::RewriteEngine;
use super::render::render_match_highlight;

// ── Requests ─────────────────────────────────────────────────────────────────

/// A request sent by the client to the daemon.
#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    /// Start a new rewrite session.
    Init {
        source_file: String,
        type_name: String,
        source_diagram: String,
        #[serde(default)]
        target_diagram: Option<String>,
    },
    /// Resume from a session file on disk.
    Resume {
        session_file: String,
    },
    /// Apply the rewrite at the given choice index.
    Step {
        choice: usize,
    },
    /// Undo the last step.
    Undo,
    /// Undo back to a specific step count.
    UndoTo {
        step: usize,
    },
    /// Return the current state (no mutation).
    Show,
    /// Save the current session to a file.
    Save {
        path: String,
    },
    /// List all rewrite rules in the type at the current dimension.
    ListRules,
    /// Return the full move history.
    History,
    /// Shut down the daemon.
    Shutdown,
}

// ── Responses ─────────────────────────────────────────────────────────────────

/// The top-level response envelope.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok { data: ResponseData },
    Error { message: String },
}

impl Response {
    pub fn error(msg: impl Into<String>) -> Self {
        Self::Error { message: msg.into() }
    }
}

/// The payload of a successful response.
#[derive(Debug, Serialize)]
pub struct ResponseData {
    pub step_count: usize,
    pub current: DiagramInfo,
    pub source: DiagramInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<DiagramInfo>,
    pub target_reached: bool,
    pub rewrites: Vec<RewriteInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<ProofInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<HistoryEntry>,
    /// All rewrite rules at the current dimension; only populated by `list_rules`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RuleInfo>,
}

/// Rich information about a diagram for client display.
#[derive(Debug, Serialize)]
pub struct DiagramInfo {
    /// Space-separated top-level label string, e.g. `"id id id"`.
    pub label: String,
    /// Dimension of the diagram.
    pub dim: usize,
    /// Number of top-dimensional cells.
    pub cell_count: usize,
    /// Labels at each dimension, from 0 up to top_dim.
    pub cells_by_dim: Vec<DimSlice>,
}

/// The cells at a single dimension.
#[derive(Debug, Serialize)]
pub struct DimSlice {
    pub dim: usize,
    pub cells: Vec<String>,
}

/// Rich information about a single candidate rewrite.
#[derive(Debug, Serialize)]
pub struct RewriteInfo {
    pub index: usize,
    pub rule_name: String,
    pub source: DiagramInfo,
    pub target: DiagramInfo,
    /// Positions of the matched top-dim cells within the current diagram.
    pub match_positions: Vec<usize>,
    /// Current diagram with matched cells bracketed, e.g. `"[id id] id"`.
    pub match_display: String,
}

/// A rewrite rule (n+1-generator) in the type, independent of the current diagram.
#[derive(Debug, Serialize)]
pub struct RuleInfo {
    pub name: String,
    pub source: DiagramInfo,
    pub target: DiagramInfo,
}

/// A summary of the running proof diagram.
#[derive(Debug, Serialize)]
pub struct ProofInfo {
    pub dim: usize,
    pub step_count: usize,
    pub source_label: String,
    pub target_label: String,
}

/// A single entry in the move history.
#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub step: usize,
    pub rule_name: String,
    pub choice: usize,
}

// ── Builders ──────────────────────────────────────────────────────────────────

/// Build a [`DiagramInfo`] from a diagram and its rendering scope.
pub fn diagram_info(diagram: &Diagram, scope: &Complex) -> DiagramInfo {
    let dim = diagram.top_dim();
    let label = render_diagram(diagram, scope);
    let cell_count = diagram.labels_at(dim).map(|ls| ls.len()).unwrap_or(0);

    let cells_by_dim = (0..=dim)
        .filter_map(|d| {
            diagram.labels_at(d).map(|labels| DimSlice {
                dim: d,
                cells: labels
                    .iter()
                    .map(|tag| {
                        scope
                            .find_generator_by_tag(tag)
                            .filter(|n| !n.is_empty())
                            .cloned()
                            .unwrap_or_else(|| format!("{}", tag))
                    })
                    .collect(),
            })
        })
        .collect();

    DiagramInfo { label, dim, cell_count, cells_by_dim }
}

/// Build the standard [`ResponseData`] from an engine snapshot.
pub fn build_response(engine: &RewriteEngine, include_history: bool) -> ResponseData {
    let scope = engine.type_complex();
    let current = engine.current_diagram();

    let rewrites: Vec<RewriteInfo> = engine
        .available_rewrites()
        .iter()
        .enumerate()
        .map(|(i, c)| build_rewrite_info(i, c, current, scope))
        .collect();

    let proof = engine.running_diagram().and_then(|d| {
        let n = engine.source_diagram().top_dim();
        let src = crate::core::diagram::Diagram::boundary(
            crate::core::diagram::Sign::Source, n, d,
        ).ok()?;
        let tgt = crate::core::diagram::Diagram::boundary(
            crate::core::diagram::Sign::Target, n, d,
        ).ok()?;
        Some(ProofInfo {
            dim: d.top_dim(),
            step_count: engine.step_count(),
            source_label: render_diagram(&src, scope),
            target_label: render_diagram(&tgt, scope),
        })
    });

    let history = if include_history {
        engine.history_moves()
            .enumerate()
            .map(|(i, m)| HistoryEntry {
                step: i + 1,
                rule_name: m.rule_name.clone(),
                choice: m.choice,
            })
            .collect()
    } else {
        vec![]
    };

    ResponseData {
        step_count: engine.step_count(),
        current: diagram_info(current, scope),
        source: diagram_info(engine.source_diagram(), scope),
        target: engine.target_diagram().map(|t| diagram_info(t, scope)),
        target_reached: engine.target_reached(),
        rewrites,
        proof,
        history,
        rules: vec![],
    }
}

/// Build a [`ResponseData`] that lists all (n+1)-generators in the type,
/// regardless of whether they match the current diagram.
pub fn build_list_rules_response(engine: &RewriteEngine) -> ResponseData {
    let scope = engine.type_complex();
    let store = engine.store();
    let n = engine.current_diagram().top_dim();

    let rules: Vec<RuleInfo> = scope
        .generators_iter()
        .filter(|(_, _, dim)| *dim == n + 1)
        .filter_map(|(name, tag, _)| {
            match store.cell_data_for_tag(scope, tag)? {
                CellData::Boundary { boundary_in, boundary_out } => Some(RuleInfo {
                    name: name.clone(),
                    source: diagram_info(&boundary_in, scope),
                    target: diagram_info(&boundary_out, scope),
                }),
                CellData::Zero => None,
            }
        })
        .collect();

    let mut data = build_response(engine, false);
    data.rules = rules;
    data
}

fn build_rewrite_info(
    index: usize,
    candidate: &CandidateRewrite,
    current: &Diagram,
    scope: &Complex,
) -> RewriteInfo {
    let match_display =
        render_match_highlight(current, scope, &candidate.image_positions);
    RewriteInfo {
        index,
        rule_name: candidate.rule_name.clone(),
        source: diagram_info(&candidate.source_boundary, scope),
        target: diagram_info(&candidate.target_boundary, scope),
        match_positions: candidate.image_positions.clone(),
        match_display,
    }
}
