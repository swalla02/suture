//! `cargo run -p rules-airline -- ⟨command⟩`:
//!
//! - `split`, `sample`: print the dev cases, and the test cases asked three
//!   times, for `split.txt` and `sample.txt`;
//! - `ask ⟨extract|p1|p3⟩ ⟨dev|test|sample⟩ ⟨run⟩`: asks the models for each
//!   case not yet recorded in that run (`models::runs_dir`), and stops once
//!   the recorded answers cost more than `RULES_BUDGET` dollars (default 2);
//! - `score ⟨dev|test⟩`: every setup's answer to every case, in
//!   `results/⟨split⟩.csv`, and the summary on standard output.

use rules_airline::data::{Case, Facts, Tables};
use rules_airline::{data, data_dir, extract, models, prompts, reference, split, stats, sutured};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let r = match a[..] {
        ["split"] => print_ids("dev cases: keyed SipHash of the case id is 0 mod 5 (src/split.rs); every other case is test", split::is_dev),
        ["sample"] => print_ids("test cases asked three times: SipHash under a second key is 0 mod 4 (src/split.rs)", split::in_sample),
        ["ask", setup, which, run] => run.parse().map_err(|e| format!("run: {e}")).and_then(|run| ask(setup, which, run)),
        ["score", which] => score(which),
        _ => Err("usage: rules-airline split | sample | ask ⟨extract|p1|p3⟩ ⟨dev|test|sample⟩ ⟨run⟩ | score ⟨dev|test⟩".into()),
    };
    if let Err(e) = r {
        eprintln!("{e}");
        std::process::exit(2);
    }
}

fn print_ids(what: &str, keep: fn(&str) -> bool) -> Result<(), String> {
    println!("# {what}");
    for c in data::cases(&data_dir())? {
        if keep(&c.id) {
            println!("{}", c.id);
        }
    }
    Ok(())
}

fn chosen(which: &str) -> Result<Vec<Case>, String> {
    let keep: fn(&str) -> bool = match which {
        "dev" => split::is_dev,
        "test" => |id| !split::is_dev(id),
        "sample" => split::in_sample,
        w => return Err(format!("`{w}` is not dev, test or sample")),
    };
    Ok(data::cases(&data_dir())?.into_iter().filter(|c| keep(&c.id)).collect())
}

fn answer_path(setup: &str, run: u32, id: &str, part: &str) -> PathBuf {
    models::runs_dir().join(format!("{setup}/{run}/{id}{part}.json"))
}

/// The dollars every recorded answer cost.
fn spent() -> f64 {
    fn walk(p: &Path, sum: &mut f64) {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, sum);
            } else if p.extension().is_some_and(|x| x == "json")
                && let Ok(v) = std::fs::read_to_string(&p).map(|t| serde_json::from_str::<Value>(&t).unwrap_or_default())
            {
                *sum += v["cost"].as_f64().unwrap_or(0.0);
            }
        }
    }
    let mut sum = 0.0;
    for s in ["extract", "p1", "p3"] {
        walk(&models::runs_dir().join(s), &mut sum);
    }
    sum
}

fn ask(setup: &str, which: &str, run: u32) -> Result<(), String> {
    let cases = chosen(which)?;
    let budget: f64 = std::env::var("RULES_BUDGET").ok().and_then(|b| b.parse().ok()).unwrap_or(2.0);
    let rules = std::fs::read_to_string(data_dir().join("reference_rules.txt")).map_err(|e| e.to_string())?;
    let queue = Mutex::new(cases.iter());
    let failed = Mutex::new(vec![]);
    let over = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|s| {
        for _ in 0..6 {
            s.spawn(|| {
                loop {
                    let Some(c) = queue.lock().unwrap().next() else { return };
                    if spent() > budget {
                        over.store(true, std::sync::atomic::Ordering::SeqCst);
                        return;
                    }
                    let r = match setup {
                        "extract" => extract_one(c, run).map(|_| ()),
                        "p1" => models::recorded(&answer_path("p1", run, &c.id, ""), || models::chat(prompts::SYSTEM, &prompts::p1(&rules, &c.text))).map(|_| ()),
                        "p3" => p3_one(c, run, &rules).map(|_| ()),
                        s => Err(format!("`{s}` is not extract, p1 or p3")),
                    };
                    if let Err(e) = r {
                        failed.lock().unwrap().push(format!("{}: {e}", c.id));
                    }
                }
            });
        }
    });
    let failed = failed.into_inner().unwrap();
    for f in &failed {
        eprintln!("forfeit: {f}");
    }
    eprintln!("{setup} {which} run {run}: {} cases, {} forfeits; ${:.4} recorded in all", cases.len(), failed.len(), spent());
    if over.into_inner() {
        return Err(format!("stopped: the recorded answers cost more than ${budget}"));
    }
    Ok(())
}

