use super::diagram::{interpret_diagram_as_term, is_pure_hole_diagram};
use super::resolve::{interpret_address, resolve_map_domain_complex, resolve_module_domain, resolve_type_complex};
use super::types::{
    Context, EvalMap, InterpResult, Step, Term,
    fail, get_cell_data, make_error, make_error_from_core, sorted_generators,
};
use crate::aux::{self, HoleId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, Sign as DiagramSign},
    map_hole::{collect_hole_deps, MapHole},
    partial_map::PartialMap,
    paste_tree::PasteTree,
};
use crate::language::ast::{self, DefPartialMap, ForBlock, PMapEntry, PartialMapBasic, PartialMapClause, PartialMapDef, PartialMapExt, Span, Spanned};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

// ---- Map holes ----

/// The map under construction in an extension block: the real (hole-free)
/// partial map, the source cells of `arr => ?` clauses still to be turned into
/// holes (paired with their span), and the holes built so far.
struct MapBuild {
    map: PartialMap,
    pending_holes: Vec<(Diagram, Span)>,
    holes: Vec<MapHole>,
}

/// Build a [`MapHole`] for an `arr => ?` clause from the source cell `source_diag`.
///
/// The hole stands for the unknown image of `arr`.  Its boundaries are the
/// source cell's boundaries transported through the map *syntactically*: each
/// leaf of `∂arr`'s paste tree is replaced by the concrete image's paste tree
/// (if that face is mapped) or by a metavariable leaf (if that face is itself a
/// prior hole).  The result is never realised into a diagram, so a non-round
/// boundary is fine.  Errors if a face is neither mapped nor a hole.
fn make_map_hole(
    context: &Context,
    domain: &Complex,
    map: &PartialMap,
    prior_holes: &[MapHole],
    source_diag: &Diagram,
) -> Result<MapHole, aux::Error> {
    if !source_diag.is_cell() {
        return Err(aux::Error::new("the source of a hole must be a single cell"));
    }
    let source = source_diag
        .top_label()
        .ok_or_else(|| aux::Error::new("hole source cell has no top label"))?
        .clone();
    let dim = source_diag.top_dim();

    let cell_data = get_cell_data(context, domain, &source)
        .ok_or_else(|| aux::Error::new("cannot find cell data for hole source"))?;

    let CellData::Boundary { boundary_in, boundary_out } = &cell_data else {
        // A 0-cell has no boundary; its image is just an unknown 0-cell.
        return Ok(MapHole {
            meta: HoleId::fresh(),
            source,
            dim: 0,
            boundary_in: None,
            boundary_out: None,
            deps: BTreeSet::new(),
        });
    };

    let in_tree = transport_boundary(domain, map, prior_holes, boundary_in)?;
    let out_tree = transport_boundary(domain, map, prior_holes, boundary_out)?;
    let mut deps = collect_hole_deps(&in_tree);
    deps.extend(collect_hole_deps(&out_tree));
    Ok(MapHole {
        meta: HoleId::fresh(),
        source,
        dim,
        boundary_in: Some(in_tree),
        boundary_out: Some(out_tree),
        deps,
    })
}

/// Transport a source-cell boundary through the map, as a paste tree.  Every
/// leaf is rewritten to the concrete image's paste tree or, for a face that is
/// itself a hole, to a metavariable leaf.
fn transport_boundary(
    domain: &Complex,
    map: &PartialMap,
    prior_holes: &[MapHole],
    boundary: &Diagram,
) -> Result<PasteTree, aux::Error> {
    let n = boundary.top_dim();
    let tree = boundary
        .tree(DiagramSign::Input, n)
        .ok_or_else(|| aux::Error::new("boundary diagram has no paste tree"))?;

    // Build the full rewrite table first, erroring on any leaf that is neither
    // mapped nor a hole (so we never rely on `substitute` to surface a gap).
    let mut rewrites: HashMap<Tag, PasteTree> = HashMap::new();
    collect_leaf_rewrites(tree, domain, map, prior_holes, &mut rewrites)?;
    Ok(tree.substitute(&|t| rewrites.get(t).cloned()))
}

