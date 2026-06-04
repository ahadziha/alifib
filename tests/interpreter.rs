use std::path::PathBuf;

use alifib::aux::loader::Loader;
use alifib::interpreter::InterpretedFile;
use alifib::output::{Cell, Dim, Map, Module, Store, Type};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn example(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// `Delta.ali` asserts the cosimplicial simplicial identities (`d0.d1 = d2.d0`, …),
/// which are pure map-composition equalities — exercising the dotted-expression
/// *map* form, where the whole chain is collected and composed in one pass.  A
/// clean load means every assertion held.
#[test]
fn delta_simplicial_identities_hold() {
    InterpretedFile::load(&Loader::default(vec![]), &example("Delta.ali"))
        .ok()
        .expect("Delta.ali should interpret without errors (simplicial identities hold)");
}

#[test]
fn magma_interpretation() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("Magma.ali"))
        .ok()
        .expect("Magma.ali should interpret without errors");


    let norm = file.state.normalize();

    let pt   = || Cell { name: "pt".into(),    input: String::new(),       output: String::new()       };
    let ob   = || Cell { name: "ob".into(),    input: "pt".into(),         output: "pt".into()         };
    let obpt = || Cell { name: "Ob.pt".into(), input: String::new(),       output: String::new()       };
    let obob = || Cell { name: "Ob.ob".into(), input: "Ob.pt".into(),     output: "Ob.pt".into()      };

    assert_eq!(norm, Store {
        cells_count: 12,
        types_count: 5,
        modules: vec![Module {
            path: fixture("Magma.ali"),
            types: vec![
                Type { name: String::new(), dims: vec![], diagrams: vec![], maps: vec![] },
                Type {
                    name: "Ob".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec![pt()] },
                        Dim { dim: 1, cells: vec![ob()] },
                    ],
                    diagrams: vec![ob(), pt()],
                    maps: vec![Map { name: "Ob".into(), domain: "Ob".into(), holes: vec![] }],
                },
                Type {
                    name: "Magma".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec![obpt()] },
                        Dim { dim: 1, cells: vec![obob()] },
                        Dim { dim: 2, cells: vec![
                            Cell { name: "m".into(), input: "(Ob.ob #0 Ob.ob)".into(), output: "Ob.ob".into() },
                        ]},
                    ],
                    diagrams: vec![
                        Cell { name: "m".into(), input: "(Ob.ob #0 Ob.ob)".into(), output: "Ob.ob".into() },
                    ],
                    maps: vec![Map { name: "Magma".into(), domain: "Magma".into(), holes: vec![] }, Map { name: "Ob".into(), domain: "Ob".into(), holes: vec![] }],
                },
                Type {
                    name: "Comagma".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec![obpt()] },
                        Dim { dim: 1, cells: vec![obob()] },
                        Dim { dim: 2, cells: vec![
                            Cell { name: "c".into(), input: "Ob.ob".into(), output: "(Ob.ob #0 Ob.ob)".into() },
                        ]},
                    ],
                    diagrams: vec![
                        Cell { name: "c".into(), input: "Ob.ob".into(), output: "(Ob.ob #0 Ob.ob)".into() },
                    ],
                    maps: vec![Map { name: "Comagma".into(), domain: "Comagma".into(), holes: vec![] }, Map { name: "Ob".into(), domain: "Ob".into(), holes: vec![] }],
                },
                Type {
                    name: "FrobeniusMagma".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec![obpt()] },
                        Dim { dim: 1, cells: vec![obob()] },
                        Dim { dim: 2, cells: vec![
                            Cell { name: "Comagma.c".into(), input: "Ob.ob".into(), output: "(Ob.ob #0 Ob.ob)".into() },
                            Cell { name: "Magma.m".into(),   input: "(Ob.ob #0 Ob.ob)".into(), output: "Ob.ob".into() },
                        ]},
                    ],
                    diagrams: vec![],
                    maps: vec![
                        Map { name: "Comagma".into(),       domain: "Comagma".into(), holes: vec![] },
                        Map { name: "FrobeniusMagma".into(), domain: "FrobeniusMagma".into(), holes: vec![] },
                        Map { name: "Magma".into(),         domain: "Magma".into(), holes: vec![] },
                        Map { name: "Ob".into(),            domain: "Ob".into(), holes: vec![] },
                    ],
                },
            ],
        }],
    });
}

