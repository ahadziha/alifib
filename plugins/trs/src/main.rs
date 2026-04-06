use alifib::interpreter::Context;
use trs_rs::{generate::generate_program, parse::parse_ari};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let print_ali = args.iter().any(|a| a == "--print-ali");
    let positional: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(String::as_str)
        .collect();

    if positional.is_empty() {
        eprintln!("Usage: ari2ali-rs [--print-ali] <input.ari>");
        std::process::exit(1);
    }

    let input_path = positional[0];
    let input = std::fs::read_to_string(input_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        std::process::exit(1);
    });

    let basename = input_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(input_path);
    let stem = basename.trim_end_matches(".ari");
    let mut module_name: String = stem
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if module_name.starts_with(|c: char| c.is_numeric()) {
        module_name = format!("TRS_{}", module_name);
    }

    let trs = parse_ari(&input);
    eprintln!(
        "Parsed: {} function symbols, {} rules",
        trs.funs.len(),
        trs.rules.len()
    );
    for f in &trs.funs {
        eprintln!("  {}/{}", f.name, f.arity);
    }

    let program = generate_program(&trs, &module_name);

    if print_ali {
        print!("{}", program.print_ali());
        return;
    }

    let result = program.interpret(Context::new_empty(module_name));

    if !result.errors.is_empty() {
        for e in &result.errors {
            eprintln!("error: {}", e);
        }
        std::process::exit(1);
    }

    println!("{}", result.context.state);
}
