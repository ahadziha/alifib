//! The web backend must let user sources `include Theory`, `include Semigroup`,
//! etc. without any file on disk — the bundled examples stand in for the
//! filesystem-based module search that the CLI uses.

use alifib::interactive::web::WebRepl;

#[test]
fn include_bundled_module_resolves() {
    let mut repl = WebRepl::new();
    let src = "@Type\ninclude Theory,\nlet S = Theory.Set";
    let out = repl.load_source_with_modules(src, alifib_web_shared::virtual_module_files());
    assert!(
        out.contains("\"status\":\"ok\""),
        "load_source failed for `include Theory`: {}",
        out,
    );
}
