use super::loader::{LoadError, LoadFileError, ResolveError};

pub fn report_load_file_error(err: &LoadFileError) {
    match err {
        LoadFileError::Load { path, cause } => match cause {
            LoadError::NotFound => eprintln!("error: could not load `{}`", path),
            LoadError::IoError(reason) => eprintln!("error: could not load `{}`: {}", path, reason),
        },
        LoadFileError::Parse { path, source, errors } => {
            crate::language::report_errors(errors, source, path);
        }
        LoadFileError::Resolve(resolve_err) => report_resolve_error(resolve_err),
    }
}

fn report_resolve_error(err: &ResolveError) {
    match err {
        ResolveError::NotFound { module_name } => {
            eprintln!("error: module file {}.ali not found in search paths", module_name);
        }
        ResolveError::IoError { path, reason } => {
            eprintln!("error: could not load `{}`: {}", path, reason);
        }
        ResolveError::ParseError { path, source, errors } => {
            crate::language::report_errors(errors, source, path);
        }
        ResolveError::Cycle { path } => {
            eprintln!("error: cyclic module dependency involving `{}`", path);
        }
    }
}
