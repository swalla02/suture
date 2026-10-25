//! The chess ladder (eval/chess): decision models of several kinds and
//! strengths, each alone and inside a Suture strategy, rated against
//! Stockfish capped at known Elo ratings.

pub mod elo;
pub mod game;
pub mod players;
pub mod uci;

use game::Player;
use players::{Decider, Flat, Lc0, Llm, Naked, Nnue, Random, Rng, Scorer, Stockfish, Sutured, Via};
use std::path::{Path, PathBuf};

/// Where the engines and networks are: `CHESS_TOOLS`, else ~/chess-tools.
pub fn tools() -> PathBuf {
    std::env::var("CHESS_TOOLS").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("chess-tools"))
}

pub fn stockfish() -> String {
    tools().join("stockfish/stockfish/stockfish-ubuntu-x86-64-avx2").display().to_string()
}

fn lc0() -> String {
    tools().join("lc0/lc0.exe").display().to_string()
}

/// A path as the Windows lc0 opens it, through WSL.
fn windows(p: &Path) -> Result<String, String> {
    let out = std::process::Command::new("wslpath").arg("-w").arg(p).output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub const NETS: &[(&str, &str)] = &[("maia1100", "maia-1100.pb.gz"), ("maia1500", "maia-1500.pb.gz"), ("maia1900", "maia-1900.pb.gz"), ("leela256", "t1-256x10.pb.gz"), ("leela512", "t1-512x15.pb.gz")];

/// `none`, `nnue`, a net from NETS, `llm:anthropic|cli|openrouter:⟨model⟩`,
/// or `decide:⟨model⟩`, a decision model on OpenRouter.
pub fn scorer(spec: &str, scratch: &Path) -> Result<Box<dyn Scorer>, String> {
    if spec == "none" {
        return Ok(Box::new(Flat));
    }
    if spec == "nnue" {
        return Ok(Box::new(Nnue::new(&stockfish())?));
    }
    if let Some((_, file)) = NETS.iter().find(|(n, _)| *n == spec) {
        return Ok(Box::new(Lc0::new(&lc0(), &windows(&tools().join("nets").join(file))?, spec)?));
    }
    if let Some(rest) = spec.strip_prefix("llm:") {
        let (via, model) = rest.split_once(':').ok_or(format!("`{spec}`: llm:⟨via⟩:⟨model⟩"))?;
        let via = match via {
            "anthropic" => Via::Anthropic,
            "cli" => Via::ClaudeCli,
            "openrouter" => Via::OpenRouter,
            v => return Err(format!("`{v}` is not anthropic, cli or openrouter")),
        };
        return Ok(Box::new(Llm::new(via, model, scratch.to_path_buf())));
    }
    if let Some(model) = spec.strip_prefix("decide:") {
        return Ok(Box::new(Decider::new(model, scratch.to_path_buf())));
    }
    Err(format!("`{spec}` is not a scorer"))
}

/// `random`, `stockfish:⟨elo⟩`, `naked:⟨scorer⟩` or `suture:⟨scorer⟩`.
pub fn player(spec: &str, seed: u64, strategy: &Path, scratch: &Path, ms: u32) -> Result<Box<dyn Player>, String> {
    let rng = Rng::new(seed);
    if spec == "random" {
        return Ok(Box::new(Random(rng)));
    }
    let (arm, rest) = spec.split_once(':').ok_or(format!("`{spec}` is not a player"))?;
    match arm {
        "stockfish" => Ok(Box::new(Stockfish::new(&stockfish(), rest.parse().map_err(|_| format!("`{rest}` is not an Elo"))?, ms)?)),
        "naked" => Ok(Box::new(Naked { scorer: scorer(rest, scratch)?, rng })),
        "suture" => {
            let prog = suture_lang::compile_dir(strategy).map_err(|e| e.iter().map(|e| format!("{}:{}: {}", e.file, e.line, e.msg)).collect::<Vec<_>>().join("\n"))?;
            Ok(Box::new(Sutured::new(prog, scorer(rest, scratch)?, rng)))
        }
        _ => Err(format!("`{arm}` is not random, stockfish, naked or suture")),
    }
}
