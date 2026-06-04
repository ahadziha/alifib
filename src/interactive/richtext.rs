//! Medium-neutral structured rendering of a [`ResponseData`].
//!
//! A single producer turns a response into [`RichText`] — lines of role-tagged
//! [`Segment`]s — which each front-end then styles in its own medium: the CLI to
//! ANSI (via [`Display::style`](super::display::Display::style)), the web to CSS
//! spans.  Layout and content live here **once**, so they cannot drift between
//! front-ends; only the role→colour mapping is declared per medium (which is
//! correct — styling *should* differ per medium).
//!
//! Plain-mode invariant: concatenating every segment's `text` (with [`Role::Redex`]
//! wrapped in `[…]`) reproduces the uncoloured transcript exactly.

use serde::Serialize;

use super::protocol::{
    ConstraintInfo, DiagramInfo, HoleInfo, HomologyInfo, Request, ResponseData, RuleInfo,
    TypeDetailInfo, TypeSummaryInfo, ZeroCellInfo,
};

/// A semantic role for a run of text — mapped to a colour per medium, never
/// chosen by the front-end.
#[derive(Clone, Copy, Debug, Serialize)]
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
#[derive(Clone, Debug, Serialize)]
pub struct Segment {
    pub role: Role,
    pub text: String,
}

/// A line is a sequence of segments; serialized as a JSON array of `{role,text}`.
pub type Line = Vec<Segment>;

/// A rendered transcript block: lines of segments.  Serializes as
/// `{ "lines": [ [ {role,text}, … ], … ] }`.
#[derive(Clone, Debug, Default, Serialize)]
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

// ── Dispatch ─────────────────────────────────────────────────────────────────

/// Which view to render for a given response.
#[derive(Clone, Copy, Debug)]
pub enum RenderKind {
    State,
    Auto,
    Rules,
    History,
    Proof,
    Store,
    Holes,
    Types,
    TypeDetail,
    Homology,
}

/// The render kind for a session request, or `None` for requests whose reply is
/// a single canonical message (`stop`/`done`/`save`/`backward`), the
/// bespoke `parallel` mode line, or a non-rendered request.
pub fn render_kind_for(req: &Request) -> Option<RenderKind> {
    use Request::*;
    Some(match req {
        Start { .. } | Resume { .. } | Step { .. } | StepMulti { .. } | Undo | UndoTo { .. }
        | Redo | RedoTo { .. } | Show | SetTarget { .. } | Fill { .. } => RenderKind::State,
        Auto { .. } | Random { .. } => RenderKind::Auto,
        ListRules => RenderKind::Rules,
        History => RenderKind::History,
        Proof => RenderKind::Proof,
        Store { .. } => RenderKind::Store,
        Holes => RenderKind::Holes,
        Types => RenderKind::Types,
        TypeInfo { .. } => RenderKind::TypeDetail,
        Homology { .. } => RenderKind::Homology,
        Parallel { .. } | Stop | Backward { .. } | Save { .. } | Done | Load { .. }
        | Cell { .. } | Help { .. } | Shutdown => return None,
    })
}

/// Render a response in the requested view.
pub fn render_response(kind: RenderKind, data: &ResponseData) -> RichText {
    match kind {
        RenderKind::State => match &data.zero_cell {
            Some(zc) => zero_cell(zc),
            None => state(data),
        },
        RenderKind::Auto => auto(data),
        RenderKind::Rules => rules(&data.rules),
        RenderKind::History => history(data),
        RenderKind::Proof => proof(data),
        RenderKind::Store => store(data),
        RenderKind::Holes => holes(&data.holes, &data.constraints),
        RenderKind::Types => types(&data.types),
        RenderKind::TypeDetail => data.type_detail.as_ref().map(type_detail).unwrap_or_default(),
        RenderKind::Homology => data.homology.as_ref().map(homology).unwrap_or_default(),
    }
}

// ── Producers ────────────────────────────────────────────────────────────────

