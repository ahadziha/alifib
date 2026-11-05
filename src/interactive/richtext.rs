//! Medium-neutral structured rendering of a command's [`Outcome`].
//!
//! A single producer turns an answer into [`RichText`] — lines of role-tagged
//! [`Segment`]s — which each front-end then styles in its own medium: the CLI to
//! ANSI (the CLI crate's `Display`), the browser to CSS spans. Layout and content
//! live here **once**, so they cannot drift between front-ends; only the
//! role→colour mapping is declared per
//! medium (which is correct — styling *should* differ per medium).
//!
//! Plain-mode invariant: concatenating every segment's `text` (with [`Role::Redex`]
//! wrapped in `[…]`) reproduces the uncoloured transcript exactly.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::response::{
    AutoInfo, ConstraintInfo, DiagramInfo, FillState, HistoryEntry, HoleInfo,
    HomologyInfo, Outcome, Proof, RewriteState, RuleInfo, State, StoredInfo,
    TypeInfo, ZeroCellInfo,
};

/// A semantic role for a run of text — mapped to a colour per medium, never
/// chosen by the front-end.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Indentation, punctuation, connectives, the `→` arrow (default foreground).
    Plain,
    /// Field labels and secondary text: `step:`, `match:`, `= expr` (dim).
    Label,
    /// Names, counts, diagrams (bright).
    Value,
    /// The input side of a boundary (one "syntax" colour).
    Src,
    /// The output side of a boundary (a contrasting colour).
    Tgt,
    /// Section titles: `available rewrites:`, `Diagrams`, `Maps` (emphasised).
    Section,
    /// Success: `✓ reached`, `Stored '…'`.
    Ok,
    /// The matched redex inside a `match:` line (highlighted; `[bracketed]` when
    /// rendered without colour).
    Redex,
}

/// One styled run of text.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Segment {
    pub role: Role,
    pub text: String,
}

/// A line is a sequence of segments; serialized as a JSON array of `{role,text}`.
pub type Line = Vec<Segment>;

/// A rendered transcript block: lines of segments.  Serializes as
/// `{ "lines": [ [ {role,text}, … ], … ] }`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RichText {
    pub lines: Vec<Line>,
}

impl RichText {
    fn new() -> Self {
        Self::default()
    }

    /// Begin a new (empty) line; subsequent pushes append to it.  An empty line
    /// is a blank transcript row.
    fn line(&mut self) -> &mut Self {
        self.lines.push(Vec::new());
        self
    }

    fn push(&mut self, role: Role, text: impl AsRef<str>) -> &mut Self {
        let seg = Segment { role, text: text.as_ref().to_owned() };
        match self.lines.last_mut() {
            Some(l) => l.push(seg),
            None => self.lines.push(vec![seg]),
        }
        self
    }

    /// Append pre-built segments (e.g. a parsed match line) to the current line.
    fn extend(&mut self, segs: Vec<Segment>) -> &mut Self {
        match self.lines.last_mut() {
            Some(l) => l.extend(segs),
            None => self.lines.push(segs),
        }
        self
    }

    fn plain(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Plain, t) }
    fn label(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Label, t) }
    fn value(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Value, t) }
    fn src(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Src, t) }
    fn tgt(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Tgt, t) }
    fn section(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Section, t) }
    fn ok(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Ok, t) }
}

impl fmt::Display for RichText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 { writeln!(f)?; }
            for segment in line {
                match segment.role {
                    Role::Redex => write!(f, "[{}]", segment.text)?,
                    _ => f.write_str(&segment.text)?,
                }
            }
        }
        Ok(())
    }
}

// ── Dispatch ─────────────────────────────────────────────────────────────────

