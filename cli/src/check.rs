//! `suture check`: the loop a coding agent runs after each edit (appendix AO).
//! It compiles the module, runs every scenario file beside it, and shows the
//! trace of a scenario only when it fails, so the output stays short.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// What a check found, in the order it found it.
#[derive(Default)]
pub struct Found {
    pub errors: Vec<Value>,
    pub warnings: Vec<String>,
    pub scenarios: Vec<Value>,
}

impl Found {
    fn error(&mut self, file: &str, line: usize, message: &str) {
        self.errors.push(json!({ "file": file, "line": line, "message": message }));
    }

    pub fn passed(&self) -> bool {
        self.errors.is_empty() && self.scenarios.iter().all(|s| s["passed"] == true)
    }

    pub fn json(&self) -> Value {
        json!({ "passed": self.passed(), "errors": self.errors, "warnings": self.warnings, "scenarios": self.scenarios })
    }

    /// The short form: an error per line, a line per passing scenario, and the
    /// trace and failures of each failing one.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for e in &self.errors {
            let at = match e["line"].as_u64() {
                Some(0) | None => e["file"].as_str().unwrap_or("").to_string(),
                Some(l) => format!("{}:{l}", e["file"].as_str().unwrap_or("")),
            };
            out += &format!("error: {at}: {}\n", e["message"].as_str().unwrap_or(""));
        }
        for w in &self.warnings {
            out += &format!("warning: {w}\n");
        }
        for s in &self.scenarios {
            let name = format!("{}: \"{}\"", s["file"].as_str().unwrap_or(""), s["title"].as_str().unwrap_or(""));
            if s["passed"] == true {
                out += &format!("✓ {name}\n");
                continue;
            }
            out += &format!("✗ {name}\n");
            for l in s["trace"].as_array().into_iter().flatten() {
                out += &format!("    {}\n", l.as_str().unwrap_or(""));
            }
            for f in s["failures"].as_array().into_iter().flatten() {
                out += &format!("  ✗ {}\n", f.as_str().unwrap_or(""));
            }
        }
        let failed = self.scenarios.iter().filter(|s| s["passed"] != true).count();
        out += &match (self.errors.len(), self.scenarios.len()) {
            (0, 0) => "compiles; no scenarios beside it: write a ⟨name⟩.scenario (`suture help scenarios`)\n".to_string(),
            (0, n) => format!("{n} scenario{}: {} passed, {failed} failed\n", plural(n), n - failed),
            (e, _) => format!("{e} error{}; fix the first and check again (`suture help` lists the forms)\n", plural(e)),
        };
        out
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Compiles the module in `dir` and runs the scenarios of every `.scenario`
/// file in it, sorted by name.
pub fn check(dir: &Path) -> Found {
    let mut found = Found::default();
    let prog = match suture_lang::compile_dir(dir) {
        Ok(p) => p,
        Err(errors) => {
            for e in errors {
                found.error(&e.file, e.line, &e.msg);
            }
            return found;
        }
    };
    found.warnings = prog.warnings.iter().map(|w| w.to_string()).collect();
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "scenario")).collect();
    files.sort();
    for path in files {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                found.error(&name, 0, &e.to_string());
                continue;
            }
        };
        let list = match suture_lang::scenario::parse_scenarios(&name, &src) {
            Ok(l) => l,
            Err(e) => {
                found.error(&e.file, e.line, &e.msg);
                continue;
            }
        };
        for sc in &list {
            let r = suture_runtime::scenario::run(&prog, sc);
            let trace: Vec<&String> = if r.passed() { vec![] } else { r.host.trace.iter().collect() };
            found.scenarios.push(json!({ "file": name, "title": r.title, "passed": r.passed(), "failures": r.failures, "trace": trace }));
        }
    }
    found
}
