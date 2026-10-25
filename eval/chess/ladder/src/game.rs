//! One game between two players, to its end by the rules.

use chess_engine::{Board, Color, Kind, Move};

/// What a player sees: the start, the moves so far and the position now.
pub struct Game {
    pub start: Board,
    pub moves: Vec<Move>,
    pub boards: Vec<Board>,
}

impl Game {
    pub fn new(start: Board) -> Game {
        Game { boards: vec![start.clone()], start, moves: vec![] }
    }

    pub fn board(&self) -> &Board {
        self.boards.last().expect("a game has a position")
    }

    pub fn ply(&self) -> usize {
        self.moves.len()
    }

    /// The moves so far in UCI, for `position fen … moves …`.
    pub fn uci_moves(&self) -> Vec<String> {
        self.moves.iter().map(Move::to_string).collect()
    }

    /// Whether `next` has been seen before in this game.
    pub fn repeats(&self, next: &Board) -> bool {
        let k = key(next);
        self.boards.iter().any(|b| key(b) == k)
    }

    fn play(&mut self, m: Move) {
        let next = self.board().play(m);
        self.moves.push(m);
        self.boards.push(next);
    }
}

/// A position as repetition counts it: pieces, side, rights and en passant.
fn key(b: &Board) -> String {
    b.fen().split(' ').take(4).collect::<Vec<_>>().join(" ")
}

pub trait Player {
    fn name(&self) -> String;
    fn new_game(&mut self) {}
    fn choose(&mut self, g: &Game) -> Result<Move, String>;
    /// How often, this game, the player's strategy overrode its model's own
    /// first choice: Suture's mark on the game.
    fn overrides(&self) -> u32 {
        0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Result_ {
    White,
    Black,
    Draw,
}

pub struct Outcome {
    pub result: Result_,
    pub reason: String,
    pub moves: Vec<String>,
    pub overrides: (u32, u32),
}

/// Plays `white` against `black` from `start` until mate, a draw by the
/// rules, `max_plies`, or a player failing (which loses).
pub fn play(white: &mut dyn Player, black: &mut dyn Player, start: Board, max_plies: usize) -> Outcome {
    white.new_game();
    black.new_game();
    let mut g = Game::new(start);
    let end = |g: &Game, result, reason: &str, w: &dyn Player, b: &dyn Player| Outcome { result, reason: reason.to_string(), moves: g.uci_moves(), overrides: (w.overrides(), b.overrides()) };
    loop {
        let b = g.board();
        let legal = b.legal();
        if legal.is_empty() {
            return match (b.in_check(b.turn), b.turn) {
                (true, Color::White) => end(&g, Result_::Black, "mate", white, black),
                (true, Color::Black) => end(&g, Result_::White, "mate", white, black),
                (false, _) => end(&g, Result_::Draw, "stalemate", white, black),
            };
        }
        if b.halfmove >= 100 {
            return end(&g, Result_::Draw, "fifty moves", white, black);
        }
        if bare(b) {
            return end(&g, Result_::Draw, "insufficient material", white, black);
        }
        let k = key(b);
        if g.boards.iter().filter(|x| key(x) == k).count() >= 3 {
            return end(&g, Result_::Draw, "repetition", white, black);
        }
        if g.ply() >= max_plies {
            return end(&g, Result_::Draw, "move limit", white, black);
        }
        let turn = b.turn;
        let p: &mut dyn Player = if turn == Color::White { white } else { black };
        match p.choose(&g) {
            Ok(m) if legal.contains(&m) => g.play(m),
            Ok(m) => {
                let r = if turn == Color::White { Result_::Black } else { Result_::White };
                return end(&g, r, &format!("illegal move {m} by {}", p.name()), white, black);
            }
            Err(e) => {
                let r = if turn == Color::White { Result_::Black } else { Result_::White };
                return end(&g, r, &format!("{} failed: {e}", p.name()), white, black);
            }
        }
    }
}

/// No mate is possible: kings alone, or a king and one minor piece.
fn bare(b: &Board) -> bool {
    let rest: Vec<Kind> = (0..64).filter_map(|s| b.at(s)).map(|p| p.kind).filter(|k| *k != Kind::King).collect();
    rest.is_empty() || (rest.len() == 1 && matches!(rest[0], Kind::Knight | Kind::Bishop))
}

/// Short, common openings, each played from both sides, so that games
/// between players that choose the same way are not one game repeated.
pub const OPENINGS: &[&str] = &[
    "e2e4 e7e5 g1f3 b8c6",
    "e2e4 e7e5 g1f3 g8f6",
    "e2e4 c7c5 g1f3 d7d6",
    "e2e4 c7c5 b1c3 b8c6",
    "e2e4 e7e6 d2d4 d7d5",
    "e2e4 c7c6 d2d4 d7d5",
    "e2e4 d7d6 d2d4 g8f6",
    "e2e4 d7d5 e4d5 d8d5",
    "d2d4 d7d5 c2c4 e7e6",
    "d2d4 d7d5 c2c4 c7c6",
    "d2d4 g8f6 c2c4 e7e6",
    "d2d4 g8f6 c2c4 g7g6",
    "d2d4 g8f6 g1f3 d7d5",
    "d2d4 f7f5 g2g3 g8f6",
    "c2c4 e7e5 b1c3 g8f6",
    "c2c4 c7c5 g1f3 b8c6",
    "g1f3 d7d5 g2g3 g8f6",
    "g1f3 g8f6 c2c4 g7g6",
    "e2e4 e7e5 f1c4 g8f6",
    "e2e4 g7g6 d2d4 f8g7",
];

/// The start of opening `i`.
pub fn opening(i: usize) -> Board {
    let mut b = Board::start();
    for m in OPENINGS[i % OPENINGS.len()].split(' ') {
        let mv = b.parse_move(m).expect("an opening of legal moves");
        b = b.play(mv);
    }
    b
}
