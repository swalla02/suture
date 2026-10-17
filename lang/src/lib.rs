//! The Suture language, prototype slice 1 (spec appendix B): statements,
//! controlled-English sentences, checks, read-backs and compiled programs.

pub mod ast;
pub mod check;
pub mod ir;
pub mod lex;
pub mod parse;
pub mod readback;
pub mod scenario;
pub mod sentence;
pub mod suggest;

pub use lex::{statements, tokens, Cursor, Error, Stmt, Tok};
pub use parse::parse_file;

/// Compiles a module from its files, given as (name, text) pairs.
pub fn compile(files: &[(String, String)]) -> Result<ir::Program, Vec<Error>> {
    let mut src = ast::Source::default();
    let mut errors = vec![];
    let mut failed = vec![];
    for (name, text) in files {
        match parse::parse_all(name, text, &mut src) {
            Ok(f) => failed.extend(f),
            Err(e) => errors.push(e),
        }
    }
    // Suggestions are made against everything that did parse (appendix H.2.5).
    errors.extend(failed.into_iter().map(|(st, e)| suggest::improve(&src, &st, e)));
    if !errors.is_empty() {
        return Err(errors);
    }
    check::check(&src).map_err(|es| es.into_iter().map(|e| suggest::names(&src, e)).collect())
}

/// Compiles every `.st` file in `dir`, in file-name order.
pub fn compile_dir(dir: &std::path::Path) -> Result<ir::Program, Vec<Error>> {
    let fail = |file: String, msg: String| vec![Error { file, line: 0, msg }];
    let entries = std::fs::read_dir(dir).map_err(|e| fail(dir.display().to_string(), e.to_string()))?;
    let mut files = vec![];
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|x| x == "st") {
            let text = std::fs::read_to_string(&path).map_err(|e| fail(path.display().to_string(), e.to_string()))?;
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            files.push((name, text));
        }
    }
    if files.is_empty() {
        return Err(fail(dir.display().to_string(), "no .st files here".into()));
    }
    files.sort();
    compile(&files)
}
