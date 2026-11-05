//! Build the static deployment's examples with the server's scanner and index.

use std::path::Path;
use std::process::ExitCode;

use alifib_cli::examples::mirror;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [src, dest] = args.as_slice() else {
        eprintln!("usage: static_examples <src> <dest>");
        return ExitCode::FAILURE;
    };
    match mirror(Path::new(src), Path::new(dest)) {
        Ok(scan) => {
            for skipped in &scan.skipped {
                eprintln!("static_examples: {skipped}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("static_examples: {error}");
            ExitCode::FAILURE
        }
    }
}
