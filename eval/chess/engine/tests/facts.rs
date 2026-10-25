use chess_engine::{Board, facts::facts};

fn of(fen: &str, uci: &str) -> chess_engine::facts::Facts {
    let b = Board::from_fen(fen).unwrap();
    facts(&b, b.parse_move(uci).unwrap_or_else(|| panic!("{uci} is not legal in {fen}")))
}

#[test]
fn captures_hanging_pieces_and_mates() {
    // Scholar's mate: Qxf7 takes a pawn and mates.
    let f = of("r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5Q2/PPPP1PPP/RNB1K1NR w KQkq - 0 4", "f3f7");
    assert!(f.mate && f.check && f.captures == 1 && f.hangs == 0, "{f:?}");
    // Qxa7 takes a pawn the rook on a8 defends: the queen hangs for a rook.
    let f = of("r3k3/p7/8/8/8/8/8/Q3K3 w - - 0 1", "a1a7");
    assert_eq!((f.captures, f.hangs), (1, 9), "{f:?}");
    // A knight to where a pawn takes it: all of it, then less when defended.
    let f = of("4k3/8/8/3p4/8/2N5/8/4K3 w - - 0 1", "c3e4");
    assert_eq!(f.hangs, 3, "{f:?}");
    let f = of("4k3/8/8/3p4/8/2N5/3P4/4K3 w - - 0 1", "c3e4");
    assert_eq!(f.hangs, 3, "{f:?}");
    let f = of("4k3/8/8/3p4/8/2N2P2/8/4K3 w - - 0 1", "c3e4");
    assert_eq!(f.hangs, 2, "{f:?}");
}

#[test]
fn threats_mate_in_one_against_and_development() {
    // Nd5 forks nothing but attacks an undefended rook on c7.
    let f = of("4k3/2r5/8/8/8/2N5/8/4K3 w - - 0 1", "c3d5");
    assert_eq!(f.threat, 5, "{f:?}");
    // Moving the f-pawn lets the queen mate on h4 (fool's mate).
    let f = of("rnbqkbnr/pppp1ppp/8/4p3/8/5P2/PPPPP1PP/RNBQKBNR w KQkq - 0 2", "g2g4");
    assert!(f.allows_mate, "{f:?}");
    let f = of(chess_engine::START, "g1f3");
    assert!(f.develops && !f.allows_mate && f.hangs == 0, "{f:?}");
    let f = of(chess_engine::START, "e2e4");
    assert!(f.center && f.pawn_rank == 4, "{f:?}");
}

#[test]
fn exchanges_count_every_piece_on_the_square() {
    use chess_engine::{Color, facts::see};
    // Rooks doubled on the d-file take a knight a rook defends: 3 - 5 + 5.
    let b = Board::from_fen("3r2k1/8/8/3n4/8/8/3R4/3RK3 w - - 0 1").unwrap();
    assert_eq!(see(&b, 35, Color::White), 3);
    // A pawn that a pawn defends is not worth a rook.
    let b = Board::from_fen("4k3/8/4p3/3p4/8/8/8/3RK3 w - - 0 1").unwrap();
    assert_eq!(see(&b, 35, Color::White), 0);
    // A king takes only what nothing defends.
    let b = Board::from_fen("4k3/8/8/8/8/8/3p4/4K3 w - - 0 1").unwrap();
    assert_eq!(see(&b, 11, Color::White), 1);
    let b = Board::from_fen("4k3/8/8/8/8/4p3/3p4/4K3 w - - 0 1").unwrap();
    assert_eq!(see(&b, 11, Color::White), 0);
}

#[test]
fn forks_and_what_a_move_wins_after_the_best_reply() {
    // Nc7+ forks the king and the rook: the rook falls whatever Black does.
    let f = of("r3k3/8/8/1N6/8/8/8/4K3 w - - 0 1", "b5c7");
    assert!(f.check && f.outcome == 5, "{f:?}");
    // d5 forks two knights: the king can guard both, so a knight for the pawn.
    let f = of("4k3/8/2n1n3/8/3P4/8/8/4K3 w - - 0 1", "d4d5");
    assert!(f.fork == 3 && f.outcome == 2, "{f:?}");
    // Qxa7 wins a pawn and loses the queen to the rook.
    let f = of("r3k3/p7/8/8/8/8/8/Q3K3 w - - 0 1", "a1a7");
    assert_eq!(f.outcome, 1 - 9, "{f:?}");
    // A queen a pawn attacks is in danger before the move, and after a king move.
    let f = of("4k3/8/8/2p5/3Q4/8/8/4K3 w - - 0 1", "e1e2");
    assert!(f.danger == 9 && f.hangs == 9 && f.outcome < 0, "{f:?}");
}

#[test]
fn back_rank_checks_that_can_only_be_blocked() {
    // Rxc8 wins the queen for the rook, but Rxc8+ can only be blocked by
    // Rd8, and Rxd8 mates: the move leaves the king trapped.
    let fen = "2Qr2k1/p4ppp/3r4/8/3pP3/3q2P1/P4P1P/2R3K1 b - - 3 26";
    let f = of(fen, "d8c8");
    assert!(f.trapped && f.outlook == 4 && !f.allows_mate, "{f:?}");
    // h6 gives the king a square: no check can only be blocked.
    let f = of(fen, "h7h6");
    assert!(!f.trapped, "{f:?}");
}

#[test]
fn a_check_that_takes_nothing_only_delays_a_threat() {
    // Nd5 attacks two loose rooks; Black's best is a check, Re4+, after which
    // the knight cannot take at once, but one rook still falls.
    let f = of("4k3/8/1r6/8/5r2/2N5/8/4K3 w - - 0 1", "c3d5");
    assert_eq!((f.fork, f.outcome), (5, 0), "{f:?}");
    assert!(f.outlook >= 2, "{f:?}");
}

#[test]
fn endgame_facts() {
    // A pawn no pawn of the other side stops, on its file or beside it.
    let f = of("7k/8/8/8/8/8/P7/K7 w - - 0 1", "a2a4");
    assert!(f.passed && f.pawn_rank == 4, "{f:?}");
    let f = of("7k/8/8/8/8/8/P5p1/K7 w - - 0 1", "a2a4");
    assert!(f.passed, "{f:?}");
    let f = of("7k/p7/8/8/8/8/1P6/K7 w - - 0 1", "b2b4");
    assert!(!f.passed, "{f:?}");
    // Qg6 leaves the cornered king no move: boxed in, and stalemate.
    let f = of("7k/8/8/8/8/8/8/K5Q1 w - - 0 1", "g1g6");
    assert_eq!((f.boxed, f.their_edge, f.kings), (0, 3, 7), "{f:?}");
    assert!(f.stalemate, "{f:?}");
    // Qg5 leaves it one: h7.
    let f = of("7k/8/8/8/8/8/8/K5Q1 w - - 0 1", "g1g5");
    assert_eq!(f.boxed, 1, "{f:?}");
}