/// Render an outcome, reading the state only when the view shows it.
pub fn render(outcome: &Outcome, state: impl FnOnce() -> State) -> RichText {
    match outcome {
        Outcome::Help => help(),
        Outcome::State => render_state(&state()),
        Outcome::Auto(info) => auto(info, &state()),
        Outcome::Types(info) => types(info),
        Outcome::Type(info) => type_detail(info),
        Outcome::Homology(info) => homology(info),
        Outcome::Holes { holes: open, constraints } => holes(open, constraints),
        Outcome::Rules(info) => rules(info),
        Outcome::History(entries) => history(entries),
        Outcome::Proof(running) => proof(running.as_ref()),
        Outcome::Stored(info) => store(info),
        Outcome::Source(source) => {
            let mut t = RichText::new();
            // Keep the source's final newline as a trailing empty transcript row.
            for line in source.split('\n') {
                t.line().value(line);
            }
            t
        }
    }
}

// ── Producers ────────────────────────────────────────────────────────────────

fn render_state(state: &State) -> RichText {
    match state {
        State::Unloaded => {
            let mut t = RichText::new();
            t.line().label("no active session");
            t
        }
        State::Idle { module, .. } => {
            let mut t = RichText::new();
            t.line().label("module:").plain(" ").value(module);
            t
        }
        State::Rewrite { rewrite: data, .. }
        | State::Fill { by: FillState::Rewrite(data), .. } => rewrite(data),
        State::Fill { by: FillState::ZeroCell(zc), .. } => zero_cell(zc),
    }
}

/// An active rewrite state: step count, current/target diagrams, and the
/// available rewrites (each `in → out` with its bracketed match), or none.
fn rewrite(data: &RewriteState) -> RichText {
    let mut t = RichText::new();

    t.line().label("step:").plain(" ").value(data.step_count.to_string());

    let cur = &data.current;
    let label = if cur.label.is_empty() { "—" } else { cur.label.as_str() };
    t.line().label("current:").plain(" ").value(label);

    if let Some(tg) = &data.target {
        t.line().label("target:").plain(" ").value(&tg.label);
        if data.target_reached {
            t.plain(" ").ok("✓ reached");
        }
    }

    if data.rewrites.is_empty() {
        t.line().label("no rewrites available");
    } else {
        t.line();
        t.line().section("available rewrites:");
        for r in &data.rewrites {
            t.line().plain("  [").value(r.index.to_string()).plain("] ").value(&r.rule_name);
            if r.family.is_empty() {
                t.plain("  ").src(&r.input.label).plain(" → ").tgt(&r.output.label);
            } else {
                t.plain(format!("  (parallel ×{})", r.family.len()));
            }
            if !r.match_display.is_empty() {
                t.line().plain("      ").label("match:").plain(" ");
                t.extend(match_segments(&r.match_display));
            }
        }
    }
    t
}

/// A boundaryless 0-cell fill: synthetic step count, a target-reached banner once
/// a cell is chosen, and the candidate 0-cells while unchosen.
fn zero_cell(zc: &ZeroCellInfo) -> RichText {
    let mut t = RichText::new();
    t.line().label("step:").plain(" ").value(if zc.chosen.is_some() { "1" } else { "0" });
    if zc.target_reached {
        t.line().ok("✓ target reached");
    }
    if zc.choices.is_empty() {
        t.line().label("no rewrites available");
    } else {
        t.line();
        t.line().section("available rewrites:");
        for c in &zc.choices {
            t.line().plain("  [").value(c.index.to_string()).plain("] ").value(&c.name);
        }
    }
    t
}

/// An `auto`/`random` run: the summary line then the resulting state.
fn auto(info: &AutoInfo, state: &State) -> RichText {
    let applied = info.applied;
    let reason = info.stop_reason.map(|r| format!(" ({})", r)).unwrap_or_default();
    let mut t = RichText::new();
    t.line().label(format!("applied {} step{}{}", applied, if applied == 1 { "" } else { "s" }, reason));
    t.lines.extend(render_state(state).lines);
    t
}

