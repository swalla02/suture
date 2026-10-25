//! A chess engine over UCI, as a child process: Stockfish and lc0.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct Engine {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Engine {
    pub fn start(program: &str, args: &[&str]) -> Result<Engine, String> {
        let mut child = Command::new(program).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| format!("{program}: {e}"))?;
        let input = child.stdin.take().ok_or("no stdin")?;
        let output = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let mut e = Engine { child, input, output };
        e.send("uci")?;
        e.until("uciok")?;
        Ok(e)
    }

    pub fn send(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.input, "{line}").and_then(|_| self.input.flush()).map_err(|e| format!("engine: {e}"))
    }

    /// The lines up to and including the first that starts with `end`.
    pub fn until(&mut self, end: &str) -> Result<Vec<String>, String> {
        let mut lines = vec![];
        loop {
            let mut l = String::new();
            if self.output.read_line(&mut l).map_err(|e| format!("engine: {e}"))? == 0 {
                return Err(format!("the engine stopped before `{end}`"));
            }
            let l = l.trim_end().to_string();
            let done = l.starts_with(end);
            lines.push(l);
            if done {
                return Ok(lines);
            }
        }
    }

    pub fn option(&mut self, name: &str, value: &str) -> Result<(), String> {
        self.send(&format!("setoption name {name} value {value}"))
    }

    pub fn ready(&mut self) -> Result<(), String> {
        self.send("isready")?;
        self.until("readyok").map(|_| ())
    }

    pub fn position(&mut self, fen: &str, moves: &[String]) -> Result<(), String> {
        match moves.is_empty() {
            true => self.send(&format!("position fen {fen}")),
            false => self.send(&format!("position fen {fen} moves {}", moves.join(" "))),
        }
    }

    /// `go …` and the move it settles on, with the lines before it.
    pub fn go(&mut self, how: &str) -> Result<(String, Vec<String>), String> {
        self.send(&format!("go {how}"))?;
        let lines = self.until("bestmove")?;
        let best = lines.last().and_then(|l| l.split_whitespace().nth(1)).ok_or("no bestmove")?.to_string();
        Ok((best, lines))
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.send("quit");
        let _ = self.child.wait();
    }
}
