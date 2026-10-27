//! `ladder match ⟨player⟩ ⟨player⟩ ⟨games⟩ ⟨results.jsonl⟩ [threads]`
//! plays a match, colours alternating, each opening from both sides, and
//! appends one line per game. `ladder elo ⟨results.jsonl⟩…` fits ratings.
//!
//! `CHESS_STRATEGY` is the Suture module (eval/chess/suture),
//! `CHESS_SCRATCH` a directory for request bodies, `CHESS_MS` Stockfish's
//! time per move in milliseconds (100).

use chess_ladder::{elo, game, player};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("match") if args.len() >= 5 => run(&args[1], &args[2], args[3].parse().unwrap_or(2), &args[4], args.get(5).and_then(|t| t.parse().ok()).unwrap_or(1)),
        Some("elo") if args.len() >= 2 => ratings(&args[1..]),
        _ => Err("usage: ladder match ⟨player⟩ ⟨player⟩ ⟨games⟩ ⟨out.jsonl⟩ [threads] | ladder elo ⟨results.jsonl⟩…".into()),
    };
    if let Err(e) = r {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn env(var: &str, or: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| or.to_string())
}

fn run(a: &str, b: &str, games: usize, out: &str, threads: usize) -> Result<(), String> {
    let strategy = PathBuf::from(env("CHESS_STRATEGY", "eval/chess/suture"));
    let scratch = PathBuf::from(env("CHESS_SCRATCH", &std::env::temp_dir().display().to_string()));
    let ms: u32 = env("CHESS_MS", "100").parse().unwrap_or(100);
    let next = Arc::new(AtomicUsize::new(0));
    let file = Arc::new(Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(out).map_err(|e| format!("{out}: {e}"))?));
    let seed0: u64 = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1);
    let mut handles = vec![];
    for t in 0..threads.max(1) {
        let (a, b, strategy, scratch, next, file) = (a.to_string(), b.to_string(), strategy.clone(), scratch.clone(), next.clone(), file.clone());
        handles.push(std::thread::spawn(move || -> Result<(), String> {
            let seed = seed0 + t as u64 * 7919;
            let mut pa = player(&a, seed, &strategy, &scratch, ms)?;
            let mut pb = player(&b, seed + 1, &strategy, &scratch, ms)?;
            loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= games {
                    return Ok(());
                }
                // Game 2k and 2k+1 play opening k from each side.
                let start = game::opening(i / 2);
                let a_white = i % 2 == 0;
                let o = if a_white { game::play(pa.as_mut(), pb.as_mut(), start, 300) } else { game::play(pb.as_mut(), pa.as_mut(), start, 300) };
                let (white, black) = if a_white { (pa.name(), pb.name()) } else { (pb.name(), pa.name()) };
                let result = match o.result {
                    game::Result_::White => "1-0",
                    game::Result_::Black => "0-1",
                    game::Result_::Draw => "1/2-1/2",
                };
                let line = serde_json::json!({"white": white, "black": black, "result": result, "reason": o.reason, "opening": i / 2 % game::OPENINGS.len(), "plies": o.moves.len(), "overrides": [o.overrides.0, o.overrides.1], "moves": o.moves.join(" ")});
                let mut f = file.lock().map_err(|_| "a poisoned lock")?;
                writeln!(f, "{line}").map_err(|e| e.to_string())?;
                eprintln!("{i}: {white} - {black} {result} ({}, {} plies)", o.reason, o.moves.len());
            }
        }));
    }
    for h in handles {
        h.join().map_err(|_| "a game thread panicked")??;
    }
    Ok(())
}

fn ratings(files: &[String]) -> Result<(), String> {
    let mut games = vec![];
    for f in files {
        let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
        for l in text.lines().filter(|l| !l.trim().is_empty()) {
            let v: serde_json::Value = serde_json::from_str(l).map_err(|e| format!("{f}: {e}"))?;
            let score = match v["result"].as_str() {
                Some("1-0") => 1.0,
                Some("0-1") => 0.0,
                _ => 0.5,
            };
            games.push(elo::Game { white: v["white"].as_str().unwrap_or("").into(), black: v["black"].as_str().unwrap_or("").into(), score });
        }
    }
    let out: Vec<serde_json::Value> = elo::fit(&games).into_iter().map(|(n, r)| serde_json::json!({"player": n, "elo": r.elo.round(), "ci": r.ci.round(), "games": r.games, "score": r.score})).collect();
    println!("{}", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?);
    Ok(())
}