/// The result of `store`: the confirmation and the appended `let` clause.
fn store(s: &StoredInfo) -> RichText {
    let mut t = RichText::new();
    t.line().ok(format!("Stored '{}'", s.def_name));
    t.line().plain("  let ").value(&s.def_name).plain(" = ").label(&s.expr);
    t
}

/// The module's open holes, numbered for `fill <n>`, followed by the constraints
/// that conditional pending assignments impose on those holes.
fn holes(holes: &[HoleInfo], constraints: &[ConstraintInfo]) -> RichText {
    let mut t = RichText::new();
    if holes.is_empty() && constraints.is_empty() {
        t.line().label("(no open holes)");
        return t;
    }
    if !holes.is_empty() {
        t.line().section("open holes:");
        for h in holes {
            t.line().plain("  [").value(h.index.to_string()).plain("] ");
            map_header(&mut t, &h.type_name, &h.map_name, &h.domain_name);
            t.line().plain("      ");
            hole_into(&mut t, &h.boundary);
        }
    }
    if !constraints.is_empty() {
        t.line().section("constraints:");
        for c in constraints {
            t.line().plain("  ");
            map_header(&mut t, &c.type_name, &c.map_name, &c.domain_name);
            for eq in &c.equations {
                t.line().plain("    ");
                match eq.split_once(" = ") {
                    Some((lhs, rhs)) => { t.value(lhs).plain(" = ").value(rhs); }
                    None => { t.value(eq); }
                }
            }
        }
    }
    t
}

/// The type summaries for `types`: one line each, `name (dim …, N generators, …)`.
fn types(types: &[TypeInfo]) -> RichText {
    let mut t = RichText::new();
    if types.is_empty() {
        t.line().label("  (No types found)");
        return t;
    }
    for ty in types {
        let mut parts = Vec::new();
        let max_dim = ty.generators.iter().map(|g| g.dim).max();
        let generator_count = ty.generators.len();
        let diagram_count = ty.diagrams.len();
        let map_count = ty.maps.len();
        if let Some(d) = max_dim { parts.push(format!("dim {}", d)); }
        if generator_count > 0 { parts.push(format!("{} generator{}", generator_count, plural(generator_count))); }
        if diagram_count > 0 { parts.push(format!("{} diagram{}", diagram_count, plural(diagram_count))); }
        if map_count > 0 { parts.push(format!("{} map{}", map_count, plural(map_count))); }
        t.line().plain("  ").value(&ty.name);
        if !parts.is_empty() {
            t.plain(" ").label(format!("({})", parts.join(", ")));
        }
    }
    t
}

/// The full detail of a type: `generators:` by dimension, `diagrams:` with
/// `= expr`, and `maps:` (flagged `… with holes`, each hole's boundary shown).
fn type_detail(d: &TypeInfo) -> RichText {
    let mut t = RichText::new();

    if !d.generators.is_empty() {
        t.line().section("generators:");
        let mut last_dim: Option<usize> = None;
        for g in &d.generators {
            if last_dim != Some(g.dim) {
                t.line().plain("  ").label(format!("[{}]", g.dim));
                last_dim = Some(g.dim);
            }
            t.line().plain("    ");
            boundary_into(&mut t, &g.name, &g.input, &g.output);
        }
    }

    if !d.diagrams.is_empty() {
        t.line().section("diagrams:");
        for g in &d.diagrams {
            t.line().plain("  ");
            boundary_into(&mut t, &g.name, &g.input, &g.output);
            t.line().plain("    = ").label(&g.expr);
        }
    }

    if !d.maps.is_empty() {
        t.line().section("maps:");
        for m in &d.maps {
            t.line().plain("  ").value(&m.name).plain(" :: ").label(&m.domain);
            if !m.holes.is_empty() {
                t.label(" with holes");
            }
            for hole in &m.holes {
                t.line().plain("    ");
                hole_into(&mut t, hole);
            }
        }
    }
    t
}

