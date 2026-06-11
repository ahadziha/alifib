use super::diagram::interpret_diagram_as_term;
use super::resolve::{interpret_address, resolve_map_domain_complex, resolve_module_domain, resolve_type_complex};
use super::types::{
    Context, EvalMap, InterpResult, Step, Term,
    fail, get_cell_data, make_error, make_error_from_core, sorted_generators,
};
use crate::aux::{self, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, Sign as DiagramSign},
    map_hole::MapHole,
    partial_map::PartialMap,
    paste_tree::PasteTree,
};
use crate::language::ast::{self, DefPartialMap, ForBlock, PMapEntry, PartialMapBasic, PartialMapClause, PartialMapDef, PartialMapExt, Span, Spanned};
use std::collections::HashMap;
use std::sync::Arc;

// ---- Maps with holes ----

/// A map under construction: the committed (hole-free) [`PartialMap`] plus the
/// pending assignments — pure holes (`x => ?`) and conditional assignments
/// (`x => a` whose boundary faces are not all mapped yet).
struct MapBuild {
    map: PartialMap,
    holes: Vec<MapHole>,
    /// Span of the clause currently being interpreted.  Stamped onto every
    /// pending entry created while processing it, so that a later cascade
    /// failure can be blamed on the assignment truly responsible rather than on
    /// whichever innocent filler happened to close its last boundary face.
    current_span: Span,
    /// Source span of each pending entry, keyed by its source generator.  Pending
    /// entries carried in from a prefix map have no span here (they were written
    /// elsewhere), and so fall back to the current clause's span.
    hole_spans: HashMap<Tag, Span>,
    /// Set when committing a *pending* assignment fails during cascading: the
    /// span of that pending assignment, used to re-aim the resulting error.
    blame_span: Option<Span>,
}

impl MapBuild {
    /// Position of the pending entry for `tag`, if any.
    fn entry_index(&self, tag: &Tag) -> Option<usize> {
        self.holes.iter().position(|h| &h.source == tag)
    }
}

/// Assign `x_diag => image` (image `None` for the bare hole `x => ?`).
///
/// Sound boundary inference (case 1) still fires when the image is known and a
/// whole boundary is a single cell.  Any boundary face left undefined becomes a
/// hole.  When the image is known and every face is committed, the assignment is
/// committed to the real map; otherwise a pending entry — pure hole (`image`
/// `None`) or conditional (`image` `Some`) — is recorded for `x`.
fn assign_cell(
    build: &mut MapBuild,
    context: &Context,
    domain: &Complex,
    x_diag: &Diagram,
    image: Option<&Diagram>,
) -> Result<(), aux::Error> {
    if !x_diag.is_cell() {
        return Err(aux::Error::new("Left-hand side of map instruction must be a cell"));
    }
    let tag = x_diag
        .top_label()
        .ok_or_else(|| aux::Error::new("Domain cell has no top label"))?
        .clone();
    let dim = x_diag.top_dim();

    // Already committed: a re-assignment must be consistent.
    if build.map.is_defined_at(&tag) {
        if let Some(a) = image {
            let current = build.map.image(&tag)?;
            if !Diagram::isomorphic(current, a) {
                return Err(aux::Error::new("The same generator is mapped to multiple diagrams"));
            }
        }
        return Ok(());
    }

    // Already a pending hole and no image to add: idempotent (`x => ?` twice, or
    // a face re-reached during transport).  Filling it requires an actual image.
    if image.is_none() && build.entry_index(&tag).is_some() {
        return Ok(());
    }

    let cell_data = get_cell_data(context, domain, &tag)
        .ok_or_else(|| aux::Error::new("Cannot find cell data for generator"))?;

    // Case 1 (sound): when the image is known and a whole boundary is a single
    // cell, that cell's image is forced.
    if let Some(a) = image {
        for sign in [DiagramSign::Input, DiagramSign::Output] {
            let Some(source_boundary) = boundary_of_sign(&cell_data, sign) else { continue; };
            if !source_boundary.is_cell() {
                continue;
            }
            let Some(face_tag) = source_boundary.top_label() else { continue; };
            if build.map.is_defined_at(face_tag) {
                continue;
            }
            let target_boundary = Diagram::boundary(sign, dim - 1, a)?;
            assign_cell(build, context, domain, &source_boundary, Some(&target_boundary))?;
        }
    }

    // Force the map to be defined (committed or holed) on every still-undefined
    // face.  Collapse inference inside these calls may resolve some faces to
    // concrete images, which can in turn collapse `x` itself — hence we test for
    // collapse only *after* the faces are settled.
    for (face_tag, _) in boundary_dependencies(&cell_data, &build.map) {
        ensure_defined(build, context, domain, &face_tag)?;
    }

    let fully_defined = boundary_dependencies(&cell_data, &build.map).is_empty();
    if let Some(a) = image {
        // Fully determined with a known image: commit directly (+ reconcile/cascade).
        if fully_defined {
            return commit(build, context, domain, tag, dim, cell_data, a.clone());
        }
    } else if let Some(d) = collapsed_boundary_image(&build.map, &cell_data, dim) {
        // A boundary has collapsed below dimension n-1, so the only possible image
        // of the n-cell `x` is that diagram — infer it instead of making a hole.
        // Any incompatibility with the other boundary is caught by the commit.
        return assign_cell(build, context, domain, x_diag, Some(&d));
    }

    // Otherwise record a pending entry: a conditional (`image` Some) or pure hole.
    let boundary = transport_cell_boundaries(build, domain, &cell_data)?;
    upsert_entry(build, tag, dim, image.cloned(), boundary);
    Ok(())
}

