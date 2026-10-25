//! What a move does, as facts a Suture strategy reads. Each fact looks at
//! the move and at the other side's replies to it, one reply deep and no
//! further; what a position holds after that is judged statically, by the
//! exchange on a square (`see`). Deeper play is the strategy's job, or the
//! decision model's. Material is in pawns.

use crate::{Board, Color, Kind, Move};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    /// The moving piece: pawn, knight, bishop, rook, queen or king.
    pub piece: &'static str,
    /// The value of what it takes, and of what a promotion adds.
    pub captures: i32,
    pub check: bool,
    pub mate: bool,
    /// Leaves the side to move no move and no check: a draw.
    pub stalemate: bool,
    /// The most the other side can win with one capture next, counting the
    /// exchange that follows on that square.
    pub hangs: i32,
    /// The most this side could win with one capture after that, if the
    /// other side did nothing.
    pub threat: i32,
    /// A double attack: the second most this side could win, if the other
    /// side did nothing, on another square. It can save only one.
    pub fork: i32,
    /// The material this move wins (positive) or loses (negative) once the
    /// other side has made its best reply and this side its best capture
    /// after it, each capture judged by the exchange on its square.
    pub outcome: i32,
    /// As `outcome`, but a check in reply that takes nothing does not undo
    /// what this side threatens: it is answered, and the threat stays.
    pub outlook: i32,
    /// The other side has a mate in one after it.
    pub allows_mate: bool,
    /// Some check in reply leaves this side's king at most one answer: a
    /// mating net may be closing.
    pub cornered: bool,
    /// Some check in reply can only be blocked: the king cannot move and
    /// the checking piece cannot be taken. A back-rank mate often follows.
    pub trapped: bool,
    /// The squares beside this side's king that it could step to after the
    /// move: empty or the other side's, and not attacked.
    pub luft: i32,
    /// What the other side could win with one capture before the move: a
    /// piece of this side is attacked.
    pub danger: i32,
    pub castles: bool,
    pub promotes: bool,
    /// A knight or bishop off its home rank.
    pub develops: bool,
    /// A knight or bishop back to its home rank.
    pub undevelops: bool,
    /// To d4, e4, d5 or e5.
    pub center: bool,
    /// A pawn's rank counted from its own side (2 to 8), after the move; 0
    /// for other pieces.
    pub pawn_rank: i32,
    /// A pawn with no pawn of the other side ahead of it on its file or the
    /// files beside it, after the move.
    pub passed: bool,
    /// A capture of a piece (not a pawn) worth what the capturing piece is.
    pub trades: bool,
    /// How many moves the other side's king has after the move.
    pub boxed: i32,
    /// The distance, in king steps, between the two kings after the move.
    pub kings: i32,
    /// How far this side's king is from the centre after the move (0 to 3).
    pub king_edge: i32,
    /// How far the other side's king is from the centre after the move.
    pub their_edge: i32,
    /// How many replies the other side has.
    pub replies: i32,
    /// How many moves this side would have next, if the other side passed;
    /// 0 after a check.
    pub mobility: i32,
    /// This side's knights, bishops, rooks and queens that nothing of its
    /// own defends, after the move.
    pub loose: i32,
    /// This side's pawns in front of its king, after the move (0 to 6).
    pub shield: i32,
    /// The squares beside the other side's king that this side attacks.
    pub pressure: i32,
    /// The squares beside this side's king that the other side attacks.
    pub peril: i32,
    /// How far a knight, bishop or queen lands from the centre (0 to 3); 0
    /// for other pieces.
    pub rim: i32,
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

/// What `by` wins by starting the captures on `s`: the static exchange.
/// Each side takes with its cheapest piece and stops when going on would
/// lose; pieces behind the first come into play as it leaves. Pins are not
/// seen. A king takes only where no piece of the other side still attacks.
pub fn see(b: &Board, s: u8, by: Color) -> i32 {
    let Some(target) = b.at(s) else { return 0 };
    if target.color == by || target.kind == Kind::King {
        return 0;
    }
    let order = |k: Kind| if k == Kind::King { 100 } else { k.value() };
    let Some((from, p)) = b.attackers(s, by).filter_map(|a| b.at(a).map(|p| (a, p))).min_by_key(|(_, p)| order(p.kind)) else { return 0 };
    let mut next = b.clone();
    next.sq[s as usize] = Some(p);
    next.sq[from as usize] = None;
    if p.kind == Kind::King && next.attacked(s, by.other()) {
        return 0;
    }
    (target.kind.value() - see(&next, s, by.other())).max(0)
}

