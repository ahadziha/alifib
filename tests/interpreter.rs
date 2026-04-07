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

#[test]
fn magma_interpretation() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &fixture("Magma.ali"))
        .ok()
        .expect("Magma.ali should interpret without errors");

    assert!(!file.has_holes());

    let norm = file.state.normalize();

    let pt   = || Cell { name: "pt".into(),    src: vec![],           tgt: vec![]           };
    let ob   = || Cell { name: "ob".into(),    src: vec!["pt".into()], tgt: vec!["pt".into()] };
    let obpt = || Cell { name: "Ob.pt".into(), src: vec![],           tgt: vec![]           };
    let obob = || Cell { name: "Ob.ob".into(), src: vec!["Ob.pt".into()], tgt: vec!["Ob.pt".into()] };

    assert_eq!(norm, Store {
        cells_count: 12,
        types_count: 5,
        modules: vec![Module {
            path: fixture("Magma.ali"),
            types: vec![
                Type { name: String::new(), dims: vec![], diagrams: vec![], maps: vec![] },
                Type {
                    name: "Comagma".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec![obpt()] },
                        Dim { dim: 1, cells: vec![obob()] },
                        Dim { dim: 2, cells: vec![
                            Cell { name: "c".into(), src: vec!["Ob.ob".into()], tgt: vec!["Ob.ob".into(), "Ob.ob".into()] },
                        ]},
                    ],
                    diagrams: vec![
                        Cell { name: "c".into(), src: vec!["Ob.ob".into()], tgt: vec!["Ob.ob".into(), "Ob.ob".into()] },
                    ],
                    maps: vec![Map { name: "Comagma".into(), domain: "Comagma".into() }, Map { name: "Ob".into(), domain: "Ob".into() }],
                },
                Type {
                    name: "FrobeniusMagma".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec![obpt()] },
                        Dim { dim: 1, cells: vec![obob()] },
                        Dim { dim: 2, cells: vec![
                            Cell { name: "Comagma.c".into(), src: vec!["Ob.ob".into()], tgt: vec!["Ob.ob".into(), "Ob.ob".into()] },
                            Cell { name: "Magma.m".into(),   src: vec!["Ob.ob".into(), "Ob.ob".into()], tgt: vec!["Ob.ob".into()] },
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
                Type {
                    name: "Magma".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec![obpt()] },
                        Dim { dim: 1, cells: vec![obob()] },
                        Dim { dim: 2, cells: vec![
                            Cell { name: "m".into(), src: vec!["Ob.ob".into(), "Ob.ob".into()], tgt: vec!["Ob.ob".into()] },
                        ]},
                    ],
                    diagrams: vec![
                        Cell { name: "m".into(), src: vec!["Ob.ob".into(), "Ob.ob".into()], tgt: vec!["Ob.ob".into()] },
                    ],
                    maps: vec![Map { name: "Magma".into(), domain: "Magma".into() }, Map { name: "Ob".into(), domain: "Ob".into() }],
                },
                Type {
                    name: "Ob".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec![pt()] },
                        Dim { dim: 1, cells: vec![ob()] },
                    ],
                    diagrams: vec![ob(), pt()],
                    maps: vec![Map { name: "Ob".into(), domain: "Ob".into() }],
                },
            ],
        }],
    });
}

#[test]
fn empty2_single_type_with_one_cell() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &example("Empty2.ali"))
        .ok()
        .expect("Empty2.ali should interpret without errors");
    assert!(!file.has_holes());
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 1);
    assert_eq!(norm.types_count, 2);
    let c = norm.modules[0].types.iter().find(|t| t.name == "C").unwrap();
    assert_eq!(c.dims, vec![Dim { dim: 0, cells: vec![Cell { name: "c".into(), src: vec![], tgt: vec![] }] }]);
    assert_eq!(c.maps, vec![Map { name: "C".into(), domain: "C".into() }]);
}

#[test]
fn empty_maps_across_types() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &example("Empty.ali"))
        .ok()
        .expect("Empty.ali should interpret without errors");
    assert!(!file.has_holes());
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 2);
    assert_eq!(norm.types_count, 4);
    let module = &norm.modules[0];
    // D reuses C's generators
    let d = module.types.iter().find(|t| t.name == "D").unwrap();
    assert_eq!(d.dims, vec![Dim { dim: 0, cells: vec![Cell { name: "c".into(), src: vec![], tgt: vec![] }] }]);
    // E has a module-level let g :: D = f, exposed as a map alongside f :: C
    let e = module.types.iter().find(|t| t.name == "E").unwrap();
    assert!(e.maps.contains(&Map { name: "f".into(), domain: "C".into() }));
    assert!(e.maps.contains(&Map { name: "g".into(), domain: "D".into() }));
}

#[test]
fn total_composite_map() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &example("Total.ali"))
        .ok()
        .expect("Total.ali should interpret without errors");
    assert!(!file.has_holes());
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 10);
    assert_eq!(norm.types_count, 3);
    let graph = norm.modules[0].types.iter().find(|t| t.name == "Graph").unwrap();
    // `let total F :: Arrow` should produce a map F :: Arrow
    assert!(graph.maps.contains(&Map { name: "F".into(), domain: "Arrow".into() }));
    // the mid cell is a diagram (it is explicitly named)
    assert!(graph.diagrams.contains(&Cell { name: "mid".into(), src: vec!["A.t".into()], tgt: vec!["B.s".into()] }));
}

#[test]
fn tutorial_pair_maps() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &example("Tutorial.ali"))
        .ok()
        .expect("Tutorial.ali should interpret without errors");
    assert!(!file.has_holes());
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
    let file = InterpretedFile::load(&Loader::default(vec![]), &example("Theory.ali"))
        .ok()
        .expect("Theory.ali should interpret without errors");
    assert!(!file.has_holes());
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

#[test]
fn hole_loads_with_holes() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &example("Hole.ali"))
        .ok()
        .expect("Hole.ali should interpret without errors");
    assert!(file.has_holes());
    let norm = file.state.normalize();
    assert_eq!(norm.cells_count, 78);
    assert_eq!(norm.types_count, 8);
}
