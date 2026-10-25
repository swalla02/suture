//! The players: the anchors (a random mover, Stockfish at a capped Elo), and
//! each decision model alone and inside the Suture strategy.

use crate::game::{Game, Player};
use crate::uci::Engine;
use chess_engine::facts::{facts, material};
use chess_engine::{Color, Kind, Move};
use std::process::{Command, Stdio};
use std::io::Write;
use suture_lang::ir::Program;
use suture_runtime::host::Host;
use suture_runtime::state::{Bind, Val};

/// A small, seeded generator, so that a run can be played again.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

pub struct Random(pub Rng);

impl Player for Random {
    fn name(&self) -> String {
        "random".into()
    }
    fn choose(&mut self, g: &Game) -> Result<Move, String> {
        let legal = g.board().legal();
        Ok(legal[self.0.below(legal.len())])
    }
}

/// Stockfish with its strength capped at `elo` (its own UCI_Elo scale),
/// thinking `ms` per move.
pub struct Stockfish {
    engine: Engine,
    elo: u32,
    ms: u32,
}

impl Stockfish {
    pub fn new(program: &str, elo: u32, ms: u32) -> Result<Stockfish, String> {
        let mut engine = Engine::start(program, &[])?;
        engine.option("Threads", "1")?;
        engine.option("Hash", "16")?;
        engine.option("UCI_LimitStrength", "true")?;
        engine.option("UCI_Elo", &elo.to_string())?;
        engine.ready()?;
        Ok(Stockfish { engine, elo, ms })
    }
}

impl Player for Stockfish {
    fn name(&self) -> String {
        format!("stockfish {}", self.elo)
    }
    fn new_game(&mut self) {
        let _ = self.engine.send("ucinewgame");
    }
    fn choose(&mut self, g: &Game) -> Result<Move, String> {
        self.engine.position(&g.start.fen(), &g.uci_moves())?;
        let (best, _) = self.engine.go(&format!("movetime {}", self.ms))?;
        g.board().parse_move(&best).ok_or(format!("stockfish played {best}"))
    }
}

/// A decision model: a score from 0 to 1000 for each legal move.
pub trait Scorer {
    fn name(&self) -> String;
    fn scores(&mut self, g: &Game, moves: &[Move]) -> Result<Vec<i64>, String>;
    /// Answers it gave that named no legal move, this game.
    fn misses(&self) -> u32 {
        0
    }
}

/// No model: every move the same, so the strategy alone decides.
pub struct Flat;

impl Scorer for Flat {
    fn name(&self) -> String {
        "none".into()
    }
    fn scores(&mut self, _: &Game, moves: &[Move]) -> Result<Vec<i64>, String> {
        Ok(vec![500; moves.len()])
    }
}

/// A Leela network's policy at one node (Maia, Leela): the probability it
/// gives each move, without search.
pub struct Lc0 {
    engine: Engine,
    net: String,
}

impl Lc0 {
    /// `program` is lc0; `weights` a path it can open.
    pub fn new(program: &str, weights: &str, net: &str) -> Result<Lc0, String> {
        let mut engine = Engine::start(program, &[])?;
        engine.option("WeightsFile", weights)?;
        engine.option("Threads", "1")?;
        engine.option("VerboseMoveStats", "true")?;
        engine.option("SmartPruningFactor", "0")?;
        engine.ready()?;
        Ok(Lc0 { engine, net: net.to_string() })
    }
}

impl Scorer for Lc0 {
    fn name(&self) -> String {
        self.net.clone()
    }
    fn scores(&mut self, g: &Game, moves: &[Move]) -> Result<Vec<i64>, String> {
        self.engine.position(&g.start.fen(), &g.uci_moves())?;
        let (_, lines) = self.engine.go("nodes 1")?;
        let mut out = vec![0; moves.len()];
        for l in &lines {
            let Some(rest) = l.strip_prefix("info string ") else { continue };
            let Some(m) = rest.split_whitespace().next() else { continue };
            let Some(p) = rest.split("(P:").nth(1).and_then(|p| p.split('%').next()).and_then(|p| p.trim().parse::<f64>().ok()) else { continue };
            if let Some(i) = moves.iter().position(|x| x.to_string() == castling(g.board(), m)) {
                out[i] = (p * 10.0).round() as i64;
            }
        }
        Ok(out)
    }
}

