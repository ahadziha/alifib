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

fn legacy_example(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("legacy/examples")
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
                    maps: vec![Map { name: "Ob".into(), domain: "Ob".into() }],
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
                    maps: vec![Map { name: "Magma".into(), domain: "Magma".into() }, Map { name: "Ob".into(), domain: "Ob".into() }],
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
                    maps: vec![Map { name: "Comagma".into(), domain: "Comagma".into() }, Map { name: "Ob".into(), domain: "Ob".into() }],
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
                        Map { name: "Comagma".into(),       domain: "Comagma".into() },
                        Map { name: "FrobeniusMagma".into(), domain: "FrobeniusMagma".into() },
                        Map { name: "Magma".into(),         domain: "Magma".into() },
                        Map { name: "Ob".into(),            domain: "Ob".into() },
                    ],
                },
            ],
        }],
    });
}

#[test]
fn empty2_single_type_with_one_cell() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &legacy_example("Empty2.ali"))
        .ok()
        .expect("Empty2.ali should interpret without errors");
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 1);
    assert_eq!(norm.types_count, 2);
    let c = norm.modules[0].types.iter().find(|t| t.name == "C").unwrap();
    assert_eq!(c.dims, vec![Dim { dim: 0, cells: vec![Cell { name: "c".into(), input: String::new(), output: String::new() }] }]);
    assert_eq!(c.maps, vec![Map { name: "C".into(), domain: "C".into() }]);
}

#[test]
fn empty_maps_across_types() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &legacy_example("Empty.ali"))
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
    assert!(e.maps.contains(&Map { name: "f".into(), domain: "C".into() }));
    assert!(e.maps.contains(&Map { name: "g".into(), domain: "D".into() }));
}

#[test]
fn total_composite_map() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &legacy_example("Total.ali"))
        .ok()
        .expect("Total.ali should interpret without errors");
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 10);
    assert_eq!(norm.types_count, 3);
    let graph = norm.modules[0].types.iter().find(|t| t.name == "Graph").unwrap();
    // `let total F :: Arrow` should produce a map F :: Arrow
    assert!(graph.maps.contains(&Map { name: "F".into(), domain: "Arrow".into() }));
    // the mid cell is a diagram (it is explicitly named)
    assert!(graph.diagrams.contains(&Cell { name: "mid".into(), input: "A.t".into(), output: "B.s".into() }));
}

#[test]
fn tutorial_pair_maps() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &legacy_example("Tutorial.ali"))
        .ok()
        .expect("Tutorial.ali should interpret without errors");
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 21);
    assert_eq!(norm.types_count, 4);
    let pair = norm.modules[0].types.iter().find(|t| t.name == "Pair").unwrap();
    // Pair attaches f :: Mor, g :: Mor, and three Ob attachments
    assert!(pair.maps.contains(&Map { name: "f".into(), domain: "Mor".into() }));
    assert!(pair.maps.contains(&Map { name: "g".into(), domain: "Mor".into() }));
    assert!(pair.maps.contains(&Map { name: "x".into(), domain: "Ob".into() }));
    assert!(pair.maps.contains(&Map { name: "y".into(), domain: "Ob".into() }));
    assert!(pair.maps.contains(&Map { name: "z".into(), domain: "Ob".into() }));
}

#[test]
fn theory_function_and_set_maps() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &legacy_example("Theory.ali"))
        .ok()
        .expect("Theory.ali should interpret without errors");
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 26);
    assert_eq!(norm.types_count, 4);
    let module = &norm.modules[0];
    // Function attaches Dom :: Set, Cod :: Set, and two Equation laws
    let function = module.types.iter().find(|t| t.name == "Function").unwrap();
    assert!(function.maps.contains(&Map { name: "Dom".into(),    domain: "Set".into() }));
    assert!(function.maps.contains(&Map { name: "Cod".into(),    domain: "Set".into() }));
    assert!(function.maps.contains(&Map { name: "Id_fun".into(), domain: "Equation".into() }));
    assert!(function.maps.contains(&Map { name: "Fun_id".into(), domain: "Equation".into() }));
    // Set defines an identity Function at the module level
    let set = module.types.iter().find(|t| t.name == "Set").unwrap();
    assert!(set.maps.contains(&Map { name: "Id".into(), domain: "Function".into() }));
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
/// must be rejected.
#[test]
fn embedded_hole_is_error() {
    let result = InterpretedFile::load(&Loader::default(vec![]), &fixture("EmbeddedHole.ali"));
    assert!(!result.is_ok(), "an embedded `?` should fail to load");
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