fn extract_one(c: &Case, run: u32) -> Result<Facts, String> {
    let state = serde_json::json!({"case": c.text});
    let trip = models::recorded(&answer_path("extract", run, &c.id, ".trip"), || models::decide(&state, &extract::trip_questions(&c.text)))?;
    let count = extract::count(&trip["picks"])?;
    let items = if count >= 2 {
        models::recorded(&answer_path("extract", run, &c.id, ".items"), || models::decide(&state, &extract::item_questions(&c.text, count)))?
    } else {
        serde_json::json!({"picks": {}})
    };
    extract::facts(&trip["picks"], &items["picks"])
}

/// The recorded extraction, without asking: none when the trip was never
/// read, an error when the rest is missing or makes no facts.
fn extracted(c: &Case, run: u32) -> Option<Result<Facts, String>> {
    let trip = read_json(&answer_path("extract", run, &c.id, ".trip"))?;
    let items = || -> Result<Facts, String> {
        let count = extract::count(&trip["picks"])?;
        let items = if count >= 2 { read_json(&answer_path("extract", run, &c.id, ".items")).ok_or("forfeit: no items recorded")? } else { serde_json::json!({"picks": {}}) };
        extract::facts(&trip["picks"], &items["picks"])
    };
    Some(items())
}

fn p3_one(c: &Case, run: u32, rules: &str) -> Result<Value, String> {
    let v = models::recorded(&answer_path("p3", run, &c.id, ""), || models::chat(prompts::SYSTEM, &prompts::p3(rules, &c.text)))?;
    models::recorded(&answer_path("p3", run, &c.id, ".out"), || {
        let Some(program) = prompts::p3_program(v["text"].as_str().unwrap_or("")) else { return Ok(serde_json::json!({"error": "no program"})) };
        let scratch = models::runs_dir().join(format!("tmp/p3-{}-{}-{}", std::process::id(), c.id, run));
        let out = prompts::sandboxed(&program, &scratch);
        let _ = std::fs::remove_dir_all(&scratch);
        Ok(match out {
            Ok(stdout) => serde_json::json!({"stdout": stdout}),
            Err(e) => serde_json::json!({"error": e}),
        })
    })
}

/// One setup's answer to one case in one run.
#[derive(Clone, Debug)]
struct Row {
    id: String,
    setup: String,
    run: u32,
    answer: Option<i64>,
    right: bool,
    cost: f64,
    seconds: f64,
    note: String,
}

fn read_json(p: &Path) -> Option<Value> {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok())
}