/// lc0 may write castling as the king taking its rook.
fn castling<'a>(b: &chess_engine::Board, m: &'a str) -> &'a str {
    let king = b.parse_move(&m[..4.min(m.len())]).is_none() && b.at(if m.starts_with("e1") { 4 } else { 60 }).is_some_and(|p| p.kind == Kind::King);
    if !king {
        return m;
    }
    match m {
        "e1h1" => "e1g1",
        "e1a1" => "e1c1",
        "e8h8" => "e8g8",
        "e8a8" => "e8c8",
        _ => m,
    }
}

/// Stockfish's static NNUE evaluation of each move's position, no search.
pub struct Nnue {
    engine: Engine,
}

impl Nnue {
    pub fn new(program: &str) -> Result<Nnue, String> {
        let mut engine = Engine::start(program, &[])?;
        engine.option("Threads", "1")?;
        engine.ready()?;
        Ok(Nnue { engine })
    }
}

impl Scorer for Nnue {
    fn name(&self) -> String {
        "nnue".into()
    }
    fn scores(&mut self, g: &Game, moves: &[Move]) -> Result<Vec<i64>, String> {
        let b = g.board();
        let side = if b.turn == Color::White { 1.0 } else { -1.0 };
        let mut out = vec![];
        for m in moves {
            let next = b.play(*m);
            if next.legal().is_empty() {
                out.push(if next.in_check(next.turn) { 1000 } else { 500 });
                continue;
            }
            self.engine.position(&next.fen(), &[])?;
            self.engine.send("eval")?;
            let lines = self.engine.until("Final evaluation")?;
            let v = lines.last().and_then(|l| l.split_whitespace().find_map(|w| w.parse::<f64>().ok()));
            // In check there is no static evaluation: count it as level.
            let pawns = v.map_or(0.0, |v| v * side);
            out.push((1000.0 / (1.0 + (-pawns / 2.0).exp())).round() as i64);
        }
        Ok(out)
    }
}

/// Where an LLM is reached.
pub enum Via {
    /// The Anthropic API, with the key in `ANTHROPIC_API_KEY`.
    Anthropic,
    /// Claude Code in print mode, on the person's own plan.
    ClaudeCli,
    /// OpenRouter, with the key in `OPENROUTER_API_KEY`.
    OpenRouter,
}

/// An LLM asked for its three best moves from the list it is given.
pub struct Llm {
    pub via: Via,
    pub model: String,
    pub scratch: std::path::PathBuf,
    misses: u32,
    /// Its own request file, apart from other threads' players.
    id: u64,
}