/// `name : in → out` for a cell with a boundary, appended to the current line; or
/// just `name` for a 0-cell.
fn boundary_into(t: &mut RichText, name: &str, input: &Option<DiagramInfo>, output: &Option<DiagramInfo>) {
    match (input, output) {
        (Some(i), Some(o)) => {
            t.value(name).plain(" : ").src(&i.label).plain(" → ").tgt(&o.label);
        }
        _ => { t.value(name); }
    }
}

/// A map hole's pre-rendered boundary (`?name : in → out`, or `?name` for a
/// 0-cell), coloured like a diagram: name white, input amber, output cyan.  The
/// ` : ` and ` → ` connectives are the only separators in the string.
fn hole_into(t: &mut RichText, hole: &str) {
    match hole.split_once(" : ") {
        Some((name, bound)) => match bound.split_once(" → ") {
            Some((inp, out)) => { t.value(name).plain(" : ").src(inp).plain(" → ").tgt(out); }
            None => { t.value(name).plain(" : ").src(bound); }
        },
        None => { t.value(hole); }
    }
}

/// A map's `@type name :: domain` header, appended to the current line, coloured
/// as in the `type` view: name bright, `::` punctuation, domain dim.  The `@type`
/// locator is dim, the line being keyed by the map.
fn map_header(t: &mut RichText, type_name: &str, map_name: &str, domain_name: &str) {
    t.label(format!("@{}", type_name)).plain(" ")
        .value(map_name).plain(" :: ").label(domain_name);
}

/// The rewrite rules at the current dimension for `rules`.
fn rules(rules: &[RuleInfo]) -> RichText {
    let mut t = RichText::new();
    if rules.is_empty() {
        t.line().label("(no rules)");
        return t;
    }
    for r in rules {
        t.line().plain("  ").value(&r.name).plain("  ").label(&r.input.label).plain(" → ").label(&r.output.label);
    }
    t
}

/// The running proof for `proof`: the re-parseable expression `store` would
/// persist, headed by its boundary (none for a 0-cell).  A zero-step session is
/// the identity proof on the initial diagram, so only an unchosen 0-cell fill
/// has no proof.
fn proof(running: Option<&Proof>) -> RichText {
    let mut t = RichText::new();
    let Some(running) = running else {
        t.line().label("(no proof yet)");
        return t;
    };
    t.line().label("proof :");
    if let Some((input, output)) = &running.boundary {
        t.plain(" ").src(input).plain(" → ").tgt(output);
    }
    for line in running.expr.lines() {
        t.line().plain("  ").value(line);
    }
    t
}