/// Ensure the map is committed, or has a pending entry, on `tag`.  When the
/// logic forces the map to be defined on a cell (a boundary face), this is how
/// we record it: by assigning it a hole — which [`assign_cell`] may instead
/// resolve to a concrete image by collapse inference, if a boundary forces it.
/// A cell genuinely never required stays untouched (and so undefined).
fn ensure_defined(
    build: &mut MapBuild,
    context: &Context,
    domain: &Complex,
    tag: &Tag,
) -> Result<(), aux::Error> {
    if build.map.is_defined_at(tag) || build.entry_index(tag).is_some() {
        return Ok(());
    }
    let cell_data = get_cell_data(context, domain, tag).ok_or_else(|| {
        aux::Error::new(format!("Cannot find cell data for boundary cell {}", tag))
    })?;
    let cell = Diagram::cell(tag.clone(), &cell_data)?;
    assign_cell(build, context, domain, &cell, None)
}

/// Insert or upgrade the pending entry for `tag`.  An existing entry keeps its
/// metavariable (so dependents stay valid) and gains the image if one is given.
fn upsert_entry(
    build: &mut MapBuild,
    tag: Tag,
    dim: usize,
    image: Option<Diagram>,
    boundary: Option<(PasteTree, PasteTree)>,
) {
    if let Some(i) = build.entry_index(&tag) {
        let h = &mut build.holes[i];
        if image.is_some() {
            h.image = image;
        }
        h.boundary = boundary;
    } else {
        build.hole_spans.insert(tag.clone(), build.current_span);
        build.holes.push(MapHole {
            source: tag,
            dim,
            image,
            boundary,
        });
    }
}

/// Pointwise reading of `<map> => ?`: hole every constituent cell of the image
/// of each generator in `f_map`'s domain.  A generator's image may be a single
/// cell or a composite diagram; either way each cell it is built from becomes a
/// hole (`ensure_hole` no-ops on cells already mapped or pending).
fn hole_map_image(
    build: &mut MapBuild,
    context: &Context,
    domain: &Complex,
    f_map: &EvalMap,
) -> Result<(), aux::Error> {
    let map_domain = &*f_map.domain;
    for (_, _, tag) in sorted_generators(map_domain) {
        if !f_map.map.is_defined_at(&tag) {
            continue;
        }
        let image = f_map.map.image(&tag)?;
        for label in image.all_labels() {
            ensure_defined(build, context, domain, label)?;
        }
    }
    Ok(())
}

/// Commit `tag => actual_image` to the real map, then reconcile holes and commit
/// any conditional whose dependencies are now closed (cascading).
fn commit(
    build: &mut MapBuild,
    context: &Context,
    domain: &Complex,
    tag: Tag,
    dim: usize,
    cell_data: CellData,
    actual_image: Diagram,
) -> Result<(), aux::Error> {
    commit_one(build, tag, dim, cell_data, actual_image)?;
    cascade(build, context, domain)
}

