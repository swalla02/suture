//! The Suture language, prototype slice 1 (spec appendix B): statements,
//! controlled-English sentences, checks, read-backs and compiled programs.

pub mod ast;
pub mod check;
pub mod ir;
pub mod lex;
pub mod parse;
pub mod readback;
pub mod sentence;

pub use lex::{statements, tokens, Cursor, Error, Stmt, Tok};
pub use parse::parse_file;

/// Compiles a module from its files, given as (name, text) pairs.
pub fn compile(files: &[(String, String)]) -> Result<ir::Program, Vec<Error>> {
    let mut src = ast::Source::default();
    let errors: Vec<Error> = files.iter().filter_map(|(name, text)| parse::parse_file(name, text, &mut src).err()).collect();
    if !errors.is_empty() {
        return Err(errors);
    }
    check::check(&src)
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
