//! The `suture` command (spec appendix B.1):
//!
//! - `suture build <dir>`
//! - `suture run <dir> --scenario <file>`
//! - `suture explain <dir> --scenario <file> <name>`

use std::path::Path;
use std::process::ExitCode;

use suture_lang::ir::Program;

const USAGE: &str =
    "usage:\n  suture build <dir>\n  suture run <dir> --scenario <file>\n  suture explain <dir> --scenario <file> <name>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["build", dir] => build(Path::new(dir)),
        ["run", dir, "--scenario", file] => scenarios(Path::new(dir), Path::new(file), None),
        ["explain", dir, "--scenario", file, name] => scenarios(Path::new(dir), Path::new(file), Some(*name)),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}

fn compile(dir: &Path) -> Result<Program, String> {
    suture_lang::compile_dir(dir).map_err(|errors| errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))
}

/// Compiles, writes the program file, and prints every rule's read-back.
fn build(dir: &Path) -> Result<bool, String> {
    let prog = compile(dir)?;
    let out = dir.join(format!("{}-v{}.sut", prog.module, prog.version));
    let json = serde_json::to_string_pretty(&prog).map_err(|e| e.to_string())?;
    std::fs::write(&out, json).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("{} v{} → {}", prog.module, prog.version, out.display());
    println!(
        "  kinds {} · effects {} · rules {} · goals {}",
        prog.kinds.len(),
        prog.effects.len(),
        prog.rules.len(),
        prog.goals.len()
    );
    for w in &prog.warnings {
        println!("warning: {w}");
    }
    for r in &prog.rules {
        println!("\nrule {}: {}", r.name, r.source);
        println!("  understood as:   {}", r.formula);
        println!("  in other words:  {}", r.paraphrase);
        println!("  cost:            {}", r.tier);
    }
    if !prog.bindings.is_empty() {
        println!("\nchoices:");
        for b in &prog.bindings {
            println!("  {}", suture_lang::readback::choice(&prog, b));
        }
    }
    if !prog.roles.is_empty() || !prog.admits.is_empty() {
        println!("\nroles: {}", suture_lang::readback::roles(&prog));
        println!("admitted:");
        let mut seen = vec![];
        for a in &prog.admits {
            if !seen.contains(&&a.source) {
                println!("  {}", a.source);
                seen.push(&a.source);
            }
        }
        let watches = suture_lang::readback::watches(&prog);
        if !watches.is_empty() {
            println!("watches:");
            for w in watches {
                println!("  {w}");
            }
        }
    }
    Ok(true)
}

/// Runs every scenario in `file` and prints its trace. With `explain`, it
/// also explains that unit at the end of each scenario that has it.
fn scenarios(dir: &Path, file: &Path, explain: Option<&str>) -> Result<bool, String> {
    let prog = compile(dir)?;
    let src = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let list = suture_lang::scenario::parse_scenarios(&file.display().to_string(), &src).map_err(|e| e.to_string())?;
    let mut all_passed = true;
    for sc in &list {
        let report = suture_runtime::scenario::run(&prog, sc);
        println!("scenario \"{}\"", report.title);
        for line in &report.host.trace {
            println!("  {line}");
        }
        if let Some(unit) = explain.and_then(|name| report.host.unit(name)) {
            println!();
            let host = &report.host;
            for line in suture_runtime::explain::explain(&prog, &unit, |k| host.deadline(k).map(|d| d - host.now)) {
                println!("  {line}");
            }
        }
        for f in &report.failures {
            println!("  ✗ {f}");
        }
        println!("  {}\n", if report.passed() { "passed" } else { "FAILED" });
        all_passed &= report.passed();
    }
    Ok(all_passed)
}