/// Walk the leaves of `tree`, recording for each domain tag the paste tree it
/// should be replaced by.  Errors on a leaf that is neither in the map nor a hole.
fn collect_leaf_rewrites(
    tree: &PasteTree,
    domain: &Complex,
    map: &PartialMap,
    prior_holes: &[MapHole],
    rewrites: &mut HashMap<Tag, PasteTree>,
) -> Result<(), aux::Error> {
    match tree {
        PasteTree::Leaf(tag) => {
            if rewrites.contains_key(tag) {
                return Ok(());
            }
            if map.is_defined_at(tag) {
                let image = map.image(tag)?;
                let img_tree = image
                    .tree(DiagramSign::Input, image.top_dim())
                    .ok_or_else(|| aux::Error::new("map image has no paste tree"))?
                    .clone();
                rewrites.insert(tag.clone(), img_tree);
                Ok(())
            } else if let Some(h) = prior_holes.iter().find(|h| &h.source == tag) {
                rewrites.insert(tag.clone(), PasteTree::Leaf(Tag::Hole(h.meta)));
                Ok(())
            } else {
                let name = domain
                    .find_generator_by_tag(tag)
                    .cloned()
                    .unwrap_or_else(|| format!("{}", tag));
                Err(aux::Error::new(format!(
                    "this hole's boundary references `{}`, which is neither mapped nor a hole",
                    name
                )))
            }
        }
        PasteTree::Node { left, right, .. } => {
            collect_leaf_rewrites(left, domain, map, prior_holes, rewrites)?;
            collect_leaf_rewrites(right, domain, map, prior_holes, rewrites)
        }
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
            let (domain_opt, result) = resolve_map_domain_complex(ctx.context, domain, span);
            let Some(domain) = domain_opt else {
                return (None, result);
            };
            (Some(EvalMap { map: map.clone(), domain, holes: vec![] }), InterpResult::ok(ctx.context.clone()))
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

/// Interpret an extension-style partial map (`{ prefix? clause* }`).
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
        pending_holes: vec![],
        holes: initial.holes,
    };
    let clauses_ctx = PartialMapCtx { context: &prefix_result.context, domain: &domain, ..*ctx };
    let (build_opt, clause_result) = eval_pmap_clauses(&clauses_ctx, init_build, &ext.clauses);
    let Some(mut build) = build_opt else {
        return (None, prefix_result.merge(clause_result));
    };
    let mut result = prefix_result.merge(clause_result);

    // Finalize holes only on a clean run: build each `MapHole` in ascending source
    // dimension so its faces are already in the map or in `holes` (as metavariables).
    if !result.has_errors() && !build.pending_holes.is_empty() {
        let mut pending = std::mem::take(&mut build.pending_holes);
        pending.sort_by_key(|(d, _)| d.top_dim());
        for (source_diag, span) in pending {
            match make_map_hole(&result.context, &domain, &build.map, &build.holes, &source_diag) {
                Ok(hole) => build.holes.push(hole),
                Err(e) => {
                    result.add_error(make_error_from_core(span, e));
                    return (None, result);
                }
            }
        }
    }

    (Some(EvalMap { map: build.map, domain, holes: build.holes }), result)
}

