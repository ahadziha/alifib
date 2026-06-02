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
    DiagramInfo, HoleInfo, HomologyInfo, Request, ResponseData, RuleInfo, TypeDetailInfo,
    TypeSummaryInfo, ZeroCellInfo,
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a new (empty) line; subsequent pushes append to it.  An empty line
    /// is a blank transcript row.
    pub fn line(&mut self) -> &mut Self {
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

    pub fn plain(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Plain, t) }
    pub fn label(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Label, t) }
    pub fn value(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Value, t) }
    pub fn src(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Src, t) }
    pub fn tgt(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Tgt, t) }
    pub fn section(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Section, t) }
    pub fn ok(&mut self, t: impl AsRef<str>) -> &mut Self { self.push(Role::Ok, t) }
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
        | Cell { .. } | Shutdown => return None,
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
        RenderKind::Holes => holes(&data.holes),
        RenderKind::Types => types(&data.types),
        RenderKind::TypeDetail => data.type_detail.as_ref().map(type_detail).unwrap_or_default(),
        RenderKind::Homology => data.homology.as_ref().map(homology).unwrap_or_default(),
    }
}

// ── Producers ────────────────────────────────────────────────────────────────

/// An active rewrite state: step count, current/target diagrams, and the
/// available rewrites (each `in → out` with its bracketed match), or none.
pub fn state(data: &ResponseData) -> RichText {
    let mut t = RichText::new();
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
pub fn zero_cell(zc: &ZeroCellInfo) -> RichText {
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
pub fn auto(data: &ResponseData) -> RichText {
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
pub fn store(data: &ResponseData) -> RichText {
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

/// The module's open holes, numbered for `fill <n>`.
pub fn holes(holes: &[HoleInfo]) -> RichText {
    let mut t = RichText::new();
    if holes.is_empty() {
        t.line().label("(no open holes)");
        return t;
    }
    t.line().section("open holes:");
    for h in holes {
        t.line().plain("  [").value(h.index.to_string()).plain("] ")
            .label(format!("@{} {} :: {}", h.type_name, h.map_name, h.domain_name));
        t.line().plain("      ").value(&h.boundary);
    }
    t
}

/// The type summaries for `types`: one line each, `name (dim …, N generators, …)`.
pub fn types(types: &[TypeSummaryInfo]) -> RichText {
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
pub fn type_detail(d: &TypeDetailInfo) -> RichText {
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

/// The rewrite rules at the current dimension for `rules`.
pub fn rules(rules: &[RuleInfo]) -> RichText {
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
pub fn proof(data: &ResponseData) -> RichText {
    let mut t = RichText::new();
    let Some(expr) = &data.proof_expr else {
        t.line().label("(no proof yet)");
        return t;
    };
    match &data.proof {
        Some(p) => { t.line().label("proof :").plain(" ").src(&p.input_label).plain(" → ").tgt(&p.output_label); }
        None => { t.line().label("proof:"); }
    }
    for line in expr.lines() {
        t.line().plain("  ").value(line);
    }
    t
}

/// The move history for `history`.
pub fn history(data: &ResponseData) -> RichText {
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
pub fn homology(h: &HomologyInfo) -> RichText {
    let mut t = RichText::new();
    if h.groups.is_empty() {
        t.line().label("(no generators)");
        return t;
    }
    for g in &h.groups {
        t.line().plain("  ").label(format!("H_{}", g.dim)).plain(" = ").value(&g.display);
    }
    t.line().plain("  ").label("χ").plain(" = ").value(h.euler_characteristic.to_string());
    t
}

fn plural(n: usize) -> &'static str { if n == 1 { "" } else { "s" } }

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
