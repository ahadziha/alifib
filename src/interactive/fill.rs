//! Interactive hole-filling.
//!
//! A hole on an *m*-cell `x` of a map `F : D → T` is a request to build `F(x)`:
//! an *m*-diagram in `T` from `F(x.in)` to `F(x.out)`.  For `m ≥ 1` that is a
//! rewrite, driven by the existing [`RewriteEngine`]; for a 0-cell it is the
//! choice of one of `T`'s 0-cells.  Finalising (`done`) appends `x => <proof>` to
//! `F`'s source definition and re-evaluates the file — the new clause, sitting
//! after the original `x => ?`, commits `x` to the proof (by the idempotence of
//! `[x => ?, x => a] ≡ [x => a]`) with the hole gone.
//!
//! This module is front-end-agnostic: the CLI and web REPLs both drive it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::aux::{GlobalId, HoleId, Tag};
use crate::core::complex::{Complex, MapDomain};
use crate::core::diagram::{CellData, Diagram, Sign};
use crate::core::map_hole::MapHole;
use crate::core::paste_tree::realise_tree;
use crate::interpreter::GlobalStore;
use crate::language::ast::{self, Block, LocalInst, PMapEntry, PartialMapDef, Spanned};
use crate::output::normalize::{domain_complex, render_hole_boundary, render_hole_constraints};

use super::engine::{reevaluate, RewriteEngine};
use super::protocol::ConstraintInfo;

/// One entry of the global hole list: an *actual* hole (`image: None`) of a map
/// in a type of the current module.
#[derive(Debug, Clone)]
pub struct HoleRef {
    /// 0-based position in the list (displayed 1-based).
    pub index: usize,
    pub type_gid: GlobalId,
    pub type_name: String,
    pub map_name: String,
    /// The map's domain, e.g. `D` in `@T H :: D`.
    pub domain_name: String,
    pub source: Tag,
    pub source_name: String,
    pub meta: HoleId,
    pub dim: usize,
    /// Pre-rendered boundary, `?name : in → out` (or `?name` for a 0-cell).
    pub boundary: String,
}

/// What a `done` needs to know to extend the right map.
#[derive(Debug, Clone)]
pub struct FillContext {
    pub type_gid: GlobalId,
    pub type_name: String,
    pub map_name: String,
    pub domain_name: String,
    pub source_name: String,
    /// Dimension of the hole (0 → a 0-cell choice; ≥1 → a rewrite session).
    pub dim: usize,
    /// Pre-rendered hole boundary, `?name : in → out` (or `?name` for a 0-cell).
    pub boundary: String,
}

/// A boundaryless 0-cell fill: choosing one of `T`'s 0-cells.  Choosing is a
/// reversible step so the session feels like a rewrite — `undo` reopens the
/// candidates, `redo` re-picks — and `target_reached` holds once a cell is chosen.
#[derive(Debug, Clone)]
pub struct ZeroCellFill {
    /// Candidate 0-cells of `T`, `(tag, name)`.
    pub choices: Vec<(Tag, String)>,
    /// The current pick (the single applied step), if any.
    pub chosen: Option<Tag>,
    /// The last undone pick, available for `redo`.
    pub undone: Option<Tag>,
}

impl ZeroCellFill {
    pub fn new(choices: Vec<(Tag, String)>) -> Self {
        Self { choices, chosen: None, undone: None }
    }

    /// Pick candidate `k` (a step).
    pub fn choose(&mut self, k: usize) -> Result<(), String> {
        let (tag, _) = self.choices.get(k)
            .ok_or_else(|| format!("no 0-cell numbered {}", k))?;
        self.chosen = Some(tag.clone());
        self.undone = None;
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), String> {
        match self.chosen.take() {
            Some(c) => { self.undone = Some(c); Ok(()) }
            None => Err("nothing to undo".to_owned()),
        }
    }

    pub fn redo(&mut self) -> Result<(), String> {
        match self.undone.take() {
            Some(c) => { self.chosen = Some(c); Ok(()) }
            None => Err("nothing to redo".to_owned()),
        }
    }

    pub fn target_reached(&self) -> bool { self.chosen.is_some() }
    pub fn can_redo(&self) -> bool { self.undone.is_some() }

    /// Display name of the chosen 0-cell, if any.
    pub fn chosen_name(&self) -> Option<&str> {
        let tag = self.chosen.as_ref()?;
        self.choices.iter().find(|(t, _)| t == tag).map(|(_, n)| n.as_str())
    }

    /// The candidate index of the chosen 0-cell — the `choice` a history entry
    /// records, mirroring a rewrite step's match index.
    pub fn chosen_index(&self) -> Option<usize> {
        let tag = self.chosen.as_ref()?;
        self.choices.iter().position(|(t, _)| t == tag)
    }