/// Interpret a single clause `lhs => rhs` in a partial map extension block.
///
/// A bare-`?` right-hand side (`arr => ?`) is the basic hole case: the source
/// cell is recorded as a pending hole and the RHS is *not* evaluated as a diagram.
/// Otherwise both sides are evaluated and `interpret_assign` extends the map.
fn interpret_partial_map_clause(ctx: &PartialMapCtx<'_>, mut build: MapBuild, clause: &PartialMapClause, span: Span) -> Step<MapBuild> {
    let (left_opt, left_result) = interpret_diagram_as_term(ctx.context, ctx.domain, &clause.lhs);
    let Some(left_term) = left_opt else { return (None, left_result); };

    // Basic hole case: `arr => ?`.  Record the source cell; defer hole construction.
    if is_pure_hole_diagram(&clause.rhs.inner) {
        return match left_term {
            Term::Diag(d) => {
                build.pending_holes.push((d, span));
                (Some(build), left_result)
            }
            Term::Map(_) => {
                let mut r = left_result;
                r.add_error(make_error(span, "The source of a hole must be a single cell, not a map"));
                (None, r)
            }
        };
    }

    let (right_opt, right_result) =
        interpret_diagram_as_term(&left_result.context, ctx.scope, &clause.rhs);
    let mut combined = left_result.merge(right_result);
    let Some(right_term) = right_opt else { return (None, combined); };

    match interpret_assign(&combined.context, build.map, ctx.domain, ctx.scope, &left_term, &right_term) {
        Ok(new_map) => {
            build.map = new_map;
            (Some(build), combined)
        }
        Err(e) => {
            combined.add_error(make_error_from_core(span, e));
            (None, combined)
        }
    }
}

/// Match two evaluated map terms pointwise, extending the map for each generator in the shared domain.
fn extend_matching_map_images(
    context: &Context,
    map: PartialMap,
    domain: &Complex,
    target: &Complex,
    left_map: &EvalMap,
    right_map: &EvalMap,
) -> Result<PartialMap, aux::Error> {
    let map_domain = &*left_map.domain;
    let mut extended = map;

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
                    extended = extend_map_for_cell(context, extended, domain, target, left_image, right_image)?;
                } else {
                    let all_defined = left_image.all_labels().all(|tag| extended.is_defined_at(tag));
                    if !all_defined {
                        return Err(aux::Error::new("Failed to extend map (not enough information)"));
                    }
                }
            }
        }
    }

    Ok(extended)
}

/// Map every image of an evaluated map to a constant 0-dimensional diagram.
fn extend_map_to_constant(
    context: &Context,
    map: PartialMap,
    domain: &Complex,
    target: &Complex,
    left_map: &EvalMap,
    point: &Diagram,
) -> Result<PartialMap, aux::Error> {
    let map_domain = &*left_map.domain;
    let mut extended = map;

    for (_, _, tag) in sorted_generators(map_domain) {
        if !left_map.map.is_defined_at(&tag) {
            continue;
        }
        let left_image = left_map.map.image(&tag)?;
        if left_image.is_cell() {
            extended = extend_map_for_cell(context, extended, domain, target, left_image, point)?;
        } else {
            let all_defined = left_image.all_labels().all(|tag| extended.is_defined_at(tag));
            if !all_defined {
                return Err(aux::Error::new("Failed to extend map (not enough information)"));
            }
        }
    }

    Ok(extended)
}