#[test]
fn empty2_single_type_with_one_cell() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("Empty2.ali"))
        .ok()
        .expect("Empty2.ali should interpret without errors");
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 1);
    assert_eq!(norm.types_count, 2);
    let c = norm.modules[0].types.iter().find(|t| t.name == "C").unwrap();
    assert_eq!(c.dims, vec![Dim { dim: 0, cells: vec![Cell { name: "c".into(), input: String::new(), output: String::new() }] }]);
    assert_eq!(c.maps, vec![Map { name: "C".into(), domain: "C".into(), holes: vec![] }]);
}

#[test]
fn empty_maps_across_types() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("Empty.ali"))
        .ok()
        .expect("Empty.ali should interpret without errors");
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 2);
    assert_eq!(norm.types_count, 4);
    let module = &norm.modules[0];
    // D reuses C's generators
    let d = module.types.iter().find(|t| t.name == "D").unwrap();
    assert_eq!(d.dims, vec![Dim { dim: 0, cells: vec![Cell { name: "c".into(), input: String::new(), output: String::new() }] }]);
    // E has a module-level let g :: D = f, exposed as a map alongside f :: C
    let e = module.types.iter().find(|t| t.name == "E").unwrap();
    assert!(e.maps.contains(&Map { name: "f".into(), domain: "C".into(), holes: vec![] }));
    assert!(e.maps.contains(&Map { name: "g".into(), domain: "D".into(), holes: vec![] }));
}

#[test]
fn total_composite_map() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("Total.ali"))
        .ok()
        .expect("Total.ali should interpret without errors");
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 10);
    assert_eq!(norm.types_count, 3);
    let graph = norm.modules[0].types.iter().find(|t| t.name == "Graph").unwrap();
    // `let total F :: Arrow` should produce a map F :: Arrow
    assert!(graph.maps.contains(&Map { name: "F".into(), domain: "Arrow".into(), holes: vec![] }));
    // the mid cell is a diagram (it is explicitly named)
    assert!(graph.diagrams.contains(&Cell { name: "mid".into(), input: "A.t".into(), output: "B.s".into() }));
}

#[test]
fn tutorial_pair_maps() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("Tutorial.ali"))
        .ok()
        .expect("Tutorial.ali should interpret without errors");
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 21);
    assert_eq!(norm.types_count, 4);
    let pair = norm.modules[0].types.iter().find(|t| t.name == "Pair").unwrap();
    // Pair attaches f :: Mor, g :: Mor, and three Ob attachments
    assert!(pair.maps.contains(&Map { name: "f".into(), domain: "Mor".into(), holes: vec![] }));
    assert!(pair.maps.contains(&Map { name: "g".into(), domain: "Mor".into(), holes: vec![] }));
    assert!(pair.maps.contains(&Map { name: "x".into(), domain: "Ob".into(), holes: vec![] }));
    assert!(pair.maps.contains(&Map { name: "y".into(), domain: "Ob".into(), holes: vec![] }));
    assert!(pair.maps.contains(&Map { name: "z".into(), domain: "Ob".into(), holes: vec![] }));
}

#[test]
fn theory_function_and_set_maps() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("Theory.ali"))
        .ok()
        .expect("Theory.ali should interpret without errors");
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 26);
    assert_eq!(norm.types_count, 4);
    let module = &norm.modules[0];
    // Function attaches Dom :: Set, Cod :: Set, and two Equation laws
    let function = module.types.iter().find(|t| t.name == "Function").unwrap();
    assert!(function.maps.contains(&Map { name: "Dom".into(),    domain: "Set".into(), holes: vec![] }));
    assert!(function.maps.contains(&Map { name: "Cod".into(),    domain: "Set".into(), holes: vec![] }));
    assert!(function.maps.contains(&Map { name: "Id_fun".into(), domain: "Equation".into(), holes: vec![] }));
    assert!(function.maps.contains(&Map { name: "Fun_id".into(), domain: "Equation".into(), holes: vec![] }));
    // Set defines an identity Function at the module level
    let set = module.types.iter().find(|t| t.name == "Set").unwrap();
    assert!(set.maps.contains(&Map { name: "Id".into(), domain: "Function".into(), holes: vec![] }));
}

