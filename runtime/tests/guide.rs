//! The guide (`docs/guide/`) as a test: every example in it compiles, fails
//! with the errors it shows, or passes as a scenario (spec §7.5).
//!
//! In each chapter, code blocks are read in order:
//! - ```` ```suture ````: a program that starts with `module` replaces the
//!   chapter's program; any other block is added to it. Either way the
//!   result must compile.
//! - ```` ```suture fails ````: the chapter's program with this block added
//!   (or this program, if it starts with `module`) must not compile. If the
//!   next block is ```` ```errors ````, the errors must be exactly its text.
//! - ```` ```scenario ````: every scenario in it passes on the chapter's program.
//!
//! Other blocks (shell commands, output) are not checked.

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::run;

struct Block {
    info: String,
    text: String,
    line: usize,
}

fn blocks(md: &str) -> Vec<Block> {
    let mut out = vec![];
    let mut open: Option<Block> = None;
    for (i, l) in md.lines().enumerate() {
        match (&mut open, l.strip_prefix("```")) {
            (None, Some(info)) => open = Some(Block { info: info.trim().to_string(), text: String::new(), line: i + 1 }),
            (Some(_), Some("")) => out.push(open.take().expect("open")),
            (Some(b), _) => {
                b.text.push_str(l);
                b.text.push('\n');
            }
            (None, None) => {}
        }
    }
    out
}

/// Whether a block is a whole program: its first line that is not a
/// comment is the `module` line.
fn whole(text: &str) -> bool {
    text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("--")).is_some_and(|l| l.starts_with("module"))
}

fn compile(src: &str) -> Result<suture_lang::ir::Program, Vec<String>> {
    suture_lang::compile(&[("main.st".to_string(), src.to_string())]).map_err(|e| e.iter().map(|e| e.to_string()).collect())
}

fn check(name: &str, md: &str) -> Vec<String> {
    let mut problems = vec![];
    let mut program = String::new();
    let all = blocks(md);
    let mut k = 0;
    while k < all.len() {
        let b = &all[k];
        let at = format!("{name}:{}", b.line);
        let with = |text: &str| if whole(text) { text.to_string() } else { format!("{program}{text}") };
        match b.info.as_str() {
            "suture" => {
                let src = with(&b.text);
                match compile(&src) {
                    Ok(_) => program = src,
                    Err(e) => problems.push(format!("{at}: does not compile:\n{}", e.join("\n"))),
                }
            }
            "suture fails" => match compile(&with(&b.text)) {
                Ok(_) => problems.push(format!("{at}: compiles, but the guide says it fails")),
                Err(e) => {
                    if let Some(want) = all.get(k + 1).filter(|n| n.info == "errors") {
                        let got = e.join("\n") + "\n";
                        if got != want.text {
                            problems.push(format!("{at}: errors differ\n--- the guide\n{}--- the compiler\n{got}", want.text));
                        }
                        k += 1;
                    }
                }
            },
            "scenario" => match (compile(&program), parse_scenarios(&at, &b.text)) {
                (Ok(p), Ok(list)) => {
                    for sc in list {
                        let r = run(&p, &sc);
                        if !r.passed() {
                            problems.push(format!("{at}: scenario \"{}\" fails: {:?}", r.title, r.failures));
                        }
                    }
                }
                (Err(_), _) => problems.push(format!("{at}: a scenario with no program before it")),
                (_, Err(e)) => problems.push(format!("{at}: {e}")),
            },
            _ => {}
        }
        k += 1;
    }
    problems
}

#[test]
fn every_example_in_the_guide_holds() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/guide");
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "md")).collect();
    files.sort();
    let mut problems = vec![];
    let mut checked = 0;
    for f in &files {
        let md = std::fs::read_to_string(f).unwrap();
        checked += blocks(&md).iter().filter(|b| b.info.starts_with("suture") || b.info == "scenario").count();
        problems.extend(check(&f.file_name().unwrap().to_string_lossy(), &md));
    }
    assert!(checked > 0, "no examples found in {dir}");
    assert!(problems.is_empty(), "{}", problems.join("\n\n"));
}

#[test]
fn the_tickets_example_passes() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/tickets");
    let p = suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"));
    let src = std::fs::read_to_string(format!("{dir}/tickets.scenario")).unwrap();
    let list = parse_scenarios("tickets.scenario", &src).unwrap();
    assert_eq!(list.len(), 5);
    for sc in list {
        let r = run(&p, &sc);
        assert!(r.passed(), "{}: {:?}", r.title, r.failures);
    }
}

#[test]
fn the_credit_example_passes() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/credit");
    let p = suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"));
    let src = std::fs::read_to_string(format!("{dir}/credit.scenario")).unwrap();
    let list = parse_scenarios("credit.scenario", &src).unwrap();
    assert_eq!(list.len(), 5);
    for sc in list {
        let r = run(&p, &sc);
        assert!(r.passed(), "{}: {:?}", r.title, r.failures);
    }
}

#[test]
fn the_returns_example_passes() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/returns");
    let p = suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"));
    let src = std::fs::read_to_string(format!("{dir}/returns.scenario")).unwrap();
    let list = parse_scenarios("returns.scenario", &src).unwrap();
    assert_eq!(list.len(), 2);
    for sc in list {
        let r = run(&p, &sc);
        assert!(r.passed(), "{}: {:?}", r.title, r.failures);
    }
    // models.json is host configuration: it must load, and it names a model the program decides with.
    let text = std::fs::read_to_string(format!("{dir}/models.json")).unwrap();
    let names: Vec<String> = suture_runtime::backend::load(&text, || Box::new(suture_runtime::backend::Curl)).unwrap().into_iter().map(|(n, _)| n).collect();
    assert!(names.iter().all(|n| p.decisions.iter().any(|d| d.model.as_ref() == Some(n))), "{names:?}");
}

#[test]
fn the_claims_evaluation_passes() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../eval/claims/suture");
    let p = suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"));
    let file = format!("{dir}/claims.scenario");
    let list = parse_scenarios(&file, &std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(list.len(), 13);
    for sc in list {
        let r = run(&p, &sc);
        assert!(r.passed(), "{}: {:?}", r.title, r.failures);
    }
}

#[test]
fn the_returns_evaluation_passes() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../eval/returns/suture");
    let p = suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"));
    let file = format!("{dir}/returns.scenario");
    let list = parse_scenarios(&file, &std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(list.len(), 22);
    for sc in list {
        let r = run(&p, &sc);
        assert!(r.passed(), "{}: {:?}", r.title, r.failures);
    }
}

#[test]
fn the_leave_evaluation_passes() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../eval/leave/suture");
    let p = suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"));
    let file = format!("{dir}/leave.scenario");
    let list = parse_scenarios(&file, &std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(list.len(), 15);
    for sc in list {
        let r = run(&p, &sc);
        assert!(r.passed(), "{}: {:?}", r.title, r.failures);
    }
}