/// Add one entry to the real map; close the matching pending entry and splice
/// its image into the boundary trees of the remaining ones.
fn commit_one(
    build: &mut MapBuild,
    tag: Tag,
    dim: usize,
    cell_data: CellData,
    actual_image: Diagram,
) -> Result<(), aux::Error> {
    debug_assert!(
        !actual_image.all_labels().any(|t| matches!(t, Tag::Hole(_))),
        "a committed image must never contain a metavariable",
    );
    let map = std::mem::replace(&mut build.map, PartialMap::empty());
    build.map = PartialMap::extend(map, tag.clone(), dim, cell_data, actual_image.clone())?;

    let Some(i) = build.entry_index(&tag) else { return Ok(()); };
    build.holes.remove(i);
    build.hole_spans.remove(&tag);
    let n = actual_image.top_dim();
    let img_tree = actual_image
        .tree(DiagramSign::Input, n)
        .ok_or_else(|| aux::Error::new("map image has no paste tree"))?
        .clone();
    let subst = |leaf: &Tag| -> Option<PasteTree> {
        match leaf {
            Tag::Hole(source) if **source == tag => Some(img_tree.clone()),
            _ => None,
        }
    };
    for h in &mut build.holes {
        if let Some((input, output)) = &h.boundary {
            h.boundary = Some((input.substitute(&subst), output.substitute(&subst)));
        }
    }
    Ok(())
}

/// Commit every conditional whose dependencies are all closed, repeatedly, until
/// none remain.  Each commit removes one pending entry, so this terminates.
fn cascade(build: &mut MapBuild, context: &Context, domain: &Complex) -> Result<(), aux::Error> {
    loop {
        let ready = build
            .holes
            .iter()
            .find(|h| h.image.is_some() && h.deps().is_empty())
            .map(|h| (h.source.clone(), h.dim, h.image.clone().unwrap()));
        let Some((source, dim, image)) = ready else { return Ok(()); };
        let cell_data = get_cell_data(context, domain, &source)
            .ok_or_else(|| aux::Error::new("Cannot find cell data for conditional assignment"))?;
        if let Err(e) = commit_one(build, source.clone(), dim, cell_data, image) {
            return Err(blame_pending(build, domain, &source, e));
        }
    }
}

/// Rephrase a cascade-commit failure as a fault of the *pending* assignment that
/// failed to commit, and re-aim the error at where that assignment was written.
///
/// A pending assignment `x => a` imposes a constraint on the images of `x`'s
/// boundary faces; the faces are filled by later clauses.  When those fillings
/// violate the constraint, the boundary mismatch surfaces only now, on commit —
/// but the clause that closed the last face is itself well-formed.  We therefore
/// name `x` and point back at its clause rather than at the innocent filler.
fn blame_pending(
    build: &mut MapBuild,
    domain: &Complex,
    source: &Tag,
    e: aux::Error,
) -> aux::Error {
    if let Some(span) = build.hole_spans.get(source) {
        build.blame_span = Some(*span);
    }
    let name = domain
        .find_generator_by_tag(source)
        .cloned()
        .unwrap_or_else(|| format!("{}", source));
    aux::Error {
        message: format!(
            "The pending assignment of `{}` is incompatible with the images filled in for its boundary: {}",
            name, e.message
        ),
        notes: e.notes,
    }
}

/// Transport a cell's boundaries through the build, as paste trees over
/// committed images and metavariables.  `None` for a 0-cell.
fn transport_cell_boundaries(
    build: &MapBuild,
    domain: &Complex,
    cell_data: &CellData,
) -> Result<Option<(PasteTree, PasteTree)>, aux::Error> {
    let CellData::Boundary { boundary_in, boundary_out } = cell_data else {
        return Ok(None);
    };
    let in_tree = transport_boundary(build, domain, boundary_in)?;
    let out_tree = transport_boundary(build, domain, boundary_out)?;
    Ok(Some((in_tree, out_tree)))
}

/// Transport one boundary diagram through the build: rewrite every leaf to the
/// committed image's paste tree, or to a metavariable leaf for a pending face.
fn transport_boundary(
    build: &MapBuild,
    domain: &Complex,
    boundary: &Diagram,
) -> Result<PasteTree, aux::Error> {
    let n = boundary.top_dim();
    let tree = boundary
        .tree(DiagramSign::Input, n)
        .ok_or_else(|| aux::Error::new("boundary diagram has no paste tree"))?;
    tree.try_substitute(&|tag| transport_leaf(build, domain, tag))
}

/// The replacement for one boundary leaf: the committed image's paste tree, or a
/// `Tag::Hole` metavariable for a pending face.  Every face is map-or-holed by
/// `assign_cell`/`ensure_defined` before transport, so the final arm is an
/// invariant violation rather than a user error.
fn transport_leaf(
    build: &MapBuild,
    domain: &Complex,
    tag: &Tag,
) -> Result<Option<PasteTree>, aux::Error> {
    if build.map.is_defined_at(tag) {
        let image = build.map.image(tag)?;
        let img_tree = image
            .tree(DiagramSign::Input, image.top_dim())
            .ok_or_else(|| aux::Error::new("map image has no paste tree"))?
            .clone();
        Ok(Some(img_tree))
    } else if build.entry_index(tag).is_some() {
        Ok(Some(PasteTree::Leaf(Tag::Hole(Box::new(tag.clone())))))
    } else {
        let name = domain
            .find_generator_by_tag(tag)
            .cloned()
            .unwrap_or_else(|| format!("{}", tag));
        Err(aux::Error::new(format!(
            "internal: boundary references `{}`, which is neither mapped nor a hole",
            name
        )))
    }
}