/// A bare `?` as the image of a partial-map clause (`arr => ?`) is the basic
/// hole case: the file loads without errors and the map records one hole.
#[test]
fn map_hole_basic_loads() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("MapHole.ali"))
        .ok()
        .expect("MapHole.ali should interpret without errors");
    let gid = file.state.find_type_gid("A").expect("type A should exist");
    let tc = &file.state.find_type(gid).expect("type A entry").complex;
    let holes = tc.map_holes("H").expect("map H should exist");
    assert_eq!(holes.len(), 1, "H should have exactly one hole (arr)");
}

/// A hole embedded in a composite RHS (`arr => ? g`) is not the basic case and
/// must be rejected.  `?` is only a clause RHS, not a diagram component, so this
/// is now caught at parse time.
#[test]
fn embedded_hole_is_error() {
    let result = InterpretedFile::load(&Loader::default(vec![]), &fixture("EmbeddedHole.ali"));
    assert!(!result.is_ok(), "an embedded `?` should fail to load");
}

/// Filling a pending assignment's boundary so as to violate the constraint it
/// imposes must blame the *pending* assignment, not the innocent filler.  In
/// `WrongFiller.ali`, `r => m` (with `r : f g -> h`) is pending; `f => a a` and
/// `g => a` then force `f g ↦ a a a`, breaking `m`'s input boundary `a a`.  The
/// error must name `r` and point back at the `r => m` clause — not at `g => a`,
/// which is well-formed on its own.
#[test]
fn wrong_filler_blames_pending_assignment() {
    let result = InterpretedFile::load(&Loader::default(vec![]), &fixture("WrongFiller.ali"));
    match result {
        alifib::interpreter::LoadResult::InterpError { errors, source, .. } => {
            let err = errors.first().expect("at least one error");
            assert!(
                err.message().contains("pending assignment of `r`"),
                "message should name the pending assignment `r`, got: {}",
                err.message()
            );
            let span = err.span();
            assert_eq!(
                &source[span.start..span.end],
                "r => m",
                "the caret should underline the pending clause, not the filler"
            );
        }
        _ => panic!("WrongFiller.ali should fail to interpret"),
    }
}

/// Layered holes: `e : a0 -> a1` with `a1 => ?` and `e => ?` makes `e`'s hole
/// depend on `a1`'s.  Building it requires processing holes in ascending
/// dimension (the 0-cell `a1` before the 1-cell `e`) and referencing `a1`'s
/// metavariable in `e`'s boundary; a clean load with two holes exercises that.
#[test]
fn layered_holes_load() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("LayeredHole.ali"))
        .ok()
        .expect("LayeredHole.ali should interpret without errors");
    let gid = file.state.find_type_gid("A").expect("type A should exist");
    let tc = &file.state.find_type(gid).expect("type A entry").complex;
    let holes = tc.map_holes("H").expect("map H should exist");
    assert_eq!(holes.len(), 2, "H should have two holes (a1 and e)");
}

/// Number of unfilled holes on a named map in a type of a loaded file.
fn hole_count(file: &alifib::interpreter::InterpretedFile, type_name: &str, map_name: &str) -> usize {
    let gid = file.state.find_type_gid(type_name).expect("type should exist");
    let tc = &file.state.find_type(gid).expect("type entry").complex;
    tc.map_holes(map_name).expect("map should exist").len()
}

/// Order-independence: mapping the 2-cell `x` before its composite-boundary
/// faces creates holes that are later filled and auto-committed, while mapping
/// `x` last never creates a hole — both end hole-less.
#[test]
fn fill_is_order_independent() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("MapFill.ali"))
        .ok()
        .expect("MapFill.ali should interpret without errors");
    assert_eq!(hole_count(&file, "A", "HxFirst"), 0, "x-first map should end hole-less");
    assert_eq!(hole_count(&file, "A", "HxLast"), 0, "x-last map should be hole-less");
}