    /// The filler diagram: the chosen 0-cell.
    pub fn filler(&self) -> Result<Diagram, String> {
        let tag = self.chosen.as_ref()
            .ok_or_else(|| "choose a 0-cell first".to_owned())?;
        Diagram::cell(tag.clone(), &CellData::Zero).map_err(|e| format!("{}", e))
    }
}

/// An in-progress fill: a rewrite session (m ≥ 1) or a 0-cell choice.
pub enum FillSession {
    Rewrite(RewriteEngine),
    ZeroCell(ZeroCellFill),
}

impl FillSession {
    /// The proof diagram to fill with, if the session is complete.
    pub fn filler(&self) -> Result<Diagram, String> {
        match self {
            FillSession::Rewrite(engine) => {
                if !engine.target_reached() {
                    return Err("target not reached yet".to_owned());
                }
                engine.assemble_proof()
            }
            FillSession::ZeroCell(zc) => zc.filler(),
        }
    }
}

/// One map of the current module, as seen by the interactive listings: its
/// type, the complexes it maps between, and its pending holes — everything the
/// hole/constraint walks need, resolved once.
struct MapView<'a> {
    type_name: &'a str,
    type_gid: GlobalId,
    /// The map's target complex (where image leaves resolve).
    target: &'a Complex,
    map_name: &'a str,
    /// The map's source complex (where holes are named after their generators).
    domain: &'a Complex,
    domain_name: String,
    holes: &'a [MapHole],
}

/// Visit every map of every type in `root_module`, in the canonical
/// `(type, map)` order both listings number by, resolving each map's domain.
/// The single source of that traversal — [`list_open_holes`] and
/// [`list_constraints`] differ only in what they read off each [`MapView`].
fn visit_maps(store: &GlobalStore, root_module: &str, mut visit: impl FnMut(MapView)) {
    let Some(module) = store.find_module(root_module) else { return };

    let mut types: Vec<(String, GlobalId)> = module
        .generators_iter()
        .filter_map(|(name, tag, _)| match tag {
            Tag::Global(gid) => Some((name.clone(), *gid)),
            _ => None,
        })
        .collect();
    types.sort();

    for (type_name, gid) in types {
        let Some(entry) = store.find_type(gid) else { continue };
        let target = entry.complex.as_ref();

        let mut maps: Vec<(String, MapDomain)> =
            target.maps_iter().map(|(n, _, d)| (n.clone(), d.clone())).collect();
        maps.sort_by(|a, b| a.0.cmp(&b.0));
        for (map_name, mdom) in &maps {
            let Some(holes) = target.map_holes(map_name) else { continue };
            let domain = domain_complex(store, mdom);
            let domain_name = match mdom {
                MapDomain::Type(dgid) => module
                    .find_generator_by_tag(&Tag::Global(*dgid))
                    .cloned()
                    .unwrap_or_default(),
                MapDomain::Module(mid) => mid.clone(),
            };
            visit(MapView {
                type_name: &type_name,
                type_gid: gid,
                target,
                map_name,
                domain: domain.as_deref().unwrap_or(target),
                domain_name,
                holes,
            });
        }
    }
}

/// The holes of `m`, lowest dimension first then by source-generator name — the
/// order a hole's faces print before the holes that reference them.
fn holes_in_order<'a>(m: &MapView<'a>, want_image: bool) -> Vec<&'a MapHole> {
    let mut hs: Vec<&MapHole> = m.holes.iter().filter(|h| h.image.is_some() == want_image).collect();
    hs.sort_by_key(|h| (h.dim, m.domain.find_generator_by_tag(&h.source).cloned().unwrap_or_default()));
    hs
}

/// The actual holes (`image: None`) of maps in types of the current module,
/// numbered in a deterministic order (type, map, dim, source-generator name).
pub fn list_open_holes(store: &GlobalStore, root_module: &str) -> Vec<HoleRef> {
    let mut out = Vec::new();
    visit_maps(store, root_module, |m| {
        for h in holes_in_order(&m, false) {
            let source_name = m.domain
                .find_generator_by_tag(&h.source)
                .cloned()
                .unwrap_or_else(|| format!("{}", h.source));
            out.push(HoleRef {
                index: out.len(),
                type_gid: m.type_gid,
                type_name: m.type_name.to_owned(),
                map_name: m.map_name.to_owned(),
                domain_name: m.domain_name.clone(),
                source: h.source.clone(),
                source_name,
                meta: h.meta,
                dim: h.dim,
                boundary: render_hole_boundary(h, m.holes, m.target, m.domain),
            });
        }
    });
    out
}