/// The most the side to move wins with one legal capture or promotion,
/// counting the exchange that follows on that square.
pub fn best_capture(b: &Board) -> i32 {
    let us = b.turn;
    let mut moves = Vec::with_capacity(48);
    b.pseudo(&mut moves);
    let mut best = 0;
    for m in moves {
        let taken = match b.at(m.to) {
            Some(q) if q.kind == Kind::King => continue,
            Some(q) => q.kind.value(),
            None if b.at(m.from).is_some_and(|p| p.kind == Kind::Pawn) && m.from % 8 != m.to % 8 => 1,
            None => 0,
        };
        let promo = m.promo.map_or(0, |k| k.value() - 1);
        if taken + promo <= best {
            continue;
        }
        let next = b.play(m);
        if next.in_check(us) {
            continue;
        }
        best = best.max(taken + promo - see(&next, m.to, us.other()));
    }
    best
}

/// The two largest exchanges `by` could start against `c`'s pieces.
fn targets(b: &Board, by: Color) -> (i32, i32) {
    let mut v: Vec<i32> = (0..64u8).filter(|&s| b.at(s).is_some_and(|p| p.color != by && p.kind != Kind::King)).map(|s| see(b, s, by)).filter(|&g| g > 0).collect();
    v.sort_unstable_by(|a, b| b.cmp(a));
    (v.first().copied().unwrap_or(0), v.get(1).copied().unwrap_or(0))
}

/// Squares from the centre: 0 on d4–e5, 3 on the edge.
fn edge(s: u8) -> i32 {
    let (f, r) = ((s % 8) as i32, (s / 8) as i32);
    (3 - f).max(f - 4).max((3 - r).max(r - 4))
}

fn distance(a: u8, b: u8) -> i32 {
    ((a % 8) as i32 - (b % 8) as i32).abs().max(((a / 8) as i32 - (b / 8) as i32).abs())
}

/// No pawn of the other side ahead of the pawn on `s`, on its file or beside.
fn is_passed(b: &Board, s: u8, c: Color) -> bool {
    let (f, r) = ((s % 8) as i32, (s / 8) as i32);
    let them = crate::Piece { color: c.other(), kind: Kind::Pawn };
    !(0..64u8).any(|t| {
        let (tf, tr) = ((t % 8) as i32, (t / 8) as i32);
        (tf - f).abs() <= 1 && (if c == Color::White { tr > r } else { tr < r }) && b.at(t) == Some(them)
    })
}