/// A partially filled map keeps its residual holes; prefix-extending it with the
/// missing clause fills them and completes the map.
#[test]
fn prefix_extension_fills_holes() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("MapFill.ali"))
        .ok()
        .expect("MapFill.ali should interpret without errors");
    assert!(hole_count(&file, "A", "Partial") > 0, "Partial should still have holes");
    assert_eq!(hole_count(&file, "A", "Filled"), 0, "Filled = Partial [ g => G ] should be hole-less");
}

/// `arr => ?` followed by `arr => f` is the same as `arr => f`: the hole is
/// upgraded to a conditional and committed, leaving no holes.
#[test]
fn redundant_hole_then_value_commits() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("MapIdempotent.ali"))
        .ok()
        .expect("MapIdempotent.ali should interpret without errors");
    assert_eq!(hole_count(&file, "A", "H"), 0, "the `?` should be upgraded to `arr => f` and committed");
}

/// Collapse inference: if a boundary of a holed cell is mapped to a strictly
/// lower-dimensional diagram, the cell's image is forced to that diagram.  Here
/// `f, g => p` (a 0-cell), so `x => ?` infers `x => p` rather than making a hole.
#[test]
fn collapsed_boundary_infers_image() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("CollapseInfer.ali"))
        .ok()
        .expect("CollapseInfer.ali should interpret without errors");
    assert_eq!(hole_count(&file, "A", "H"), 0, "x => ? should infer x => p, not a hole");
}

/// Collapse inference fires for implicit faces and cascades: `w => ?` (a 3-cell)
/// holes its 2-cell faces a, b, which collapse to the 0-cell p (their 1-cell
/// boundaries do), which in turn collapses w — all inferred, no holes left.
#[test]
fn collapse_inference_cascades_through_implicit_faces() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("Cascade.ali"))
        .ok()
        .expect("Cascade.ali should interpret without errors");
    assert_eq!(hole_count(&file, "A", "H"), 0, "a, b and w should all be inferred to p");
}

/// Case-1 inference is parametric in the *source* cell's dimension: mapping a
/// 2-cell `x` to a 1-cell `e` must send `x.in` to `boundary Input 1 e` (= `e`,
/// clamped) and not to `e.in`.  A wrong (image-dimension) computation would map
/// the 1-cell `f` to a 0-cell and fail the boundary check, so a clean total load
/// confirms it.
#[test]
fn dimension_lowering_case1_is_sound() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("DimLower.ali"))
        .ok()
        .expect("DimLower.ali should interpret without errors");
    assert_eq!(hole_count(&file, "A", "H"), 0, "x => e should infer f, g, a0, a1 with no holes");
}

/// `<map> => ?` holes every constituent cell of the map's image: `Sub` picks out
/// `a, b, p`, so `Sub => ?` makes three holes.
#[test]
fn map_to_hole_holes_each_cell() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("MapToHole.ali"))
        .ok()
        .expect("MapToHole.ali should interpret without errors");
    assert_eq!(hole_count(&file, "A", "H"), 3, "Sub => ? should hole a, b and p");
}

/// A `total` map may use holes as placeholders: a generator covered by a hole
/// counts as covered, so `total H :: One = [ x => ? ]` is accepted.
#[test]
fn total_map_accepts_holes() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("TotalHole.ali"))
        .ok()
        .expect("a total map whose only generator is a hole should be accepted");
    assert_eq!(hole_count(&file, "A", "H"), 1, "the hole should still be recorded");
}

/// `arr => ?` on an already-defined generator does nothing — no error, no hole.
#[test]
fn hole_on_defined_generator_is_noop() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("HoleAfterValue.ali"))
        .ok()
        .expect("HoleAfterValue.ali should interpret without errors");
    assert_eq!(hole_count(&file, "A", "H"), 0, "`arr => ?` after `arr => f` should add no hole");
}