/// Interpret an anonymous map component (inline map definition with an explicit target complex).
pub fn interpret_anon_map_component(
    context: &Context,
    scope: &Complex,
    target: &Spanned<ast::Complex>,
    def: &Spanned<PartialMapDef>,
) -> Step<EvalMap> {
    let (ns_opt, target_result) =
        super::eval::interpret_complex(context, super::types::Mode::Local, target);
    let Some(ns) = ns_opt else { return (None, target_result); };
    // For simple type references (no block body), use the canonical Arc<Complex>
    // from the type store so that pointer-based domain comparison works correctly
    // when matching named maps against anonymous maps of the same type.
    let canonical_domain = if matches!(&target.inner, ast::Complex::Address(_)) {
        target_result.context.state
            .find_type(ns.owner_type_id)
            .map(|entry| Arc::clone(&entry.complex))
    } else {
        None
    };
    let (mc_opt, def_result) =
        interpret_pmap_def(&target_result.context, scope, &ns.working_complex, def);
    let mc_opt = mc_opt.map(|eval_map| match canonical_domain {
        Some(arc) => EvalMap { domain: arc, ..eval_map },
        None => eval_map,
    });
    (mc_opt, target_result.merge(def_result))
}

// ---- PartialMap interpretation ----

/// Bundled context for evaluating partial map expressions.
///
/// Packages the three parameters that travel together through every internal
/// evaluation function: the interpreter state, the lexical scope for name
/// resolution, and the complex being mapped from.
#[derive(Clone, Copy)]
struct PartialMapCtx<'a> {
    context: &'a Context,
    /// Lexical environment: where diagram and map names are resolved.
    scope: &'a Complex,
    /// The complex being mapped from (the domain of definition).
    domain: &'a Complex,
}

/// Interpret a partial map expression, resolving it against the given scope and domain.
pub fn interpret_partial_map(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    partial_map: &Spanned<ast::PartialMap>,
) -> Step<EvalMap> {
    eval_partial_map(&PartialMapCtx { context, scope, domain }, &partial_map.inner, partial_map.span)
}

/// Evaluate a partial map AST node, dispatching basic vs. dot-access forms.
///
/// A dot-access `base.rest` evaluates `base`, then looks up `rest` in the base map's domain.
fn eval_partial_map(ctx: &PartialMapCtx<'_>, partial_map: &ast::PartialMap, span: Span) -> Step<EvalMap> {
    match partial_map {
        ast::PartialMap::Basic(basic) => eval_partial_map_basic(ctx, basic, span),
        ast::PartialMap::Dot { base, rest } => {
            let (base_opt, base_result) = eval_partial_map_basic(ctx, base, span);
            let Some(base_map) = base_opt else { return (None, base_result); };
            // Dot traversal: the new lookup scope is the base map's domain.
            let (rest_opt, rest_result) =
                interpret_partial_map(&base_result.context, &base_map.domain, ctx.domain, rest);
            let combined = base_result.merge(rest_result);
            let Some(rest_map) = rest_opt else { return (None, combined); };
            let composed = PartialMap::compose(&base_map.map, &rest_map.map);
            (Some(EvalMap { map: composed, domain: rest_map.domain, holes: vec![] }), combined)
        }
    }
}

/// Evaluate a basic partial map expression: name lookup, anonymous map, or parenthesized form.
fn eval_partial_map_basic(ctx: &PartialMapCtx<'_>, basic: &PartialMapBasic, span: Span) -> Step<EvalMap> {
    match basic {
        PartialMapBasic::Name(name) => {
            let Some((map, domain)) = ctx.scope.find_map(name) else {
                return fail(ctx.context, span, format!("Partial map not found: `{}`", name));
            };
            // Carry the map's stored holes so that `F [ … ]` extends a
            // map-with-holes (filling) rather than only its hole-free part.
            let holes = ctx.scope.map_holes(name).map(<[_]>::to_vec).unwrap_or_default();
            let (domain_opt, result) = resolve_map_domain_complex(ctx.context, domain, span);
            let Some(domain) = domain_opt else {
                return (None, result);
            };
            (Some(EvalMap { map: map.clone(), domain, holes }), InterpResult::ok(ctx.context.clone()))
        }
        PartialMapBasic::AnonMap { def, target } => {
            interpret_anon_map_component(ctx.context, ctx.scope, target, def)
        }
        PartialMapBasic::Paren(inner) => interpret_partial_map(ctx.context, ctx.scope, ctx.domain, inner),
    }
}