fn score(which: &str) -> Result<(), String> {
    let cases = chosen(which)?;
    let t = Tables::load(&data_dir())?;
    let charges = std::fs::read_to_string(sutured::module_dir().join("charges.csv")).map_err(|e| e.to_string())?;
    let prog = sutured::compile(&sutured::module_dir())?;
    let mut s1 = sutured::Run::new(prog, &t, &charges)?;
    let mut rows = vec![];
    // Every case is asked once; dev cases and the test sample three times.
    // An answer expected and not recorded counts as a forfeit, never as a
    // case left out.
    let expected = |run: u32, id: &str| run == 1 || which == "dev" || split::in_sample(id);
    const FORFEIT: &str = "forfeit: no answer recorded";
    for c in &cases {
        let truth = reference::total(&c.facts, &t).ok_or(format!("{}: no reference answer", c.id))?;
        let mut push = |setup: &str, run: u32, answer: Result<i64, String>, cost: f64, seconds: f64| {
            let right = answer.as_ref().is_ok_and(|a| *a == truth);
            let note = answer.as_ref().err().cloned().unwrap_or_default();
            rows.push(Row { id: c.id.clone(), setup: setup.into(), run, answer: answer.ok(), right, cost, seconds, note });
        };
        push("R true", 1, reference::total(&c.facts, &t).ok_or("no fee row".into()), 0.0, 0.0);
        let start = std::time::Instant::now();
        let a = s1.total(&format!("{}t", c.id), &c.facts);
        push("S1 true", 1, a, 0.0, start.elapsed().as_secs_f64());
        for run in (1..=3).filter(|r| expected(*r, &c.id)) {
            if let Some(f) = extracted(c, run) {
                let trip = read_json(&answer_path("extract", run, &c.id, ".trip")).unwrap_or_default();
                let items = read_json(&answer_path("extract", run, &c.id, ".items")).unwrap_or_default();
                let cost = trip["cost"].as_f64().unwrap_or(0.0) + items["cost"].as_f64().unwrap_or(0.0);
                let secs = trip["seconds"].as_f64().unwrap_or(0.0) + items["seconds"].as_f64().unwrap_or(0.0);
                match f {
                    Ok(f) => {
                        push("R extracted", run, reference::total(&f, &t).ok_or("no fee row".into()), cost, secs);
                        let start = std::time::Instant::now();
                        let a = s1.total(&format!("{}x{run}", c.id), &f);
                        push("S1 extracted", run, a, cost, secs + start.elapsed().as_secs_f64());
                    }
                    Err(e) => {
                        push("R extracted", run, Err(e.clone()), cost, secs);
                        push("S1 extracted", run, Err(e), cost, secs);
                    }
                }
            } else {
                push("R extracted", run, Err(FORFEIT.into()), 0.0, 0.0);
                push("S1 extracted", run, Err(FORFEIT.into()), 0.0, 0.0);
            }
            if let Some(v) = read_json(&answer_path("p1", run, &c.id, "")) {
                let a = prompts::p1_answer(v["text"].as_str().unwrap_or("")).ok_or("no total in the answer".to_string());
                push("P1", run, a, v["cost"].as_f64().unwrap_or(0.0), v["seconds"].as_f64().unwrap_or(0.0));
            } else {
                push("P1", run, Err(FORFEIT.into()), 0.0, 0.0);
            }
            if let (Some(v), Some(out)) = (read_json(&answer_path("p3", run, &c.id, "")), read_json(&answer_path("p3", run, &c.id, ".out"))) {
                let a = match out["stdout"].as_str() {
                    Some(s) => prompts::p3_answer(s).ok_or("no number printed".to_string()),
                    None => Err(out["error"].as_str().unwrap_or("?").to_string()),
                };
                push("P3", run, a, v["cost"].as_f64().unwrap_or(0.0), v["seconds"].as_f64().unwrap_or(0.0) + out["seconds"].as_f64().unwrap_or(0.0));
            } else {
                push("P3", run, Err(FORFEIT.into()), 0.0, 0.0);
            }
        }
    }
    write_rows(which, &rows)?;
    summary(&cases, &rows);
    Ok(())
}

fn write_rows(which: &str, rows: &[Row]) -> Result<(), String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../results");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut out = String::from("case,setup,run,answer,right,cost,seconds,note\n");
    for r in rows {
        let note = r.note.replace([',', '\n', '"'], " ");
        out.push_str(&format!("{},{},{},{},{},{:.6},{:.2},{}\n", r.id, r.setup, r.run, r.answer.map(|a| a.to_string()).unwrap_or_default(), r.right, r.cost, r.seconds, note.chars().take(120).collect::<String>()));
    }
    std::fs::write(dir.join(format!("{which}.csv")), out).map_err(|e| e.to_string())
}

const SETUPS: [&str; 6] = ["R true", "S1 true", "R extracted", "S1 extracted", "P1", "P3"];