/// The constraints imposed by conditional pending assignments (`x => a`) of maps
/// in the current module: for each such assignment whose image boundary still has
/// holes, the equations `F(x.side) = a.side`.  Ordered as [`list_open_holes`].
pub fn list_constraints(store: &GlobalStore, root_module: &str) -> Vec<ConstraintInfo> {
    let mut out = Vec::new();
    visit_maps(store, root_module, |m| {
        for h in holes_in_order(&m, true) {
            let equations = render_hole_constraints(h, m.holes, m.target, m.domain);
            if equations.is_empty() {
                continue;
            }
            out.push(ConstraintInfo {
                type_name: m.type_name.to_owned(),
                map_name: m.map_name.to_owned(),
                domain_name: m.domain_name.clone(),
                equations,
            });
        }
    });
    out
}

/// Start a fill for the (0-based) hole number `index`, matching the rewrite-match
/// numbering.  Errors if out of range, or if the hole still has unfilled
/// dependencies (reporting their numbers).
pub fn start_fill(
    store: &Arc<GlobalStore>,
    root_module: &str,
    source_file: &str,
    index: usize,
    backward: bool,
) -> Result<(FillContext, FillSession), String> {
    let list = list_open_holes(store, root_module);
    let href = list.get(index).cloned()
        .ok_or_else(|| format!("no hole numbered {}", index))?;

    let blockers = blocking_holes(store, &list, &href);
    if !blockers.is_empty() {
        let nums: Vec<String> = blockers.iter().map(|i| i.to_string()).collect();
        return Err(format!(
            "Must fill hole{} {} first",
            if blockers.len() > 1 { "s" } else { "" },
            nums.join(", ")
        ));
    }

    let tc = store.find_type(href.type_gid)
        .ok_or_else(|| format!("type `{}` not found", href.type_name))?
        .complex.clone();

    let ctx = FillContext {
        type_gid: href.type_gid,
        type_name: href.type_name.clone(),
        map_name: href.map_name.clone(),
        domain_name: href.domain_name.clone(),
        source_name: href.source_name.clone(),
        dim: href.dim,
        boundary: href.boundary.clone(),
    };

    let session = if href.dim == 0 {
        let choices: Vec<(Tag, String)> = tc
            .generators_iter()
            .filter(|(_, _, d)| *d == 0)
            .map(|(name, tag, _)| (tag.clone(), name.clone()))
            .collect();
        FillSession::ZeroCell(ZeroCellFill::new(choices))
    } else {
        let (ind, outd) = realise_boundaries(&tc, &href)?;
        let (initial, target) = if backward { (outd, ind) } else { (ind, outd) };
        let engine = RewriteEngine::from_diagrams(
            Arc::clone(store),
            Arc::clone(&tc),
            initial,
            Some(target),
            source_file.to_owned(),
            href.type_name.clone(),
            format!("?{}.in", href.source_name),
            Some(format!("?{}.out", href.source_name)),
            backward,
        )?;
        FillSession::Rewrite(engine)
    };
    Ok((ctx, session))
}

/// Realise a hole's boundary paste trees (now hole-free) to concrete diagrams.
fn realise_boundaries(tc: &Complex, href: &HoleRef) -> Result<(Diagram, Diagram), String> {
    let holes = tc.map_holes(&href.map_name)
        .ok_or_else(|| format!("map `{}` not found", href.map_name))?;
    let hole = holes.iter().find(|h| h.source == href.source)
        .ok_or_else(|| "hole no longer present".to_owned())?;
    let (in_tree, out_tree) = hole.boundary.as_ref()
        .ok_or_else(|| "a 0-cell hole has no boundary".to_owned())?;
    let ind = realise_tree(in_tree, tc).map_err(|e| format!("input boundary: {}", e))?;
    let outd = realise_tree(out_tree, tc).map_err(|e| format!("output boundary: {}", e))?;
    Ok((ind, outd))
}