// ---- PartialMapDef / PartialMapExt interpretation ----

/// Interpret a partial map definition: either a direct map expression or an extension block.
pub fn interpret_pmap_def(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    partial_map_def: &Spanned<PartialMapDef>,
) -> Step<EvalMap> {
    let ctx = PartialMapCtx { context, scope, domain };
    match &partial_map_def.inner {
        PartialMapDef::PartialMap(partial_map) => eval_partial_map(&ctx, partial_map, partial_map_def.span),
        PartialMapDef::Ext(ext) => interpret_partial_map_ext(&ctx, ext),
    }
}

/// Produce the starting map for an extension block.
///
/// If a prefix map is given, evaluate it and reinterpret it as a partial map
/// from `ctx.domain`, validating that all its entries are in the domain.
/// Otherwise start from the empty map on the domain.
fn initial_eval_map(ctx: &PartialMapCtx<'_>, prefix: &Option<Box<Spanned<ast::PartialMap>>>) -> Step<EvalMap> {
    let domain = Arc::new(ctx.domain.clone());
    match prefix {
        None => (
            Some(EvalMap { map: PartialMap::empty(), domain, holes: vec![] }),
            InterpResult::ok(ctx.context.clone()),
        ),
        Some(prefix) => {
            let (eval_opt, result) = interpret_partial_map(ctx.context, ctx.scope, ctx.domain, prefix);
            let Some(eval) = eval_opt else { return (None, result); };
            let result = validate_map_as_source(
                &eval.map, &eval.domain, ctx.domain, prefix.span, result,
            );
            if result.has_errors() { return (None, result); }
            (Some(EvalMap { map: eval.map, domain, holes: eval.holes }), result)
        }
    }
}

/// Evaluate a sequence of partial map entries, extending the build after each one.
///
/// Returns early if any entry fails to produce an updated build.
fn eval_pmap_clauses(
    ctx: &PartialMapCtx<'_>,
    initial: MapBuild,
    entries: &[Spanned<PMapEntry>],
) -> Step<MapBuild> {
    let mut build = initial;
    let mut result = InterpResult::ok(ctx.context.clone());

    for entry in entries {
        let step_ctx = PartialMapCtx { context: &result.context, ..*ctx };
        let (next, entry_result) = match &entry.inner {
            PMapEntry::Clause(clause) => {
                interpret_partial_map_clause(&step_ctx, build, clause, entry.span)
            }
            PMapEntry::For(fb) => expand_pmap_for(&step_ctx, build, fb, entry.span),
        };
        result = result.merge(entry_result);
        let Some(updated) = next else {
            return (None, result);
        };
        build = updated;
        if result.has_errors() {
            return (Some(build), result);
        }
    }

    (Some(build), result)
}

fn expand_pmap_for(
    ctx: &PartialMapCtx<'_>,
    build: MapBuild,
    fb: &ForBlock,
    outer_span: Span,
) -> Step<MapBuild> {
    let values = match super::eval::resolve_index_values(ctx.domain, fb, outer_span, ctx.context) {
        Ok(v) => v,
        Err(result) => return (None, result),
    };
    let expanded = super::eval::expand_body(fb, &values);
    let entries = match crate::language::parse_pmap_clauses(&expanded) {
        Ok(entries) => entries,
        Err(errors) => {
            let mut result = InterpResult::ok(ctx.context.clone());
            for err in errors {
                result.add_error(make_error(
                    outer_span,
                    format!("In for-block expansion: {}", err),
                ));
            }
            return (None, result);
        }
    };
    let (build_opt, mut result) = eval_pmap_clauses(ctx, build, &entries);
    super::eval::relocate_errors(&mut result, outer_span);
    (build_opt, result)
}