/// An *inferred* (case-1) assignment fills holes like an explicit one:
/// `[ x => ?, f => g ]` infers `x => g.in`, which must close `?x`, leaving the
/// map hole-less.
#[test]
fn inferred_assignment_fills_hole() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("InferFill.ali"))
        .ok()
        .expect("InferFill.ali should interpret without errors");
    assert_eq!(hole_count(&file, "A", "H"), 0, "inferring x => g.in should close ?x");
}

/// Filling a face inconsistently with a conditional's stored image must fail:
/// `x => X` forces the input boundary to be `F G`, so `g => G2` contradicts it.
#[test]
fn inconsistent_fill_is_error() {
    let result = InterpretedFile::load(&Loader::default(vec![]), &fixture("MapFillBad.ali"));
    assert!(!result.is_ok(), "an inconsistent fill should fail to load");
}

#[test]
fn for_index_expansion() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("ForIndex.ali"))
        .ok()
        .expect("ForIndex.ali should interpret without errors");


    let norm = file.state.normalize();

    let module = &norm.modules[0];

    // X should have generators a, b, c (expanded from for-block)
    let x_type = module.types.iter().find(|t| t.name == "X").unwrap();
    let x_dim0_names: Vec<&str> = x_type.dims.iter()
        .filter(|d| d.dim == 0)
        .flat_map(|d| d.cells.iter())
        .map(|c| c.name.as_str())
        .collect();
    assert!(x_dim0_names.contains(&"a"), "X should contain generator 'a'");
    assert!(x_dim0_names.contains(&"b"), "X should contain generator 'b'");
    assert!(x_dim0_names.contains(&"c"), "X should contain generator 'c'");

    // Y should have generators x, y (dim 1) with boundaries Ob.a -> Ob.b
    let y_type = module.types.iter().find(|t| t.name == "Y").unwrap();
    let y_dim1_names: Vec<&str> = y_type.dims.iter()
        .filter(|d| d.dim == 1)
        .flat_map(|d| d.cells.iter())
        .map(|c| c.name.as_str())
        .collect();
    assert!(y_dim1_names.contains(&"x"), "Y should contain generator 'x'");
    assert!(y_dim1_names.contains(&"y"), "Y should contain generator 'y'");
}

#[test]
fn submodule_in_same_named_directory() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("SubMod.ali"))
        .ok()
        .expect("SubMod.ali should resolve Aux from SubMod/ subdirectory");


    let norm = file.state.normalize();
    let module = &norm.modules[0];
    assert!(module.types.iter().any(|t| t.name == "Ob"),
        "Aux.Ob should be included from SubMod/Aux.ali");

    let module = &norm.modules[1];
    assert!(module.types.iter().any(|t| t.name == "Magma"),
        "Magma should be defined in SubMod.ali");
}

#[test]
fn virtual_loader_subdirectory_resolution() {
    use std::collections::HashMap;

    let files: HashMap<String, String> = [
        ("source.ali".into(), "@Type\ninclude A,\ninclude B,\nX <<= { attach OA :: A.Aux.Ob, attach OB :: B.Aux.Ob }".into()),
        ("A.ali".into(), "@Type\ninclude Aux,\nAType <<= { attach Ob :: Aux.Ob }".into()),
        ("A/Aux.ali".into(), "@Type\nOb <<= { pt, ob: pt -> pt }".into()),
        ("B.ali".into(), "@Type\ninclude Aux,\nBType <<= { attach Ob :: Aux.Ob }".into()),
        ("B/Aux.ali".into(), "@Type\nOb <<= { pt, ob: pt -> pt }".into()),
    ].into_iter().collect();

    let loader = Loader::with_virtual_files(files);
    let file = InterpretedFile::load(&loader, "source.ali")
        .ok()
        .expect("virtual loader should resolve Aux in subdirectories A/ and B/");

    let norm = file.state.normalize();
    assert!(norm.modules.iter().any(|m| m.types.iter().any(|t| t.name == "AType")));
    assert!(norm.modules.iter().any(|m| m.types.iter().any(|t| t.name == "BType")));
}