/// An active rewrite state: step count, current/target diagrams, and the
/// available rewrites (each `in → out` with its bracketed match), or none.
fn state(data: &ResponseData) -> RichText {
    let mut t = RichText::new();

    // Idle: no active session — `show`/`status` reports the loaded module instead.
    if data.current.is_none() {
        match &data.module {
            Some(path) => { t.line().label("module:").plain(" ").value(path); }
            None => { t.line().label("no active session"); }
        }
        return t;
    }

    t.line().label("step:").plain(" ").value(data.step_count.to_string());

    if let Some(cur) = &data.current {
        let label = if cur.label.is_empty() { "—" } else { cur.label.as_str() };
        t.line().label("current:").plain(" ").value(label);
    }

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
fn auto(data: &ResponseData) -> RichText {
    let (applied, reason) = match &data.auto {
        Some(a) => (a.applied, if a.stop_reason.is_empty() { String::new() } else { format!(" ({})", a.stop_reason) }),
        None => (0, String::new()),
    };
    let mut t = RichText::new();
    t.line().label(format!("applied {} step{}{}", applied, if applied == 1 { "" } else { "s" }, reason));
    t.lines.extend(state(data).lines);
    t
}

/// The result of `store`: the confirmation and the appended `let` clause.
fn store(data: &ResponseData) -> RichText {
    let mut t = RichText::new();
    match &data.stored {
        Some(s) => {
            t.line().ok(format!("Stored '{}'", s.def_name));
            t.line().plain("  let ").value(&s.def_name).plain(" = ").label(&s.expr);
        }
        None => { t.line().plain("store failed"); }
    }
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
fn types(types: &[TypeSummaryInfo]) -> RichText {
    let mut t = RichText::new();
    if types.is_empty() {
        t.line().label("  (No types found)");
        return t;
    }
    for ty in types {
        let mut parts = Vec::new();
        if let Some(d) = ty.max_dim { parts.push(format!("dim {}", d)); }
        if ty.generator_count > 0 { parts.push(format!("{} generator{}", ty.generator_count, plural(ty.generator_count))); }
        if ty.diagram_count > 0 { parts.push(format!("{} diagram{}", ty.diagram_count, plural(ty.diagram_count))); }
        if ty.map_count > 0 { parts.push(format!("{} map{}", ty.map_count, plural(ty.map_count))); }
        t.line().plain("  ").value(&ty.name);
        if !parts.is_empty() {
            t.plain(" ").label(format!("({})", parts.join(", ")));
        }
    }
    t
}

/// The full detail of a type: `generators:` by dimension, `diagrams:` with
/// `= expr`, and `maps:` (flagged `… with holes`, each hole's boundary shown).
fn type_detail(d: &TypeDetailInfo) -> RichText {
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
/// persist, headed by its boundary.  A zero-step session is the identity proof on
/// the initial diagram, so this is non-empty for any engine session.
fn proof(data: &ResponseData) -> RichText {
    let mut t = RichText::new();
    let Some(expr) = &data.proof_expr else {
        t.line().label("(no proof yet)");
        return t;
    };
    match &data.proof {
        // A 0-cell proof has no boundary — just the header.
        Some(p) if p.dim == 0 => { t.line().label("proof :"); }
        Some(p) => { t.line().label("proof :").plain(" ").src(&p.input_label).plain(" → ").tgt(&p.output_label); }
        None => { t.line().label("proof:"); }
    }
    for line in expr.lines() {
        t.line().plain("  ").value(line);
    }
    t
}

/// The move history for `history`.
fn history(data: &ResponseData) -> RichText {
    let mut t = RichText::new();
    if data.history.is_empty() {
        t.line().label("(no moves yet)");
        return t;
    }
    for h in &data.history {
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

/// Where a command applies: every front-end, the CLI only, or the web only.
/// The two front-ends share one help table and differ only in `print`/`save`/
/// `quit` (CLI) versus `clear` (web).
#[derive(Clone, Copy)]
enum Scope { All, Cli, Web }

/// `(token, description, scope)` — always-available commands.
const ALWAYS: &[(&str, &str, Scope)] = &[
    ("types",            "List all types in the file",                          Scope::All),
    ("type <name>",      "Inspect a type: generators, diagrams, maps",          Scope::All),
    ("homology <name>",  "Compute cellular homology of a type",                 Scope::All),
    ("start <t> <s>",    "Start a rewrite session (target optional)",           Scope::All),
    ("resume <t> <p>",   "Resume a session from a diagram (target optional)",    Scope::All),
    ("holes",            "List open holes of maps in this module",              Scope::All),
    ("fill <n>",         "Start a hole-filling session for hole <n>",           Scope::All),
    ("backward [on|off]", "Show or toggle backward rewrite mode (default: off)", Scope::All),
    ("status / show",    "Session state, or module info when idle",             Scope::All),
    ("print",            "Print the running source",                            Scope::Cli),
    ("save <path>",      "Write the running source to disk",                    Scope::Cli),
    ("stop",             "End the active session",                              Scope::All),
    ("clear",            "Clear the REPL output",                               Scope::Web),
    ("help / ?",         "Show this help",                                      Scope::All),
    ("quit / exit / q",  "Exit",                                                Scope::Cli),
];

/// `(token, description, where)` — commands needing an active session.
const SESSION: &[(&str, &str, Scope)] = &[
    ("apply <n> [<n2>..]", "Apply rewrite(s) at given indices (alias: a)",      Scope::All),
    ("auto <n>",         "Apply up to <n> rewrites automatically",              Scope::All),
    ("random <n>",       "Apply randomly selected rewrites",                    Scope::All),
    ("parallel [on|off]", "Show or toggle parallel rewrite mode (default: on)", Scope::All),
    ("undo [<n>]",       "Undo the last step, or back to step <n> (alias: u)",  Scope::All),
    ("redo [<n>]",       "Redo the last undone step, or forward to step <n>",   Scope::All),
    ("undo all / restart", "Reset to the initial diagram",                      Scope::All),
    ("rules",            "List rewrite rules at current dimension (alias: r)",  Scope::All),
    ("history",          "Show the move history (alias: h)",                    Scope::All),
    ("proof",            "Show the running proof diagram (alias: p)",           Scope::All),
    ("store <name>",     "Store the current proof as a named diagram",          Scope::All),
    ("done",             "Finalise the hole-filling session",                  Scope::All),
];

/// The command help, shared by both front-ends.  `web` selects the web-only
/// commands and drops the CLI-only ones, so each lists exactly what it supports.
/// The command token is coloured like an alifib expression; descriptions plain.
pub fn help(web: bool) -> RichText {
    let mut t = RichText::new();
    let shown = |scope: Scope| match scope {
        Scope::All => true,
        Scope::Cli => !web,
        Scope::Web => web,
    };

    t.line().plain("Always available:");
    for &(tok, desc, on) in ALWAYS {
        if shown(on) { help_line(&mut t, tok, desc); }
    }
    t.line();
    t.line().plain("Session commands (require active session):");
    for &(tok, desc, on) in SESSION {
        if shown(on) { help_line(&mut t, tok, desc); }
    }
    if web {
        t.line();
        t.line().plain("Keyboard: ↑/↓ navigate history · Ctrl+Enter evaluate file");
    }
    t
}

/// One help row: `  <token padded to 20> <description>`, the token in the value
/// colour, measured on its plain length for alignment.
fn help_line(t: &mut RichText, tok: &str, desc: &str) {
    let pad = " ".repeat(20usize.saturating_sub(tok.len()));
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