/// Check that every entry in `map` is defined on a generator of `source`, and
/// that local cells have compatible boundary data.
fn validate_map_as_source(
    map: &PartialMap,
    map_domain: &Complex,
    source: &Complex,
    span: Span,
    mut result: InterpResult,
) -> InterpResult {
    for (_, tags) in map.domain_by_dim() {
        for tag in &tags {
            if source.find_generator_by_tag(tag).is_none() {
                let name = map_domain.find_generator_by_tag(tag)
                    .map(String::as_str).unwrap_or("?");
                result.add_error(make_error(span,
                    format!("Map defined on `{}` which is not in the specified domain", name)));
                return result;
            }
            if let Tag::Local(local_name) = tag {
                match (source.find_local_cell(local_name), map.cell_data(tag)) {
                    (Some(source_data), Some(map_data)) => {
                        if !cell_data_compatible(source_data, map_data) {
                            result.add_error(make_error(span,
                                "Local cell mismatch between map and specified domain"));
                            return result;
                        }
                    }
                    _ => {
                        result.add_error(make_error(span,
                            "Local cell mismatch between map and specified domain"));
                        return result;
                    }
                }
            }
        }
    }
    result
}

fn cell_data_compatible(lhs: &CellData, rhs: &CellData) -> bool {
    match (lhs, rhs) {
        (CellData::Zero, CellData::Zero) => true,
        (CellData::Boundary { boundary_in: in_l, boundary_out: out_l },
         CellData::Boundary { boundary_in: in_r, boundary_out: out_r }) =>
            Diagram::isomorphic(in_l, in_r) && Diagram::isomorphic(out_l, out_r),
        _ => false,
    }
}

/// Interpret an extension-style partial map (`prefix? [ clause, … ]`).
///
/// Evaluates the optional prefix, then each clause in order.  Pending `arr => ?`
/// clauses are turned into holes in a final pass that processes them in ascending
/// source dimension, so a hole's (lower-dimensional) faces — concrete or holes
/// themselves — exist before it.
fn interpret_partial_map_ext(ctx: &PartialMapCtx<'_>, ext: &PartialMapExt) -> Step<EvalMap> {
    let (initial_opt, prefix_result) = initial_eval_map(ctx, &ext.prefix);
    let Some(initial) = initial_opt else {
        return (None, prefix_result);
    };

    let domain = Arc::clone(&initial.domain);
    let init_build = MapBuild {
        map: initial.map,
        holes: initial.holes,
        current_span: Span::synthetic(),
        hole_spans: HashMap::new(),
        blame_span: None,
    };
    let clauses_ctx = PartialMapCtx { context: &prefix_result.context, domain: &domain, ..*ctx };
    let (build_opt, clause_result) = eval_pmap_clauses(&clauses_ctx, init_build, &ext.clauses);
    let Some(build) = build_opt else {
        return (None, prefix_result.merge(clause_result));
    };
    let result = prefix_result.merge(clause_result);
    (Some(EvalMap { map: build.map, domain, holes: build.holes }), result)
}

/// Interpret a single clause `lhs => rhs` in a partial map extension block.
///
/// A bare-`?` right-hand side (`arr => ?`) is the pure-hole assignment; otherwise
/// both sides are evaluated and `interpret_assign` extends the map.  Either may
/// create holes (for boundary faces on which information is incomplete) and may
/// close earlier holes (filling), via `assign_cell`.
fn interpret_partial_map_clause(ctx: &PartialMapCtx<'_>, mut build: MapBuild, clause: &PartialMapClause, span: Span) -> Step<MapBuild> {
    // Tag any pending entry created by this clause with its span, so a later
    // cascade can blame the right assignment.
    build.current_span = span;

    let (left_opt, left_result) = interpret_diagram_as_term(ctx.context, ctx.domain, &clause.lhs);
    let Some(left_term) = left_opt else { return (None, left_result); };

    // Pure-hole RHS `... => ?`: the image is unknown.  A cell source becomes one
    // hole; a map source holes every constituent cell of its image (pointwise).
    // The hole is its own RHS variant — there is no diagram to evaluate.
    let rhs = match &clause.rhs {
        ast::ClauseRhs::Hole(_) => {
            let res = match &left_term {
                Term::Diag(source) => assign_cell(&mut build, &left_result.context, ctx.domain, source, None),
                Term::Map(f_map) => hole_map_image(&mut build, &left_result.context, ctx.domain, f_map),
            };
            return match res {
                Ok(()) => (Some(build), left_result),
                Err(e) => {
                    let mut r = left_result;
                    let blame = build.blame_span.take().unwrap_or(span);
                    r.add_error(make_error_from_core(blame, e));
                    (None, r)
                }
            };
        }
        ast::ClauseRhs::Diagram(rhs) => rhs,
    };

    let (right_opt, right_result) =
        interpret_diagram_as_term(&left_result.context, ctx.scope, rhs);
    let mut combined = left_result.merge(right_result);
    let Some(right_term) = right_opt else { return (None, combined); };

    match interpret_assign(&mut build, &combined.context, ctx.domain, &left_term, &right_term) {
        Ok(()) => (Some(build), combined),
        Err(e) => {
            let blame = build.blame_span.take().unwrap_or(span);
            combined.add_error(make_error_from_core(blame, e));
            (None, combined)
        }
    }
}