/// The actual holes (by list index) that must be filled before `href`: the
/// `image: None` entries reachable from its dependency metavariables (recursing
/// through conditional pending assignments).
fn blocking_holes(store: &GlobalStore, list: &[HoleRef], href: &HoleRef) -> Vec<usize> {
    let Some(tc) = store.find_type(href.type_gid).map(|e| Arc::clone(&e.complex)) else { return vec![]; };
    let Some(holes) = tc.map_holes(&href.map_name) else { return vec![]; };
    let Some(hole) = holes.iter().find(|h| h.source == href.source) else { return vec![]; };

    let index_by_meta: HashMap<HoleId, usize> = list.iter()
        .filter(|r| r.type_gid == href.type_gid && r.map_name == href.map_name)
        .map(|r| (r.meta, r.index))
        .collect();

    let mut result = BTreeSet::new();
    let mut seen = HashSet::new();
    let mut stack: Vec<HoleId> = hole.deps().into_iter().collect();
    while let Some(meta) = stack.pop() {
        if !seen.insert(meta) {
            continue;
        }
        match holes.iter().find(|h| h.meta == meta) {
            Some(h) if h.image.is_none() => {
                if let Some(&i) = index_by_meta.get(&meta) {
                    result.insert(i);
                }
            }
            Some(h) => stack.extend(h.deps()), // conditional: blocked transitively
            None => {}
        }
    }
    result.into_iter().collect()
}

/// The shared "Filled ?x with <filler> : in → out" report — the boundary is
/// read off the *filler* (empty for a 0-cell, and correct even when the filler
/// is a lower-dimensional/degenerate diagram).  Used by both REPLs.
pub fn filled_report(store: &GlobalStore, ctx: &FillContext, filler: &Diagram) -> String {
    match store.find_type(ctx.type_gid) {
        Some(t) => filled_message(ctx, filler, &t.complex),
        None => format!("Filled ?{}", ctx.source_name),
    }
}

fn filled_message(ctx: &FillContext, filler: &Diagram, scope: &Complex) -> String {
    let expr = crate::output::render_diagram(filler, scope);
    let boundary = filler.top_dim().checked_sub(1).and_then(|k| {
        let inp = Diagram::boundary(Sign::Input, k, filler).ok()?;
        let out = Diagram::boundary(Sign::Output, k, filler).ok()?;
        Some(format!(" : {} → {}",
            crate::output::render_diagram(&inp, scope),
            crate::output::render_diagram(&out, scope)))
    }).unwrap_or_default();
    format!("Filled ?{} with {}{}", ctx.source_name, expr, boundary)
}

/// Finalise a fill: append `x => <filler>` to `F`'s definition in `source` and
/// re-evaluate.  Returns the new store and the edited source on success; on an
/// inconsistent fill the interpreter errors and nothing is changed.
pub fn finalize(
    store: &Arc<GlobalStore>,
    ctx: &FillContext,
    filler: &Diagram,
    canonical_path: &str,
    source: &str,
) -> Result<(Arc<GlobalStore>, String), String> {
    let new_source = edit_for_fill(store, ctx, filler, source)?;
    let new_store = reevaluate(canonical_path, &new_source)?;
    Ok((new_store, new_source))
}

/// Splice the proof of `ctx`'s hole into `F`'s definition, returning the edited
/// source *without* re-evaluating.  The CLI re-evaluates from disk; the web with
/// its own virtual loader, so they share only the edit.
pub fn edit_for_fill(
    store: &GlobalStore,
    ctx: &FillContext,
    filler: &Diagram,
    source: &str,
) -> Result<String, String> {
    let tc = store.find_type(ctx.type_gid)
        .ok_or_else(|| format!("type `{}` not found", ctx.type_name))?
        .complex.clone();
    let expr = crate::output::render_diagram(filler, &tc);
    edit_map_definition(source, &ctx.type_name, &ctx.map_name, &ctx.source_name, &expr)
}

/// Splice the proof `expr` of generator `source_name` into map `map_name`'s
/// definition.  If the hole was written explicitly as `source_name => ?`, the
/// proof *replaces* that `?` (so an explicit fill reads back as one clause);
/// otherwise — an implicit hole, with no `?` of its own to pin onto — it is
/// appended, committing the cell by the idempotence of `[x => ?, x => a]`.
/// Errors only when appending into a `for`-block (no single location).
fn edit_map_definition(
    source: &str,
    type_name: &str,
    map_name: &str,
    source_name: &str,
    expr: &str,
) -> Result<String, String> {
    let program = crate::language::parse(source)
        .map_err(|errs| format!("re-parse failed: {} error(s)", errs.len()))?;
    let value = find_map_def(&program, type_name, map_name)
        .ok_or_else(|| format!("could not locate map `{}` in type `{}`", map_name, type_name))?;

    match &value.inner {
        PartialMapDef::Ext(ext) => {
            // Pin onto an explicit `source_name => ?` clause if there is one.
            for c in &ext.clauses {
                if let PMapEntry::Clause(cl) = &c.inner {
                    if lhs_generator_name(&cl.lhs.inner).as_deref() == Some(source_name)
                        && rhs_is_hole(&cl.rhs.inner)
                    {
                        let span = cl.rhs.span;
                        return Ok(format!("{}{}{}", &source[..span.start], expr, &source[span.end..]));
                    }
                }
            }
            // No `?` to pin onto: append a fresh clause.
            if ext.clauses.iter().any(|c| matches!(c.inner, PMapEntry::For(_))) {
                return Err(format!("cannot edit map `{}`: its definition uses a for-block", map_name));
            }
            let close = source[..value.span.end].rfind(']')
                .ok_or_else(|| "could not find the closing `]` of the clause list".to_owned())?;
            // Trim trailing space off the prefix so the comma sits flush against
            // the last clause; a newline then sets the new clause on its own line,
            // indented to match the line the last clause sits on.
            let prefix = source[..close].trim_end();
            let suffix = &source[close..];
            let clause = format!("{} => {}", source_name, expr);
            if ext.clauses.is_empty() {
                Ok(format!("{} {} {}", prefix, clause, suffix))
            } else {
                let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
                let indent: String = prefix[line_start..]
                    .chars().take_while(|c| c.is_whitespace()).collect();
                Ok(format!("{},\n{}{} {}", prefix, indent, clause, suffix))
            }
        }
        PartialMapDef::PartialMap(_) => {
            let end = value.span.end;
            Ok(format!("{} [ {} => {} ]{}", &source[..end], source_name, expr, &source[end..]))
        }
    }
}

