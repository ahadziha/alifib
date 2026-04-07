use std::path::PathBuf;

use alifib::aux::loader::Loader;
use alifib::output::{Dim, InterpretedFile, Module, Store, Type};

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

    assert_eq!(norm, Store {
        cells_count: 12,
        types_count: 5,
        modules: vec![Module {
            path: fixture("Magma.ali"),
            types: vec![
                Type {
                    name: String::new(),
                    dims: vec![],
                    diagrams: vec![],
                    maps: vec![],
                },
                Type {
                    name: "Comagma".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec!["Ob.pt".into()] },
                        Dim { dim: 1, cells: vec!["Ob.ob : Ob.pt -> Ob.pt".into()] },
                        Dim { dim: 2, cells: vec!["c : Ob.ob -> Ob.ob Ob.ob".into()] },
                    ],
                    diagrams: vec!["c : Ob.ob -> Ob.ob Ob.ob".into()],
                    maps: vec!["Comagma :: Comagma".into(), "Ob :: Ob".into()],
                },
                Type {
                    name: "FrobeniusMagma".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec!["Ob.pt".into()] },
                        Dim { dim: 1, cells: vec!["Ob.ob : Ob.pt -> Ob.pt".into()] },
                        Dim { dim: 2, cells: vec![
                            "Comagma.c : Ob.ob -> Ob.ob Ob.ob".into(),
                            "Magma.m : Ob.ob Ob.ob -> Ob.ob".into(),
                        ]},
                    ],
                    diagrams: vec![],
                    maps: vec![
                        "Comagma :: Comagma".into(),
                        "FrobeniusMagma :: FrobeniusMagma".into(),
                        "Magma :: Magma".into(),
                        "Ob :: Ob".into(),
                    ],
                },
                Type {
                    name: "Magma".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec!["Ob.pt".into()] },
                        Dim { dim: 1, cells: vec!["Ob.ob : Ob.pt -> Ob.pt".into()] },
                        Dim { dim: 2, cells: vec!["m : Ob.ob Ob.ob -> Ob.ob".into()] },
                    ],
                    diagrams: vec!["m : Ob.ob Ob.ob -> Ob.ob".into()],
                    maps: vec!["Magma :: Magma".into(), "Ob :: Ob".into()],
                },
                Type {
                    name: "Ob".into(),
                    dims: vec![
                        Dim { dim: 0, cells: vec!["pt".into()] },
                        Dim { dim: 1, cells: vec!["ob : pt -> pt".into()] },
                    ],
                    diagrams: vec!["ob : pt -> pt".into(), "pt".into()],
                    maps: vec!["Ob :: Ob".into()],
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
    assert_eq!(c.dims, vec![Dim { dim: 0, cells: vec!["c".into()] }]);
    assert_eq!(c.maps, vec!["C :: C".to_string()]);
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
    assert_eq!(d.dims, vec![Dim { dim: 0, cells: vec!["c".into()] }]);
    // E has a module-level let g :: D = f, exposed as a map alongside f :: C
    let e = module.types.iter().find(|t| t.name == "E").unwrap();
    assert!(e.maps.contains(&"f :: C".to_string()));
    assert!(e.maps.contains(&"g :: D".to_string()));
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
    assert!(graph.maps.contains(&"F :: Arrow".to_string()));
    // the mid cell is a diagram (it is explicitly named)
    assert!(graph.diagrams.contains(&"mid : A.t -> B.s".to_string()));
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
    assert!(pair.maps.contains(&"f :: Mor".to_string()));
    assert!(pair.maps.contains(&"g :: Mor".to_string()));
    assert!(pair.maps.contains(&"x :: Ob".to_string()));
    assert!(pair.maps.contains(&"y :: Ob".to_string()));
    assert!(pair.maps.contains(&"z :: Ob".to_string()));
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
    assert!(function.maps.contains(&"Dom :: Set".to_string()));
    assert!(function.maps.contains(&"Cod :: Set".to_string()));
    assert!(function.maps.contains(&"Id_fun :: Equation".to_string()));
    assert!(function.maps.contains(&"Fun_id :: Equation".to_string()));
    // Set defines an identity Function at the module level
    let set = module.types.iter().find(|t| t.name == "Set").unwrap();
    assert!(set.maps.contains(&"Id :: Function".to_string()));
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
