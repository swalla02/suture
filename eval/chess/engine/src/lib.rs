//! Chess rules for the chess eval (eval/chess): positions, legal moves and
//! FEN. The strategy that picks among the moves is Suture's; this crate only
//! says what the moves are. Squares are 0..64, a1 = 0, h1 = 7, a8 = 56.

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn other(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl Kind {
    /// The usual material value, in pawns.
    pub fn value(self) -> i32 {
        match self {
            Kind::Pawn => 1,
            Kind::Knight | Kind::Bishop => 3,
            Kind::Rook => 5,
            Kind::Queen => 9,
            Kind::King => 0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Pawn => "pawn",
            Kind::Knight => "knight",
            Kind::Bishop => "bishop",
            Kind::Rook => "rook",
            Kind::Queen => "queen",
            Kind::King => "king",
        }
    }

    fn letter(self) -> char {
        match self {
            Kind::Pawn => 'p',
            Kind::Knight => 'n',
            Kind::Bishop => 'b',
            Kind::Rook => 'r',
            Kind::Queen => 'q',
            Kind::King => 'k',
        }
    }

    fn of(c: char) -> Option<Kind> {
        Some(match c.to_ascii_lowercase() {
            'p' => Kind::Pawn,
            'n' => Kind::Knight,
            'b' => Kind::Bishop,
            'r' => Kind::Rook,
            'q' => Kind::Queen,
            'k' => Kind::King,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Piece {
    pub color: Color,
    pub kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Move {
    pub from: u8,
    pub to: u8,
    pub promo: Option<Kind>,
}

impl fmt::Display for Move {
    /// UCI notation: `e2e4`, `e7e8q`.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{}", square_name(self.from), square_name(self.to))?;
        if let Some(k) = self.promo {
            write!(f, "{}", k.letter())?;
        }
        Ok(())
    }
}

pub fn square_name(s: u8) -> String {
    format!("{}{}", (b'a' + s % 8) as char, s / 8 + 1)
}

fn square_of(t: &str) -> Option<u8> {
    let b = t.as_bytes();
    match b {
        [f @ b'a'..=b'h', r @ b'1'..=b'8'] => Some((r - b'1') * 8 + (f - b'a')),
        _ => None,
    }
}

const WK: u8 = 1;
const WQ: u8 = 2;
const BK: u8 = 4;
const BQ: u8 = 8;

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Board {
    sq: [Option<Piece>; 64],
    pub turn: Color,
    castle: u8,
    ep: Option<u8>,
    pub halfmove: u32,
    pub fullmove: u32,
}

pub const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

const KNIGHT: [(i8, i8); 8] = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];
const KING: [(i8, i8); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
const ROOK: [(i8, i8); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
const BISHOP: [(i8, i8); 4] = [(1, 1), (-1, 1), (-1, -1), (1, -1)];

/// The square `(df, dr)` away from `s`, if it is on the board.
fn step(s: u8, (df, dr): (i8, i8)) -> Option<u8> {
    let (f, r) = ((s % 8) as i8 + df, (s / 8) as i8 + dr);
    ((0..8).contains(&f) && (0..8).contains(&r)).then_some((r * 8 + f) as u8)
}

impl Board {
    pub fn start() -> Board {
        Board::from_fen(START).expect("the start position parses")
    }

    pub fn from_fen(fen: &str) -> Result<Board, String> {
        let mut parts = fen.split_whitespace();
        let rows = parts.next().ok_or("an empty FEN")?;
        let mut sq = [None; 64];
        let ranks: Vec<&str> = rows.split('/').collect();
        if ranks.len() != 8 {
            return Err(format!("a FEN has 8 ranks, not {}", ranks.len()));
        }
        for (i, row) in ranks.iter().enumerate() {
            let r = 7 - i as u8;
            let mut f = 0u8;
            for c in row.chars() {
                if let Some(n) = c.to_digit(10) {
                    f += n as u8;
                } else {
                    let kind = Kind::of(c).ok_or(format!("`{c}` is not a piece"))?;
                    let color = if c.is_ascii_uppercase() { Color::White } else { Color::Black };
                    if f >= 8 {
                        return Err(format!("rank {} is too long", r + 1));
                    }
                    sq[(r * 8 + f) as usize] = Some(Piece { color, kind });
                    f += 1;
                }
            }
            if f != 8 {
                return Err(format!("rank {} has {f} squares", r + 1));
            }
        }
        let turn = match parts.next().unwrap_or("w") {
            "w" => Color::White,
            "b" => Color::Black,
            t => return Err(format!("`{t}` is not a side to move")),
        };
        let mut castle = 0;
        for c in parts.next().unwrap_or("-").chars() {
            castle |= match c {
                'K' => WK,
                'Q' => WQ,
                'k' => BK,
                'q' => BQ,
                '-' => 0,
                _ => return Err(format!("`{c}` is not a castling right")),
            };
        }
        let ep = match parts.next().unwrap_or("-") {
            "-" => None,
            t => Some(square_of(t).ok_or(format!("`{t}` is not a square"))?),
        };
        let halfmove = parts.next().unwrap_or("0").parse().map_err(|_| "a bad halfmove clock")?;
        let fullmove = parts.next().unwrap_or("1").parse().map_err(|_| "a bad move number")?;
        Ok(Board { sq, turn, castle, ep, halfmove, fullmove })
    }

    pub fn fen(&self) -> String {
        let mut out = String::new();
        for r in (0..8).rev() {
            let mut empty = 0;
            for f in 0..8 {
                match self.sq[r * 8 + f] {
                    None => empty += 1,
                    Some(p) => {
                        if empty > 0 {
                            out.push_str(&empty.to_string());
                            empty = 0;
                        }
                        let c = p.kind.letter();
                        out.push(if p.color == Color::White { c.to_ascii_uppercase() } else { c });
                    }
                }
            }
            if empty > 0 {
                out.push_str(&empty.to_string());
            }
            if r > 0 {
                out.push('/');
            }
        }
        let rights: String = [(WK, 'K'), (WQ, 'Q'), (BK, 'k'), (BQ, 'q')].iter().filter(|(b, _)| self.castle & b != 0).map(|(_, c)| *c).collect();
        let ep = self.ep.map_or("-".to_string(), square_name);
        let turn = if self.turn == Color::White { "w" } else { "b" };
        format!("{out} {turn} {} {ep} {} {}", if rights.is_empty() { "-" } else { &rights }, self.halfmove, self.fullmove)
    }

    pub fn at(&self, s: u8) -> Option<Piece> {
        self.sq[s as usize]
    }

    pub fn king(&self, c: Color) -> Option<u8> {
        (0..64).find(|&s| self.sq[s as usize] == Some(Piece { color: c, kind: Kind::King }))
    }

    /// Whether `by` attacks square `s`.
    pub fn attacked(&self, s: u8, by: Color) -> bool {
        self.attackers(s, by).next().is_some()
    }

    /// The squares of `by`'s pieces that attack `s`.
    pub fn attackers(&self, s: u8, by: Color) -> impl Iterator<Item = u8> + '_ {
        let is = move |t: u8, kinds: &[Kind]| self.sq[t as usize].is_some_and(|p| p.color == by && kinds.contains(&p.kind));
        let back = if by == Color::White { -1 } else { 1 };
        let pawns = [(-1, back), (1, back)].into_iter().filter_map(move |d| step(s, d)).filter(move |&t| is(t, &[Kind::Pawn]));
        let knights = KNIGHT.into_iter().filter_map(move |d| step(s, d)).filter(move |&t| is(t, &[Kind::Knight]));
        let kings = KING.into_iter().filter_map(move |d| step(s, d)).filter(move |&t| is(t, &[Kind::King]));
        let ray = move |d: (i8, i8), kinds: &'static [Kind]| {
            let mut t = s;
            loop {
                match step(t, d) {
                    None => return None,
                    Some(n) => match self.sq[n as usize] {
                        None => t = n,
                        Some(_) => return is(n, kinds).then_some(n),
                    },
                }
            }
        };
        let lines = ROOK.into_iter().filter_map(move |d| ray(d, &[Kind::Rook, Kind::Queen]));
        let diagonals = BISHOP.into_iter().filter_map(move |d| ray(d, &[Kind::Bishop, Kind::Queen]));
        pawns.chain(knights).chain(kings).chain(lines).chain(diagonals)
    }

    pub fn in_check(&self, c: Color) -> bool {
        self.king(c).is_some_and(|k| self.attacked(k, c.other()))
    }

    /// Moves that follow the pieces' rules, before checking the mover's king.
    fn pseudo(&self, out: &mut Vec<Move>) {
        let us = self.turn;
        let push = |out: &mut Vec<Move>, from: u8, to: u8| out.push(Move { from, to, promo: None });
        for from in 0..64u8 {
            let Some(p) = self.sq[from as usize] else { continue };
            if p.color != us {
                continue;
            }
            match p.kind {
                Kind::Pawn => {
                    let (dir, home, last) = if us == Color::White { (1, 1, 7) } else { (-1, 6, 0) };
                    let promote = |out: &mut Vec<Move>, to: u8| {
                        if to / 8 == last {
                            for k in [Kind::Queen, Kind::Rook, Kind::Bishop, Kind::Knight] {
                                out.push(Move { from, to, promo: Some(k) });
                            }
                        } else {
                            push(out, from, to);
                        }
                    };
                    if let Some(one) = step(from, (0, dir))
                        && self.sq[one as usize].is_none()
                    {
                        promote(out, one);
                        if from / 8 == home
                            && let Some(two) = step(one, (0, dir))
                            && self.sq[two as usize].is_none()
                        {
                            push(out, from, two);
                        }
                    }
                    for df in [-1, 1] {
                        if let Some(to) = step(from, (df, dir)) {
                            if self.sq[to as usize].is_some_and(|q| q.color != us) {
                                promote(out, to);
                            } else if self.ep == Some(to) {
                                push(out, from, to);
                            }
                        }
                    }
                }
                Kind::Knight | Kind::King => {
                    let dirs = if p.kind == Kind::Knight { &KNIGHT } else { &KING };
                    for &d in dirs {
                        if let Some(to) = step(from, d)
                            && self.sq[to as usize].is_none_or(|q| q.color != us)
                        {
                            push(out, from, to);
                        }
                    }
                }
                Kind::Bishop | Kind::Rook | Kind::Queen => {
                    let dirs: &[(i8, i8)] = match p.kind {
                        Kind::Bishop => &BISHOP,
                        Kind::Rook => &ROOK,
                        _ => &KING,
                    };
                    for &d in dirs {
                        let mut t = from;
                        while let Some(to) = step(t, d) {
                            match self.sq[to as usize] {
                                None => push(out, from, to),
                                Some(q) => {
                                    if q.color != us {
                                        push(out, from, to);
                                    }
                                    break;
                                }
                            }
                            t = to;
                        }
                    }
                }
            }
        }
        // Castling: the right, empty squares between, and no check on the
        // king's start, path or end.
        let (rank, short, long) = if us == Color::White { (0u8, WK, WQ) } else { (56u8, BK, BQ) };
        let them = us.other();
        let empty = |fs: &[u8]| fs.iter().all(|f| self.sq[(rank + f) as usize].is_none());
        let safe = |fs: &[u8]| fs.iter().all(|f| !self.attacked(rank + f, them));
        if self.castle & short != 0 && empty(&[5, 6]) && safe(&[4, 5, 6]) {
            push(out, rank + 4, rank + 6);
        }
        if self.castle & long != 0 && empty(&[1, 2, 3]) && safe(&[4, 3, 2]) {
            push(out, rank + 4, rank + 2);
        }
    }

    pub fn legal(&self) -> Vec<Move> {
        let mut out = Vec::with_capacity(48);
        self.pseudo(&mut out);
        out.retain(|m| !self.play(*m).in_check(self.turn));
        out
    }

    /// The position after `m`, which is assumed to follow the pieces' rules.
    pub fn play(&self, m: Move) -> Board {
        let mut b = self.clone();
        let Some(p) = b.sq[m.from as usize] else { return b };
        let captured = b.sq[m.to as usize].is_some();
        b.sq[m.from as usize] = None;
        if p.kind == Kind::Pawn && self.ep == Some(m.to) && !captured {
            let behind = if p.color == Color::White { m.to - 8 } else { m.to + 8 };
            b.sq[behind as usize] = None;
        }
        b.sq[m.to as usize] = Some(Piece { color: p.color, kind: m.promo.unwrap_or(p.kind) });
        if p.kind == Kind::King && m.from.abs_diff(m.to) == 2 {
            let rank = m.from / 8 * 8;
            let (rf, rt) = if m.to % 8 == 6 { (rank + 7, rank + 5) } else { (rank, rank + 3) };
            b.sq[rt as usize] = b.sq[rf as usize].take();
        }
        for s in [m.from, m.to] {
            b.castle &= match s {
                0 => !WQ,
                4 => !(WK | WQ),
                7 => !WK,
                56 => !BQ,
                60 => !(BK | BQ),
                63 => !BK,
                _ => !0,
            };
        }
        b.ep = (p.kind == Kind::Pawn && m.from.abs_diff(m.to) == 16).then(|| (m.from + m.to) / 2);
        b.halfmove = if p.kind == Kind::Pawn || captured { 0 } else { self.halfmove + 1 };
        if self.turn == Color::Black {
            b.fullmove += 1;
        }
        b.turn = self.turn.other();
        b
    }

    /// The legal move written `uci`, if there is one.
    pub fn parse_move(&self, uci: &str) -> Option<Move> {
        self.legal().into_iter().find(|m| m.to_string() == uci)
    }

    /// The number of move sequences `depth` plies long: the standard check
    /// of a move generator against published counts.
    pub fn perft(&self, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        let moves = self.legal();
        if depth == 1 {
            return moves.len() as u64;
        }
        moves.iter().map(|m| self.play(*m).perft(depth - 1)).sum()
    }
}
