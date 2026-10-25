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