static LLMS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Llm {
    pub fn new(via: Via, model: &str, scratch: std::path::PathBuf) -> Llm {
        Llm { via, model: model.to_string(), scratch, misses: 0, id: LLMS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) }
    }

    fn ask(&self, prompt: &str) -> Result<String, String> {
        let body = |v: serde_json::Value| -> Result<std::path::PathBuf, String> {
            let p = self.scratch.join(format!("ask-{}-{}.json", std::process::id(), self.id));
            std::fs::write(&p, v.to_string()).map_err(|e| e.to_string())?;
            Ok(p)
        };
        // The key goes to curl on stdin, in its config, never in its arguments.
        let curl = |url: &str, header: String, file: &std::path::Path| -> Result<serde_json::Value, String> {
            let mut c = Command::new("curl").args(["-s", "-m", "120", "--config", "-", "--data-binary"]).arg(format!("@{}", file.display())).arg(url).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
            c.stdin.take().ok_or("no stdin")?.write_all(format!("header = \"{header}\"\nheader = \"content-type: application/json\"\n").as_bytes()).map_err(|e| e.to_string())?;
            let out = c.wait_with_output().map_err(|e| e.to_string())?;
            serde_json::from_slice(&out.stdout).map_err(|e| format!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
        };
        let key = |var: &str| std::env::var(var).map_err(|_| format!("{var} is not set"));
        match self.via {
            Via::Anthropic => {
                let f = body(serde_json::json!({"model": self.model, "max_tokens": 400, "thinking": {"type": "disabled"}, "messages": [{"role": "user", "content": prompt}]}))?;
                let v = curl("https://api.anthropic.com/v1/messages", format!("x-api-key: {}\"\nheader = \"anthropic-version: 2023-06-01", key("ANTHROPIC_API_KEY")?), &f)?;
                v["content"].as_array().map(|c| c.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(" ")).ok_or(format!("no answer: {v}"))
            }
            Via::OpenRouter => {
                let f = body(serde_json::json!({"model": self.model, "max_tokens": 2000, "messages": [{"role": "user", "content": prompt}]}))?;
                let v = curl("https://openrouter.ai/api/v1/chat/completions", format!("authorization: Bearer {}", key("OPENROUTER_API_KEY")?), &f)?;
                v["choices"][0]["message"]["content"].as_str().map(str::to_string).ok_or(format!("no answer: {v}"))
            }
            Via::ClaudeCli => {
                let out = Command::new("claude")
                    .args(["-p", "--model", &self.model, "--output-format", "text", "--tools", "", "--no-session-persistence", "--strict-mcp-config", "--setting-sources", "", "--disable-slash-commands"])
                    .args(["--system-prompt", "You pick chess moves. Answer with moves only."])
                    .arg(prompt)
                    .output()
                    .map_err(|e| e.to_string())?;
                Ok(String::from_utf8_lossy(&out.stdout).to_string())
            }
        }
    }
}

pub fn prompt(g: &Game, moves: &[Move]) -> String {
    let side = if g.board().turn == Color::White { "White" } else { "Black" };
    let list: Vec<String> = moves.iter().map(Move::to_string).collect();
    format!(
        "You are playing chess as {side}. The position, in FEN: {}\nThe game so far, from {}: {}\nYour legal moves: {}\nReply with your three best moves from that list, best first, in the same notation, separated by spaces, and nothing else.",
        g.board().fen(),
        g.start.fen(),
        if g.moves.is_empty() { "no moves yet".to_string() } else { g.uci_moves().join(" ") },
        list.join(" ")
    )
}

/// The moves an answer names, in its order, at most three.
pub fn named(answer: &str, moves: &[Move]) -> Vec<usize> {
    let mut out = vec![];
    for w in answer.split(|c: char| !c.is_ascii_alphanumeric()) {
        if let Some(i) = moves.iter().position(|m| m.to_string() == w.to_ascii_lowercase())
            && !out.contains(&i)
        {
            out.push(i);
        }
        if out.len() == 3 {
            break;
        }
    }
    out
}

impl Scorer for Llm {
    fn name(&self) -> String {
        self.model.clone()
    }
    fn scores(&mut self, g: &Game, moves: &[Move]) -> Result<Vec<i64>, String> {
        // A rate limit is a wait, not a lost game.
        let mut tries = 0;
        let answer = loop {
            match self.ask(&prompt(g, moves)) {
                Err(e) if tries < 6 && (e.contains("429") || e.contains("rate")) => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_secs(10 * tries));
                }
                r => break r?,
            }
        };
        let picks = named(&answer, moves);
        if picks.is_empty() {
            self.misses += 1;
        }
        let mut out = vec![0; moves.len()];
        for (r, i) in picks.into_iter().enumerate() {
            out[i] = [1000, 600, 300][r];
        }
        Ok(out)
    }
    fn misses(&self) -> u32 {
        self.misses
    }
}