/// Match two evaluated map terms pointwise, assigning each shared-domain
/// generator's left image to its right image (creating holes on incomplete info).
fn extend_matching_map_images(
    build: &mut MapBuild,
    context: &Context,
    domain: &Complex,
    left_map: &EvalMap,
    right_map: &EvalMap,
) -> Result<(), aux::Error> {
    let map_domain = &*left_map.domain;

    for (_, generator_name, tag) in sorted_generators(map_domain) {
        match (left_map.map.is_defined_at(&tag), right_map.map.is_defined_at(&tag)) {
            (false, _) => {}
            (true, false) => return Err(aux::Error::new(format!(
                "`{}` is in the domain of the first map but not the second",
                generator_name
            ))),
            (true, true) => {
                let left_image = left_map.map.image(&tag)?;
                if left_image.is_cell() {
                    let right_image = right_map.map.image(&tag)?;
                    assign_cell(build, context, domain, left_image, Some(right_image))?;
                } else {
                    let all_defined = left_image.all_labels().all(|t| build.map.is_defined_at(t));
                    if !all_defined {
                        return Err(aux::Error::new("Failed to extend map (not enough information)"));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Map every image of an evaluated map to a constant 0-dimensional diagram.
fn extend_map_to_constant(
    build: &mut MapBuild,
    context: &Context,
    domain: &Complex,
    left_map: &EvalMap,
    point: &Diagram,
) -> Result<(), aux::Error> {
    let map_domain = &*left_map.domain;

    for (_, _, tag) in sorted_generators(map_domain) {
        if !left_map.map.is_defined_at(&tag) {
            continue;
        }
        let left_image = left_map.map.image(&tag)?;
        if left_image.is_cell() {
            assign_cell(build, context, domain, left_image, Some(point))?;
        } else {
            let all_defined = left_image.all_labels().all(|t| build.map.is_defined_at(t));
            if !all_defined {
                return Err(aux::Error::new("Failed to extend map (not enough information)"));
            }
        }
    }

    Ok(())
}

/// Process a `lhs => rhs` assignment, dispatching on whether the terms are diagrams or maps.
fn interpret_assign(
    build: &mut MapBuild,
    context: &Context,
    domain: &Complex,
    left: &Term,
    right: &Term,
) -> Result<(), aux::Error> {
    match (left, right) {
        (Term::Diag(d_left), Term::Diag(d_right)) => {
            assign_cell(build, context, domain, d_left, Some(d_right))
        }
        (Term::Map(mc_left), Term::Map(mc_right)) => {
            if !Arc::ptr_eq(&mc_left.domain, &mc_right.domain) {
                return Err(aux::Error::new("Not a well-formed assignment"));
            }
            extend_matching_map_images(build, context, domain, mc_left, mc_right)
        }
        (Term::Map(mc_left), Term::Diag(d_right)) if d_right.dim() == 0 => {
            extend_map_to_constant(build, context, domain, mc_left, d_right)
        }
        _ => Err(aux::Error::new("Not a well-formed assignment")),
    }
}

/// Collect the boundary cell tags not yet defined in the map, together with their sign.
fn boundary_dependencies(cell_data: &CellData, map: &PartialMap) -> Vec<(Tag, DiagramSign)> {
    let CellData::Boundary { boundary_in, boundary_out } = cell_data else {
        return vec![];
    };
    [(boundary_in.as_ref(), DiagramSign::Input), (boundary_out.as_ref(), DiagramSign::Output)]
        .into_iter()
        .flat_map(|(boundary, sign)| {
            let d = boundary.top_dim();
            boundary.labels_at(d).into_iter().flat_map(move |row| {
                row.iter()
                    .filter(|tag| !map.is_defined_at(tag))
                    .map(move |tag| (tag.clone(), sign))
            })
        })
        .collect()
}

/// Extract the input or output boundary from cell data, or `None` for a 0-cell.
fn boundary_of_sign(
    cell_data: &CellData,
    sign: DiagramSign,
) -> Option<Arc<Diagram>> {
    match (cell_data, sign) {
        (CellData::Boundary { boundary_in, .. }, DiagramSign::Input) => Some(boundary_in.clone()),
        (CellData::Boundary { boundary_out, .. }, DiagramSign::Output) => Some(boundary_out.clone()),
        _ => None,
    }
}

/// If a boundary of an n-cell (`dim` = n) is already mapped to a diagram of
/// dimension strictly below `n - 1` (a collapse), return that diagram — the only
/// possible image of the cell, since a diagram whose `(n-1)`-boundary is that
/// (lower-dimensional) diagram must equal it.  Returns `None` if neither boundary
/// is fully mapped or neither collapses.
fn collapsed_boundary_image(map: &PartialMap, cell_data: &CellData, dim: usize) -> Option<Diagram> {
    for sign in [DiagramSign::Input, DiagramSign::Output] {
        let Some(boundary) = boundary_of_sign(cell_data, sign) else { continue; };
        if let Ok(image) = PartialMap::apply(map, boundary.as_ref()) {
            // dim(image) < n - 1, written without underflow.
            if image.top_dim() + 1 < dim {
                return Some(image);
            }
        }
    }
    None
}


// ---- Partial map naming ----

/// Verify that every generator in the domain is mapped; report an error for each gap.
///
/// Only checks if `is_total` is `true`.
fn check_map_totality(
    result: &mut InterpResult,
    domain: &Complex,
    map: &PartialMap,
    holes: &[MapHole],
    map_name: &str,
    name_span: Span,
    is_total: bool,
) {
    if !is_total {
        return;
    }

    // A generator counts as covered if it is committed *or* has a pending entry
    // (a hole or conditional): `total` is for catching missed generators, and a
    // hole is a deliberate placeholder, not an omission.
    for (generator_name, tag, _) in domain.generators_iter() {
        let covered = map.is_defined_at(tag) || holes.iter().any(|h| &h.source == tag);
        if !covered {
            result.add_error(make_error(
                name_span,
                format!("Total map `{}` is not defined on generator `{}`", map_name, generator_name),
            ));
        }
    }
}

/// Shared post-resolution logic for named partial map definitions.
///
/// Given a resolved domain (complex + MapDomain), interprets the map body,
/// checks totality, and returns the binding triple.
fn finish_def_pmap(
    scope: &Complex,
    domain: &Complex,
    map_domain: MapDomain,
    dp: &DefPartialMap,
    prior_result: InterpResult,
) -> (Option<(LocalId, PartialMap, MapDomain, Vec<MapHole>)>, InterpResult) {
    let (eval_map_opt, def_result) = interpret_pmap_def(
        &prior_result.context, scope, domain, &dp.value,
    );
    let mut combined = prior_result.merge(def_result);

    let Some(eval_map) = eval_map_opt else {
        return (None, combined);
    };

    check_map_totality(&mut combined, domain, &eval_map.map, &eval_map.holes, &dp.name.inner, dp.name.span, dp.total);
    if combined.has_errors() {
        return (None, combined);
    }

    let name = dp.name.inner.clone();
    (Some((name, eval_map.map, map_domain, eval_map.holes)), combined)
}

/// Interpret a named partial map definition, resolving the domain as a type
/// via `interpret_address`. Used in complex and local blocks.
pub fn interpret_def_pmap(
    context: &Context,
    scope: &Complex,
    dp: &DefPartialMap,
) -> (Option<(LocalId, PartialMap, MapDomain, Vec<MapHole>)>, InterpResult) {
    let (id_opt, addr_result) = interpret_address(context, &dp.address.inner, dp.address.span);
    let Some(id) = id_opt else {
        return (None, addr_result);
    };

    let context_after = addr_result.context.clone();
    let (domain_opt, domain_result) =
        resolve_type_complex(&context_after, id, dp.address.span, "Type not found");
    let Some(domain) = domain_opt else {
        return (None, addr_result.merge(domain_result));
    };

    finish_def_pmap(
        scope, &domain, MapDomain::Type(id), dp,
        addr_result.merge(domain_result),
    )
}

/// Interpret a named partial map definition, resolving the domain as a module
/// via the module names table. Used in type blocks.
pub fn interpret_def_pmap_module(
    context: &Context,
    scope: &Complex,
    dp: &DefPartialMap,
) -> (Option<(LocalId, PartialMap, MapDomain, Vec<MapHole>)>, InterpResult) {
    let (resolved_opt, resolve_result) =
        resolve_module_domain(context, &dp.address.inner, dp.address.span);
    let Some(resolved) = resolved_opt else {
        return (None, resolve_result);
    };

    finish_def_pmap(
        scope, resolved.complex(), resolved.map_domain(), dp,
        resolve_result,
    )
}
