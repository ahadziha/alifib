//! Output types and formatting for interactive rewrite sessions.
//!
//! All CLI commands print either structured JSON or human-readable text,
//! selected by the `--format` flag.

use serde::Serialize;

/// The output format for a CLI command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable plain text (default).
    Text,
    /// Machine-readable JSON (for programmatic consumption).
    Json,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            other => Err(format!("unknown format '{}': expected 'text' or 'json'", other)),
        }
    }
}

/// The status of a rewrite session response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Error,
    Done,
}

/// A single available rewrite at the current state.
#[derive(Debug, Serialize)]
pub struct AvailableRewrite {
    pub index: usize,
    pub rule_name: String,
    pub rule_source: String,
    pub rule_target: String,
}

/// The JSON/text response emitted by every CLI command.
#[derive(Debug, Serialize)]
pub struct RewriteResponse {
    pub status: Status,
    pub step_count: usize,
    pub current_diagram: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_diagram: Option<String>,
    pub target_reached: bool,
    pub available_rewrites: Vec<AvailableRewrite>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RewriteResponse {
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            status: Status::Error,
            step_count: 0,
            current_diagram: String::new(),
            target_diagram: None,
            target_reached: false,
            available_rewrites: vec![],
            error: Some(msg.into()),
        }
    }

    /// Print the response to stdout in the requested format.
    ///
    /// Errors are printed to stderr in text mode, and inlined in JSON mode.
    pub fn print(&self, format: OutputFormat) {
        match format {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(self).unwrap());
            }
            OutputFormat::Text => self.print_text(),
        }
    }

    fn print_text(&self) {
        if let Status::Error = self.status {
            eprintln!("error: {}", self.error.as_deref().unwrap_or("unknown"));
            return;
        }

        println!("step: {}", self.step_count);
        println!("current: {}", self.current_diagram);
        if let Some(target) = &self.target_diagram {
            println!("target:  {}", target);
        }
        if self.target_reached {
            println!("target reached.");
        }

        if self.available_rewrites.is_empty() {
            println!("no rewrites available.");
        } else {
            println!("\navailable rewrites:");
            for r in &self.available_rewrites {
                println!("  [{}] {}  :  {}  ->  {}", r.index, r.rule_name, r.rule_source, r.rule_target);
            }
        }
    }
}