/// A decision model alone: it plays its highest-scored move, ties at random.
pub struct Naked {
    pub scorer: Box<dyn Scorer>,
    pub rng: Rng,
}

impl Player for Naked {
    fn name(&self) -> String {
        format!("{} alone", self.scorer.name())
    }
    fn choose(&mut self, g: &Game) -> Result<Move, String> {
        let legal = g.board().legal();
        let s = self.scorer.scores(g, &legal)?;
        let top = *s.iter().max().unwrap_or(&0);
        let best: Vec<usize> = (0..legal.len()).filter(|&i| s[i] == top).collect();
        Ok(legal[best[self.rng.below(best.len())]])
    }
}

/// A decision model inside the Suture strategy: the moves go into a pool
/// with their facts and the model's scores, and the program's `prefer`
/// picks one (eval/chess/suture).
pub struct Sutured {
    pub prog: Program,
    pub scorer: Box<dyn Scorer>,
    pub rng: Rng,
    host: Option<Host>,
    overrides: u32,
    /// The Move attributes the strategy declares, other than its formulas.
    reads: Vec<String>,
}

impl Sutured {
    pub fn new(prog: Program, scorer: Box<dyn Scorer>, rng: Rng) -> Sutured {
        let reads = prog.kind("Move").map(|k| k.attrs.iter().filter(|a| a.formula.is_none()).map(|a| a.name.clone()).collect()).unwrap_or_default();
        Sutured { prog, scorer, rng, host: None, overrides: 0, reads }
    }
}