/// Process a `lhs => rhs` assignment, dispatching on whether the terms are diagrams or maps.
fn interpret_assign(
    context: &Context,
    map: PartialMap,
    domain: &Complex,
    target: &Complex,
    left: &Term,
    right: &Term,
) -> Result<PartialMap, aux::Error> {
    match (left, right) {
        (Term::Diag(d_left), Term::Diag(d_right)) => {
            extend_map_for_cell(context, map, domain, target, d_left, d_right)
        }
        (Term::Map(mc_left), Term::Map(mc_right)) => {
            if !Arc::ptr_eq(&mc_left.domain, &mc_right.domain) {
                return Err(aux::Error::new("Not a well-formed assignment"));
            }
            extend_matching_map_images(context, map, domain, target, mc_left, mc_right)
        }
        (Term::Map(mc_left), Term::Diag(d_right)) if d_right.dim() == 0 => {
            extend_map_to_constant(context, map, domain, target, mc_left, d_right)
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

/// Determine the image classifier for a boundary cell by shape isomorphism.
///
/// Maps `focus` through the isomorphism between `source_boundary` and `target_boundary`,
/// then looks up the resulting tag's classifier in `target`.
fn image_classifier_via_boundary(
    focus: &Tag,
    source_boundary: &Diagram,
    target_boundary: &Diagram,
    target: &Complex,
) -> Result<Diagram, aux::Error> {
    let mapped_tag = Diagram::map_tag_via_shape_iso(source_boundary, target_boundary, focus)
        .map_err(|e| aux::Error::new(format!("Failed to extend map ({})", e)))?;
    let generator_name = target
        .find_generator_by_tag(&mapped_tag)
        .ok_or_else(|| aux::Error::new("Image tag not found in target complex"))?
        .clone();
    target.classifier(&generator_name)
        .ok_or_else(|| aux::Error::new("Classifier not found for image generator"))
        .cloned()
}

/// Smart extension of a map: adds a mapping from a source cell to a target diagram,
/// recursively extending for boundary cells as needed.
pub fn extend_map_for_cell(
    context: &Context,
    map: PartialMap,
    domain: &Complex,
    target: &Complex,
    domain_diag: &Diagram,
    target_diag: &Diagram,
) -> Result<PartialMap, aux::Error> {
    if !domain_diag.is_cell() {
        return Err(aux::Error::new(
            "Left-hand side of map instruction must be a cell",
        ));
    }
    let d = domain_diag.top_dim();
    let tag = domain_diag
        .top_label()
        .ok_or_else(|| aux::Error::new("Domain cell has no top label"))?
        .clone();

    if map.is_defined_at(&tag) {
        let current = map.image(&tag)?;
        if Diagram::isomorphic(current, target_diag) {
            return Ok(map);
        } else {
            return Err(aux::Error::new(
                "The same generator is mapped to multiple diagrams",
            ));
        }
    }

    let cell_data = get_cell_data(context, domain, &tag)
        .ok_or_else(|| aux::Error::new("Cannot find cell data for generator"))?;

    // Extend the map for any boundary dependencies not yet in the domain.
    let mut current_map = map;
    for (focus_tag, sign) in boundary_dependencies(&cell_data, &current_map) {
        if current_map.is_defined_at(&focus_tag) {
            continue;
        }
        let focus_cell_data = get_cell_data(context, domain, &focus_tag).ok_or_else(|| {
            aux::Error::new(format!("Cannot find cell data for boundary cell {}", focus_tag))
        })?;
        let target_boundary = Diagram::boundary(sign, d - 1, target_diag)?;
        let Some(source_boundary) = boundary_of_sign(&cell_data, sign) else { continue; };
        current_map = if source_boundary.is_cell() {
            extend_map_for_cell(context, current_map, domain, target, &source_boundary, &target_boundary)?
        } else {
            let focus_image = image_classifier_via_boundary(&focus_tag, &source_boundary, &target_boundary, target)?;
            let focus_diagram = Diagram::cell(focus_tag.clone(), &focus_cell_data)?;
            extend_map_for_cell(context, current_map, domain, target, &focus_diagram, &focus_image)?
        };
    }

    PartialMap::extend(current_map, tag, d, cell_data, target_diag.clone())
}

// ---- Partial map naming ----

/// Verify that every generator in the domain is mapped; report an error for each gap.
///
/// Only checks if `is_total` is `true`.
fn check_map_totality(
    result: &mut InterpResult,
    domain: &Complex,
    map: &PartialMap,
    map_name: &str,
    name_span: Span,
    is_total: bool,
) {
    if !is_total {
        return;
    }

    for (generator_name, tag, _) in domain.generators_iter() {
        if !map.is_defined_at(tag) {
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

    check_map_totality(&mut combined, domain, &eval_map.map, &dp.name.inner, dp.name.span, dp.total);
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
