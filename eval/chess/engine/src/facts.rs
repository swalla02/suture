//! What a move does, as facts a Suture strategy reads. Each fact looks one
//! move ahead and no further: deeper play is the strategy's job, or the
//! decision model's. Material is in pawns.

use crate::{Board, Color, Kind, Move};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    /// The moving piece: pawn, knight, bishop, rook, queen or king.
    pub piece: &'static str,
    /// The value of what it takes.
    pub captures: i32,
    pub check: bool,
    pub mate: bool,
    /// Leaves the side to move no move and no check: a draw.
    pub stalemate: bool,
    /// The most the other side can win with one capture next.
    pub hangs: i32,
    /// The most this side could win with one capture after that, if the
    /// other side did nothing.
    pub threat: i32,
    /// The other side has a mate in one after it.
    pub allows_mate: bool,
    pub castles: bool,
    pub promotes: bool,
    /// A knight or bishop off its home rank.
    pub develops: bool,
    /// To d4, e4, d5 or e5.
    pub center: bool,
    /// A pawn's rank counted from its own side (2 to 8), after the move; 0
    /// for other pieces.
    pub pawn_rank: i32,
    /// This side's material minus the other's, before the move.
    pub balance: i32,
}

/// The material `c` stands to lose to one capture, by the usual exchange
/// rule of thumb: all of a piece no one defends, or the difference when a
/// cheaper piece attacks it.
pub fn loose(b: &Board, c: Color) -> i32 {
    let mut worst = 0;
    for s in 0..64u8 {
        let Some(p) = b.at(s) else { continue };
        if p.color != c || p.kind == Kind::King {
            continue;
        }
        let attackers: Vec<u8> = b.attackers(s, c.other()).collect();
        if attackers.is_empty() {
            continue;
        }
        let defended = b.attackers(s, c).next().is_some();
        let cheapest = attackers.iter().filter_map(|&a| b.at(a)).filter(|a| !(defended && a.kind == Kind::King)).map(|a| if a.kind == Kind::King { 0 } else { a.kind.value() }).min();
        let loss = match (defended, cheapest) {
            (_, None) => 0,
            (false, Some(_)) => p.kind.value(),
            (true, Some(v)) => (p.kind.value() - v).max(0),
        };
        worst = worst.max(loss);
    }
    worst
}

pub fn material(b: &Board, c: Color) -> i32 {
    (0..64u8).filter_map(|s| b.at(s)).filter(|p| p.color == c).map(|p| p.kind.value()).sum()
}

pub fn facts(b: &Board, m: Move) -> Facts {
    let us = b.turn;
    let p = b.at(m.from).expect("a move from a piece");
    let after = b.play(m);
    let replies = after.legal();
    let captures = match b.at(m.to) {
        Some(q) => q.kind.value(),
        None if p.kind == Kind::Pawn && m.from % 8 != m.to % 8 => 1,
        None => 0,
    };
    let check = after.in_check(us.other());
    let mate = replies.is_empty() && check;
    let stalemate = replies.is_empty() && !check;
    let allows_mate = !replies.is_empty()
        && replies.iter().any(|r| {
            let next = after.play(*r);
            next.in_check(us) && next.legal().is_empty()
        });
    // The other side passing, to see what this side would then threaten.
    let mut pass = after.clone();
    pass.turn = us;
    let rank = if us == Color::White { (m.to / 8) as i32 + 1 } else { 8 - (m.to / 8) as i32 };
    let home = if us == Color::White { 0 } else { 7 };
    Facts {
        piece: p.kind.name(),
        captures: captures + m.promo.map_or(0, |k| k.value() - 1),
        check,
        mate,
        stalemate,
        hangs: if mate { 0 } else { loose(&after, us) },
        threat: if mate || pass.in_check(us.other()) { 0 } else { loose(&pass, us.other()) },
        allows_mate,
        castles: p.kind == Kind::King && m.from.abs_diff(m.to) == 2,
        promotes: m.promo.is_some(),
        develops: matches!(p.kind, Kind::Knight | Kind::Bishop) && m.from / 8 == home && m.to / 8 != home,
        center: matches!(m.to, 27 | 28 | 35 | 36),
        pawn_rank: if p.kind == Kind::Pawn { rank } else { 0 },
        balance: material(b, us) - material(b, us.other()),
    }
}
