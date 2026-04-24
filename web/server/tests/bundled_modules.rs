//! The web backend must let user sources `include Theory`, `include Semigroup`,
//! etc. via the modules map the frontend supplies — standing in for the
//! filesystem-based module search that the CLI uses.

use alifib::interactive::web::WebRepl;
use std::collections::HashMap;

#[test]
fn include_user_supplied_module_resolves() {
    let mut modules = HashMap::new();
    // Minimal Theory-like module sufficient for the include check.
    modules.insert(
        "Theory.ali".to_string(),
        "@Type\nSet <<= { pt, id: pt -> pt }".to_string(),
    );

    let mut repl = WebRepl::new();
    let src = "@Type\ninclude Theory,\nlet S = Theory.Set";
    let out = repl.load_source_with_modules(src, modules);
    assert!(
        out.contains("\"status\":\"ok\""),
        "load_source failed for `include Theory`: {}",
        out,
    );
}
