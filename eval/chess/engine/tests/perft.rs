//! The move generator against the published perft counts
//! (chessprogramming.org, "Perft Results"). The depths are kept small enough
//! to run in a debug build.

use chess_engine::{Board, START};

fn perft(fen: &str, counts: &[u64]) {
    let b = Board::from_fen(fen).unwrap();
    for (d, &n) in counts.iter().enumerate() {
        assert_eq!(b.perft(d as u32 + 1), n, "{fen} at depth {}", d + 1);
    }
}

#[test]
fn the_start_position() {
    perft(START, &[20, 400, 8_902, 197_281]);
}

#[test]
fn kiwipete_castling_promotion_and_pins() {
    perft("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", &[48, 2_039, 97_862]);
}

#[test]
fn position_3_en_passant_and_checks() {
    perft("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", &[14, 191, 2_812, 43_238]);
}

#[test]
fn position_4_promotions_into_check() {
    perft("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1", &[6, 264, 9_467]);
}

#[test]
fn position_5() {
    perft("rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8", &[44, 1_486, 62_379]);
}

#[test]
fn fen_reads_back_and_moves_parse() {
    let b = Board::start();
    assert_eq!(b.fen(), START);
    let e4 = b.parse_move("e2e4").unwrap();
    assert_eq!(b.play(e4).fen(), "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1");
    let b = Board::from_fen("4k3/8/8/8/3p4/8/4P3/4K3 w - - 0 1").unwrap();
    assert_eq!(b.play(b.parse_move("e2e4").unwrap()).fen(), "4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1");
    assert!(b.parse_move("e2e5").is_none());
}

#[test]
fn rights_without_their_pieces_and_bad_fens() {
    for fen in ["4k3/8/8/8/8/8/8/4K3 w K - 0 1", "4k3/8/8/8/8/8/8/5K2 w K - 0 1"] {
        let b = Board::from_fen(fen).unwrap();
        assert!(b.legal().iter().all(|m| m.to_string() != "e1g1"), "{fen}");
    }
    assert!(Board::from_fen("9999999999999999999999999999/8/8/8/8/8/8/8 w - - 0 1").is_err());
}
