//! CLI argument parsing and command dispatch for the interactive subcommands.
//!
//! # Commands
//!
//! ```text
//! alifib repl <file>    [--type <t>] [--source <s>] [--target <t>] [--emacs]
//! alifib serve          [<file> --type <t> --source <s> [--target <t>]]
//! alifib web            [<examples-dir>] [--bind <addr>]
//! ```

use super::engine::RewriteEngine;
#[cfg(feature = "cli")]
use super::repl::run_repl;

// ── Argument types ────────────────────────────────────────────────────────────

/// Arguments for the `alifib repl` subcommand.
pub struct ReplArgs {
    /// Path to the `.ali` source file.
    pub file: String,
    /// Pre-selected type name (set via `@ <TypeName>` interactively if absent).
    pub type_name: Option<String>,
    /// Pre-selected source diagram name.
    pub source: Option<String>,
    /// Pre-selected target diagram name.
    pub target: Option<String>,
    /// Use Emacs keybindings instead of the default vi mode.
    pub emacs: bool,
}

/// Arguments for the `alifib serve` subcommand.
///
/// All fields are optional: `alifib serve` with no arguments starts blank
/// and waits for an `Init` request; with arguments it pre-loads the session
/// and emits an initial state response before entering the request loop.
pub struct ServeArgs {
    /// Path to the `.ali` source file (required if any of the others are given).
    pub file: Option<String>,
    /// Name of the type to load (required if `file` is given).
    pub type_name: Option<String>,
    /// Name of the source diagram (required if `file` is given).
    pub source: Option<String>,
    /// Optional name of the target (goal) diagram.
    pub target: Option<String>,
}

/// Arguments for the `alifib web` subcommand.
pub struct WebArgs {
    /// Address to bind the localhost HTTP server to.
    pub bind: String,
    /// Directory of `.ali` files to expose as examples (and as
    /// include targets).  Defaults to `./examples` when absent.
    pub examples_dir: Option<String>,
}

// ── Parsers ───────────────────────────────────────────────────────────────────

const REPL_USAGE: &str = "\
Usage: alifib repl <file> [--type <t>] [--source <s>] [--target <t>] [--emacs]
";

const WEB_USAGE: &str = "\
Usage: alifib web [<examples-dir>] [--bind <addr>]

  <examples-dir>  Scan this directory for *.ali files and expose them as
                  examples (and include targets).  Rescanned on every
                  /api/load_source and /examples/index.json request, so
                  edits show up live.  Defaults to ./examples.
";

/// Parse the arguments following `alifib repl`.
pub fn parse_repl_args(args: &[String]) -> Result<ReplArgs, String> {
    let mut file = None;
    let mut type_name = None;
    let mut source = None;
    let mut target = None;
    let mut emacs = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--type"   => { type_name = Some(next_arg(&mut it, "--type")?); }
            "--source" => { source    = Some(next_arg(&mut it, "--source")?); }
            "--target" => { target    = Some(next_arg(&mut it, "--target")?); }
            "--emacs"  => { emacs = true; }
            "-h" | "--help" => return Err(REPL_USAGE.to_string()),
            s if s.starts_with('-') => {
                return Err(format!("unknown option '{}' for repl\n{}", s, REPL_USAGE));
            }
            s => {
                if file.is_some() {
                    return Err("repl: multiple input files specified".to_string());
                }
                file = Some(s.to_string());
            }
        }
    }

    Ok(ReplArgs {
        file:      file.ok_or("repl: <file> argument is required")?,
        type_name,
        source,
        target,
        emacs,
    })
}

/// Parse the arguments following `alifib web`.
pub fn parse_web_args(args: &[String]) -> Result<WebArgs, String> {
    let mut bind = "127.0.0.1:8000".to_string();
    let mut examples_dir = None;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bind" => { bind = next_arg(&mut it, "--bind")?; }
            "-h" | "--help" => return Err(WEB_USAGE.to_string()),
            s if s.starts_with('-') => {
                return Err(format!("unknown option '{}' for web\n{}", s, WEB_USAGE));
            }
            s => {
                if examples_dir.is_some() {
                    return Err(format!("web: multiple positional arguments\n{}", WEB_USAGE));
                }
                examples_dir = Some(s.to_string());
            }
        }
    }

    Ok(WebArgs { bind, examples_dir })
}

/// Parse the arguments following `alifib serve`.
pub fn parse_serve_args(args: &[String]) -> Result<ServeArgs, String> {
    let mut file = None;
    let mut type_name = None;
    let mut source = None;
    let mut target = None;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--type"   => { type_name = Some(next_arg(&mut it, "--type")?); }
            "--source" => { source    = Some(next_arg(&mut it, "--source")?); }
            "--target" => { target    = Some(next_arg(&mut it, "--target")?); }
            "-h" | "--help" => return Err("Usage: alifib serve [<file> --type <t> --source <s> [--target <t>]]\n".to_string()),
            s if s.starts_with('-') => {
                return Err(format!("unknown option '{}' for serve", s));
            }
            s => {
                if file.is_some() {
                    return Err("serve: multiple input files specified".to_string());
                }
                file = Some(s.to_string());
            }
        }
    }

    Ok(ServeArgs { file, type_name, source, target })
}

fn next_arg<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<String, String> {
    it.next()
        .ok_or_else(|| format!("{} requires an argument", flag))
        .cloned()
}

// ── Dispatchers ───────────────────────────────────────────────────────────────

/// Run the daemon, optionally pre-loading a session from the given arguments.
#[allow(clippy::result_unit_err)]
pub fn run_serve_cmd(args: ServeArgs) -> Result<(), ()> {
    use super::daemon::run_daemon;
    let initial = match (args.file, args.type_name, args.source) {
        (Some(file), Some(type_name), Some(source)) => {
            match RewriteEngine::init(&file, &type_name, &source, args.target.as_deref()) {
                Ok(e) => Some(e),
                Err(e) => { eprintln!("error: {}", e); return Err(()); }
            }
        }
        (None, None, None) => None,
        _ => {
            eprintln!("error: serve: if any of <file>, --type, --source are given, all three are required");
            return Err(());
        }
    };
    run_daemon(initial)
}

/// Run the REPL with the given arguments.
#[cfg(feature = "cli")]
#[allow(clippy::result_unit_err)]
pub fn run_repl_cmd(args: ReplArgs) -> Result<(), ()> {
    run_repl(&args.file, args.type_name.as_deref(), args.source.as_deref(), args.target.as_deref(), args.emacs)
}