pub fn facts(b: &Board, m: Move) -> Facts {
    let us = b.turn;
    let p = b.at(m.from).expect("a move from a piece");
    let after = b.play(m);
    let replies = after.legal();
    let taken = match b.at(m.to) {
        Some(q) => q.kind.value(),
        None if p.kind == Kind::Pawn && m.from % 8 != m.to % 8 => 1,
        None => 0,
    };
    let captures = taken + m.promo.map_or(0, |k| k.value() - 1);
    let check = after.in_check(us.other());
    let mate = replies.is_empty() && check;
    let stalemate = replies.is_empty() && !check;
    let mut allows_mate = false;
    let mut cornered = false;
    let mut trapped = false;
    // Their best reply against this side's best capture after it.
    let (mut worst, mut patient) = (i32::MAX, i32::MAX);
    for r in &replies {
        let next = after.play(*r);
        let checked = next.in_check(us);
        if checked {
            let answers = next.legal();
            allows_mate |= answers.is_empty();
            cornered |= answers.len() <= 1;
            let king = next.king(us);
            trapped |= !answers.is_empty() && answers.iter().all(|a| Some(a.from) != king && a.to != r.to);
        }
        let lost = match after.at(r.to) {
            Some(q) => q.kind.value(),
            None if after.at(r.from).is_some_and(|q| q.kind == Kind::Pawn) && r.from % 8 != r.to % 8 => 1,
            None => 0,
        } + r.promo.map_or(0, |k| k.value() - 1);
        let now = best_capture(&next);
        worst = worst.min(now - lost);
        // A check that takes nothing only puts off what this side threatens.
        patient = patient.min(if checked && lost == 0 { now.max(targets(&next, us).0) } else { now - lost });
    }
    let outcome = captures + if replies.is_empty() { 0 } else { worst };
    let outlook = captures + if replies.is_empty() { 0 } else { patient };
    // The other side passing, to see what this side would then threaten.
    let mut pass = after.clone();
    pass.turn = us;
    pass.ep = None;
    let quiet = !(mate || check);
    let (threat, fork) = if quiet { (best_capture(&pass), targets(&pass, us).1) } else { (0, 0) };
    let mut before = b.clone();
    before.turn = us.other();
    before.ep = None;
    let danger = if b.in_check(us) { 0 } else { best_capture(&before) };
    let rank = if us == Color::White { (m.to / 8) as i32 + 1 } else { 8 - (m.to / 8) as i32 };
    let home = if us == Color::White { 0 } else { 7 };
    let (ours, theirs) = (after.king(us).unwrap_or(0), after.king(us.other()).unwrap_or(0));
    Facts {
        piece: p.kind.name(),
        captures,
        check,
        mate,
        stalemate,
        hangs: if mate { 0 } else { best_capture(&after) },
        threat,
        fork,
        outcome,
        outlook,
        allows_mate,
        cornered,
        trapped,
        luft: crate::KING.iter().filter_map(|&d| crate::step(ours, d)).filter(|&t| after.at(t).is_none_or(|q| q.color != us) && !after.attacked(t, us.other())).count() as i32,
        danger,
        castles: p.kind == Kind::King && m.from.abs_diff(m.to) == 2,
        promotes: m.promo.is_some(),
        develops: matches!(p.kind, Kind::Knight | Kind::Bishop) && m.from / 8 == home && m.to / 8 != home,
        undevelops: matches!(p.kind, Kind::Knight | Kind::Bishop) && m.from / 8 != home && m.to / 8 == home,
        center: matches!(m.to, 27 | 28 | 35 | 36),
        pawn_rank: if p.kind == Kind::Pawn { rank } else { 0 },
        passed: p.kind == Kind::Pawn && m.promo.is_none() && is_passed(&after, m.to, us),
        trades: taken >= 3 && taken == p.kind.value(),
        boxed: replies.iter().filter(|r| r.from == theirs).count() as i32,
        kings: distance(ours, theirs),
        king_edge: edge(ours),
        their_edge: edge(theirs),
        replies: replies.len() as i32,
        mobility: if quiet { pass.legal().len() as i32 } else { 0 },
        loose: (0..64u8).filter(|&s| after.at(s).is_some_and(|q| q.color == us && !matches!(q.kind, Kind::Pawn | Kind::King)) && !after.attacked(s, us)).count() as i32,
        shield: shield(&after, ours, us),
        pressure: crate::KING.iter().filter_map(|&d| crate::step(theirs, d)).filter(|&t| after.attacked(t, us)).count() as i32,
        peril: crate::KING.iter().filter_map(|&d| crate::step(ours, d)).filter(|&t| after.attacked(t, us.other())).count() as i32,
        rim: if matches!(p.kind, Kind::Knight | Kind::Bishop | Kind::Queen) { edge(m.to) } else { 0 },
        balance: material(b, us) - material(b, us.other()),
    }
}

/// This side's pawns on the three squares in front of its king, and the
/// three in front of those.
fn shield(b: &Board, king: u8, c: Color) -> i32 {
    let ahead: i8 = if c == Color::White { 1 } else { -1 };
    let own = crate::Piece { color: c, kind: Kind::Pawn };
    [(-1, 1), (0, 1), (1, 1), (-1, 2), (0, 2), (1, 2)].iter().filter_map(|&(df, dr)| crate::step(king, (df, dr * ahead))).filter(|&t| b.at(t) == Some(own)).count() as i32
}