/// The (possibly dotted) generator name a clause's left-hand side denotes, if it
/// is exactly one such name — `r`, `Sub.arr` — which is the canonical form a
/// `done` assignment writes, and so the only shape a fill can be pinned back onto.
fn lhs_generator_name(d: &ast::Diagram) -> Option<String> {
    match d {
        ast::Diagram::PrincipalPaste(comps) => match comps.as_slice() {
            [c] => c.inner.dotted_name(),
            _ => None,
        },
        _ => None,
    }
}

/// Whether a clause's right-hand side is a bare hole `?`.
fn rhs_is_hole(d: &ast::Diagram) -> bool {
    let ast::Diagram::PrincipalPaste(comps) = d else { return false };
    matches!(comps.as_slice(), [c] if matches!(c.inner, ast::DExpr::Component(ast::DComponent::Hole)))
}

/// Locate the `value` of map `map_name` defined in the `@type_name` block.
fn find_map_def<'a>(program: &'a ast::Program, type_name: &str, map_name: &str) -> Option<&'a Spanned<PartialMapDef>> {
    for block in &program.blocks {
        let Block::LocalBlock { complex, body } = &block.inner else { continue; };
        if !complex_matches(complex, type_name) {
            continue;
        }
        for inst in body {
            if let LocalInst::DefPartialMap(dp) = &inst.inner {
                if dp.name.inner == map_name {
                    return Some(&dp.value);
                }
            }
        }
    }
    None
}

/// Whether a block's `@…` address names `type_name`.
fn complex_matches(c: &Spanned<ast::Complex>, type_name: &str) -> bool {
    let addr = match &c.inner {
        ast::Complex::Address(addr) => Some(addr),
        ast::Complex::Block { address, .. } => address.as_ref(),
    };
    addr.map(|a| {
        let joined = a.iter().map(|s| s.inner.as_str()).collect::<Vec<_>>().join(".");
        joined == type_name || a.last().map(|s| s.inner == type_name).unwrap_or(false)
    }).unwrap_or(false)
}

#[cfg(test)]
mod edit_tests {
    use super::edit_map_definition;

    /// An explicit hole keyed by a *dotted* generator name (the canonical form
    /// `done` writes) is pinned: the proof replaces its `?` in place.
    #[test]
    fn pins_a_dotted_explicit_hole_in_place() {
        let src = "@Type\n\nX <<= { a, z },\n\n@X\n  let H :: D = [ Sub.arr => ?, a => z ]\n";
        let out = edit_map_definition(src, "X", "H", "Sub.arr", "(a #0 a)").unwrap();
        assert!(out.contains("Sub.arr => (a #0 a)"), "{out}");
        assert!(!out.contains("=> ?"), "the hole is filled in place: {out}");
    }

    /// An implicit hole — no `?` of its own to pin onto — is appended, leaving
    /// the explicit `?` it sprang from untouched.
    #[test]
    fn appends_when_no_matching_explicit_hole() {
        let src = "@Type\n\nX <<= { a, z },\n\n@X\n  let H :: D = [ Sub => ? ]\n";
        let out = edit_map_definition(src, "X", "H", "a", "z").unwrap();
        assert!(out.contains("Sub => ?"), "the explicit hole stays: {out}");
        assert!(out.contains("a => z"), "the implicit fill is appended: {out}");
    }
}