fn summary(cases: &[Case], rows: &[Row]) {
    let mut by: BTreeMap<(&str, u32), BTreeMap<&str, &Row>> = BTreeMap::new();
    for r in rows {
        by.entry((r.setup.as_str(), r.run)).or_default().insert(r.id.as_str(), r);
    }
    let first = |s: &'static str| by.get(&(s, 1));
    println!("{} cases\n", cases.len());
    println!("| setup | answered (run 1) | right (run 1) | accuracy | right on all 3 runs | same answer on all 3 runs | $ per case | s per case |");
    println!("|---|---|---|---|---|---|---|---|");
    for s in SETUPS {
        let Some(m) = first(s) else { continue };
        let right = m.values().filter(|r| r.right).count();
        let answered = m.values().filter(|r| r.answer.is_some()).count();
        let three: Vec<&str> = cases.iter().map(|c| c.id.as_str()).filter(|id| (1..=3).all(|run| by.get(&(s, run)).is_some_and(|m| m.contains_key(id)))).collect();
        let all3 = three.iter().filter(|id| (1..=3).all(|run| by[&(s, run)][*id].right)).count();
        let same3 = three.iter().filter(|id| by[&(s, 1)][*id].answer.is_some() && (2..=3).all(|run| by[&(s, run)][*id].answer == by[&(s, 1)][*id].answer)).count();
        let n = cases.len().max(1) as f64;
        let cost: f64 = m.values().map(|r| r.cost).sum::<f64>() / n;
        let secs: f64 = m.values().map(|r| r.seconds).sum::<f64>() / n;
        let (consistency, same) = if three.is_empty() || matches!(s, "R true" | "S1 true") { ("—".to_string(), "—".to_string()) } else { (format!("{all3} of {}", three.len()), format!("{same3} of {}", three.len())) };
        println!("| {s} | {answered} of {} | {right} | {:.1}% | {consistency} | {same} | {cost:.5} | {secs:.2} |", cases.len(), 100.0 * right as f64 / n);
    }
    println!("\nRight in run 1, by level (5, 8 or 11 items):\n");
    println!("| setup | level 0 | level 1 | level 2 |");
    println!("|---|---|---|---|");
    for s in SETUPS {
        let Some(m) = first(s) else { continue };
        let cell = |level: u8| {
            let of: Vec<&Case> = cases.iter().filter(|c| c.level == level).collect();
            format!("{} of {}", of.iter().filter(|c| m.get(c.id.as_str()).is_some_and(|r| r.right)).count(), of.len())
        };
        println!("| {s} | {} | {} | {} |", cell(0), cell(1), cell(2));
    }
    println!("\nPaired, run 1, every case (a forfeit is wrong):\n");
    println!("| comparison | only first right | only second right | difference | 95% interval | McNemar p |");
    println!("|---|---|---|---|---|---|");
    for (a, b) in [("S1 extracted", "P1"), ("S1 extracted", "P3"), ("S1 extracted", "R extracted"), ("S1 true", "R true"), ("P3", "P1")] {
        let (Some(ma), Some(mb)) = (first(a), first(b)) else { continue };
        let ids: Vec<&&str> = ma.keys().filter(|id| mb.contains_key(*id)).collect();
        let xa: Vec<bool> = ids.iter().map(|id| ma[**id].right).collect();
        let xb: Vec<bool> = ids.iter().map(|id| mb[**id].right).collect();
        let only_a = xa.iter().zip(&xb).filter(|(x, y)| **x && !**y).count() as u64;
        let only_b = xa.iter().zip(&xb).filter(|(x, y)| !**x && **y).count() as u64;
        let (lo, hi) = stats::bootstrap(&xa, &xb);
        let d = (xa.iter().filter(|x| **x).count() as f64 - xb.iter().filter(|x| **x).count() as f64) / ids.len().max(1) as f64;
        println!("| {a} − {b} | {only_a} | {only_b} | {:+.1} pts | [{:+.1}, {:+.1}] | {:.2e} |", 100.0 * d, 100.0 * lo, 100.0 * hi, stats::mcnemar(only_a, only_b));
    }
    // Where extraction went wrong: which facts differ from the truth.
    let mut wrong_facts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut extracted_cases = 0;
    for c in cases {
        let Some(Ok(f)) = extracted(c, 1) else { continue };
        extracted_cases += 1;
        let t = &c.facts;
        for (what, bad) in [("class", f.class != t.class), ("region", f.region != t.region), ("way", f.way != t.way && t.region != "U.S."), ("ticket", f.ticket != t.ticket), ("item count", f.bags.len() != t.bags.len()), ("a bag", f.bags.len() == t.bags.len() && f.bags != t.bags)] {
            if bad {
                *wrong_facts.entry(what).or_default() += 1;
            }
        }
    }
    println!("\nExtraction, run 1, {extracted_cases} cases read: cases with a wrong {wrong_facts:?}");
}