impl Player for Sutured {
    fn name(&self) -> String {
        format!("{} with Suture", self.scorer.name())
    }
    fn new_game(&mut self) {
        self.host = Some(Host::new(self.prog.clone()));
        self.overrides = 0;
    }
    fn overrides(&self) -> u32 {
        self.overrides
    }
    fn choose(&mut self, g: &Game) -> Result<Move, String> {
        let b = g.board();
        let legal = b.legal();
        let scores = self.scorer.scores(g, &legal)?;
        let host = self.host.get_or_insert_with(|| Host::new(self.prog.clone()));
        let n = g.ply();
        let us = b.turn;
        let balance = material(b, us) - material(b, us.other());
        let pieces: i32 = (0..64).filter_map(|s| b.at(s)).filter(|p| !matches!(p.kind, Kind::Pawn | Kind::King)).map(|p| p.kind.value()).sum();
        let mut rank: Vec<usize> = (0..legal.len()).collect();
        rank.sort_by_key(|&i| -scores[i]);
        // Entered in a shuffled order, so that a tie the strategy leaves is
        // broken at random, not by the move generator's order.
        let mut order: Vec<usize> = (0..legal.len()).collect();
        for i in (1..order.len()).rev() {
            order.swap(i, self.rng.below(i + 1));
        }
        for &i in &order {
            let m = legal[i];
            let f = facts(b, m);
            let count = |v: i32| Val::Count(v.max(0) as i64);
            let flag = |v: bool| Val::Count(v as i64);
            let attrs: Vec<(String, Val)> = vec![
                ("ply".into(), Val::Count(n as i64)),
                ("uci".into(), Val::Text(m.to_string())),
                ("piece".into(), Val::Text(f.piece.into())),
                ("captures".into(), count(f.captures)),
                ("checking".into(), Val::Bool(f.check)),
                ("mating".into(), Val::Bool(f.mate)),
                ("stalemating".into(), Val::Bool(f.stalemate)),
                ("hangs".into(), count(f.hangs)),
                ("threat".into(), count(f.threat)),
                ("fork".into(), count(f.fork)),
                ("wins".into(), count(f.outcome)),
                ("loses".into(), count(-f.outcome)),
                ("gains".into(), count(f.outlook)),
                ("drops".into(), count(-f.outlook)),
                ("danger".into(), count(f.danger)),
                ("cornered".into(), Val::Bool(f.cornered)),
                ("luft".into(), count(f.luft)),
                ("exposing".into(), Val::Bool(f.allows_mate)),
                ("castling".into(), Val::Bool(f.castles)),
                ("promoting".into(), Val::Bool(f.promotes)),
                ("developing".into(), Val::Bool(f.develops)),
                ("undeveloping".into(), Val::Bool(f.undevelops)),
                ("central".into(), Val::Bool(f.center)),
                ("pawnrank".into(), count(f.pawn_rank)),
                ("passed".into(), Val::Bool(f.passed)),
                ("trading".into(), Val::Bool(f.trades)),
                ("boxed".into(), count(f.boxed)),
                ("kings".into(), count(f.kings)),
                ("kingedge".into(), count(f.king_edge)),
                ("theiredge".into(), count(f.their_edge)),
                ("replies".into(), count(f.replies)),
                ("mobility".into(), count(f.mobility)),
                ("loose".into(), count(f.loose)),
                ("shield".into(), count(f.shield)),
                ("pressure".into(), count(f.pressure)),
                ("up".into(), count(balance)),
                ("down".into(), count(-balance)),
                ("ahead".into(), Val::Bool(balance >= 2)),
                ("behind".into(), Val::Bool(balance <= -2)),
                ("opening".into(), Val::Bool(n < 20)),
                ("endgame".into(), Val::Bool(pieces <= 26)),
                ("repeating".into(), Val::Bool(g.repeats(&b.play(m)))),
                // The same facts as numbers, 1 or 0, for the strategy's formulas.
                ("checks".into(), flag(f.check)),
                ("castles".into(), flag(f.castles)),
                ("centers".into(), flag(f.center)),
                ("pawnmoves".into(), flag(f.piece == "pawn")),
                ("develops".into(), flag(f.develops)),
                ("undevelops".into(), flag(f.undevelops)),
                ("kingmoves".into(), flag(f.piece == "king" && !f.castles)),
                ("queenmoves".into(), flag(f.piece == "queen")),
                ("pushes".into(), count(if f.passed { f.pawn_rank } else { 0 })),
                ("trades".into(), flag(f.trades)),
                ("nets".into(), flag(f.cornered)),
                ("traps".into(), flag(f.trapped)),
                ("stalemates".into(), flag(f.stalemate)),
                ("repeats".into(), flag(g.repeats(&b.play(m)))),
                ("early".into(), flag(n < 20)),
                ("late".into(), flag(pieces <= 26)),
                ("score".into(), Val::Count(scores[i])),
                ("rank".into(), Val::Count(rank.iter().position(|&r| r == i).unwrap_or(0) as i64 + 1)),
            ];
            // Only the facts the strategy's Move declares: a strategy names
            // what it reads, and the rest is not entered.
            let attrs: Vec<(String, Val)> = attrs.into_iter().filter(|(a, _)| self.reads.iter().any(|r| r == a)).collect();
            host.create("Move", &format!("M{n}x{i}"), &attrs)?;
        }
        let turn = format!("T{n}");
        host.create("Turn", &turn, &[("ply".into(), Val::Count(n as i64))])?;
        let unit = host.unit(&turn).ok_or("the Turn was not created")?;
        let to = unit.root.parts.iter().flat_map(|p| p.binds.values()).find_map(|b| match b {
            Bind::Granted { to, .. } => Some(to.clone()),
            _ => None,
        });
        let to = to.ok_or_else(|| format!("the strategy chose no move at ply {n}"))?;
        let i: usize = to.rsplit('x').next().and_then(|i| i.parse().ok()).ok_or(format!("not a move: {to}"))?;
        if scores[i] < scores[rank[0]] {
            self.overrides += 1;
        }
        Ok(legal[i])
    }
}