/// The move history for `history`.
fn history(entries: &[HistoryEntry]) -> RichText {
    let mut t = RichText::new();
    if entries.is_empty() {
        t.line().label("(no moves yet)");
        return t;
    }
    for h in entries {
        let choice = match &h.choice {
            None => "[n/a]".to_owned(),
            Some(v) => format!("[choice {}]", v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")),
        };
        t.line().plain("  ").label(format!("{}.", h.step)).plain(" ").value(&h.rule_name).plain(" ").label(choice);
    }
    t
}

/// The cellular homology of a type: `H_d = …` lines and `χ = …`.
fn homology(h: &HomologyInfo) -> RichText {
    let mut t = RichText::new();
    if h.groups.is_empty() {
        t.line().label("(no generators)");
        return t;
    }
    for g in &h.groups {
        t.line().plain("  ").label(format!("H_{}", g.dim)).plain(" = ").value(&g.display);
        for w in &g.witnesses {
            t.line()
                .plain("      ").label(format!("Z/{}", w.order)).plain(" cycle: ").value(&w.cycle)
                .plain("  (preimage: ").value(&w.preimage).plain(")");
        }
    }
    t.line().plain("  ").label("χ").plain(" = ").value(h.euler_characteristic.to_string());
    t
}

fn plural(n: usize) -> &'static str { if n == 1 { "" } else { "s" } }

// ── Help ───────────────────────────────────────────────────────────────────

/// `(token, description)` — always-available commands.
const ALWAYS: &[(&str, &str)] = &[
    ("load [<path>]",     "Load a module, or reload the last load — quote a path with spaces"),
    ("types",            "List all types in the file"),
    ("type <name>",      "Inspect a type: generators, diagrams, maps"),
    ("homology <name>",  "Compute cellular homology of a type"),
    ("start <t> <s> [<g>]", "Start a rewrite session (target optional) — group a multi-word diagram in ( )"),
    ("resume <t> <p> [<g>]", "Resume a session from a diagram (target optional)"),
    ("holes",            "List open holes of maps in this module"),
    ("fill <n>",         "Start a hole-filling session for hole <n>"),
    ("backward [on|off]", "Show or toggle backward rewrite mode (default: off)"),
    ("status / show",    "Session state, or module info when idle"),
    ("print",            "Print the running source"),
    ("save [<path>]",     "Write the running source to disk"),
    ("stop",             "End the active session"),
    ("help / ?",         "Show this help"),
];

/// `(token, description)` — commands needing an active session.
const SESSION: &[(&str, &str)] = &[
    ("apply <n> [<n2>..]", "Apply rewrite(s) at given indices (alias: a)"),
    ("auto <n>",         "Apply up to <n> rewrites automatically"),
    ("random <n>",       "Apply randomly selected rewrites"),
    ("parallel [on|off]", "Show or toggle parallel rewrite mode (default: on)"),
    ("undo [<n>]",       "Undo the last step, or back to step <n> (alias: u)"),
    ("redo [<n>]",       "Redo the last undone step, or forward to step <n>"),
    ("undo all / restart", "Reset to the initial diagram"),
    ("rules",            "List rewrite rules at current dimension (alias: r)"),
    ("history",          "Show the move history (alias: h)"),
    ("proof",            "Show the running proof diagram (alias: p)"),
    ("store <name>",     "Store the current proof as a named diagram"),
    ("done",             "Finalise the hole-filling session"),
];

/// The command help, shared by every front end.
/// The command token is coloured like an alifib expression; descriptions plain.
fn help() -> RichText {
    let mut t = RichText::new();
    t.line().plain("Always available:");
    for &(tok, desc) in ALWAYS {
        help_line(&mut t, tok, desc);
    }
    t.line();
    t.line().plain("Session commands (require active session):");
    for &(tok, desc) in SESSION {
        help_line(&mut t, tok, desc);
    }
    t.line();
    t.line().plain("Front ends: quit / exit / q (terminal) · clear, ↑/↓ history, Ctrl+Enter evaluate (browser)");
    t
}

/// One help row: `  <token padded to 20> <description>`, the token in the value
/// colour, measured on its plain length for alignment.
fn help_line(t: &mut RichText, tok: &str, desc: &str) {
    let pad = " ".repeat(20usize.saturating_sub(tok.len()).max(1));
    t.line().plain("  ").value(tok).plain(format!("{pad}{desc}"));
}

/// Split a bracketed match string like `(a #0 [idem]) #0 b` into segments:
/// `[…]` contents become [`Role::Redex`], everything else [`Role::Label`] (the
/// dim match base).  The brackets are dropped — the styler re-adds them when
/// rendering without colour.
fn match_segments(s: &str) -> Vec<Segment> {
    let mut segs = Vec::new();
    let mut buf = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '[' {
            if !buf.is_empty() {
                segs.push(Segment { role: Role::Label, text: std::mem::take(&mut buf) });
            }
            let mut redex = String::new();
            for c2 in chars.by_ref() {
                if c2 == ']' { break; }
                redex.push(c2);
            }
            segs.push(Segment { role: Role::Redex, text: redex });
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        segs.push(Segment { role: Role::Label, text: buf });
    }
    segs
}
