use chess_engine::Board;
use chess_ladder::elo::{Game, fit};
use chess_ladder::game::{self, Player, Result_};
use chess_ladder::players::{Flat, Random, Rng, Sutured, named};
use chess_engine::Move;

/// Plays a fixed list of moves, then gives up.
struct Script(Vec<&'static str>, usize);

impl Player for Script {
    fn name(&self) -> String {
        "script".into()
    }
    fn new_game(&mut self) {
        self.1 = 0;
    }
    fn choose(&mut self, g: &game::Game) -> Result<Move, String> {
        let m = self.0.get(self.1).ok_or("out of moves")?;
        self.1 += 1;
        g.board().parse_move(m).ok_or(format!("{m} is not legal"))
    }
}

fn strategy() -> suture_lang::ir::Program {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../suture");
    suture_lang::compile_dir(&dir).unwrap_or_else(|e| panic!("{:?}", e.iter().map(|e| &e.msg).collect::<Vec<_>>()))
}

#[test]
fn a_game_ends_in_mate() {
    let mut w = Script(vec!["f2f3", "g2g4"], 0);
    let mut b = Script(vec!["e7e5", "d8h4"], 0);
    let o = game::play(&mut w, &mut b, Board::start(), 300);
    assert_eq!((o.result, o.reason.as_str(), o.moves.len()), (Result_::Black, "mate", 4));
}

#[test]
fn a_player_out_of_moves_loses() {
    let mut w = Script(vec!["e2e4"], 0);
    let mut b = Script(vec!["e7e5"], 0);
    let o = game::play(&mut w, &mut b, Board::start(), 300);
    assert_eq!(o.result, Result_::Black);
    assert!(o.reason.starts_with("script failed"), "{}", o.reason);
}

#[test]
fn repetition_and_the_move_limit_draw() {
    let shuffle = || Script(vec!["g1f3", "f3g1", "g1f3", "f3g1", "g1f3"], 0);
    let mut w = shuffle();
    let mut b = Script(vec!["g8f6", "f6g8", "g8f6", "f6g8", "g8f6"], 0);
    let o = game::play(&mut w, &mut b, Board::start(), 300);
    assert_eq!((o.result, o.reason.as_str()), (Result_::Draw, "repetition"));
    let o = game::play(&mut shuffle(), &mut Random(Rng::new(3)), Board::start(), 2);
    assert_eq!((o.result, o.reason.as_str()), (Result_::Draw, "move limit"));
}

#[test]
fn the_openings_are_legal() {
    for i in 0..game::OPENINGS.len() {
        assert!(!game::opening(i).legal().is_empty());
    }
}

#[test]
fn the_strategy_takes_a_mate_in_one() {
    // White to play Qxf7#.
    let b = Board::from_fen("r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5Q2/PPPP1PPP/RNB1K1NR w KQkq - 0 1").unwrap();
    let mut p = Sutured::new(strategy(), Box::new(Flat), Rng::new(1));
    p.new_game();
    let m = p.choose(&game::Game::new(b)).unwrap();
    assert_eq!(m.to_string(), "f3f7");
}

#[test]
fn the_strategy_beats_a_random_mover() {
    let mut s = Sutured::new(strategy(), Box::new(Flat), Rng::new(5));
    let o = game::play(&mut s, &mut Random(Rng::new(9)), game::opening(0), 300);
    assert_eq!(o.result, Result_::White, "{} after {} plies", o.reason, o.moves.len());
}

#[test]
fn an_answer_names_legal_moves_in_order() {
    let b = Board::start();
    let legal = b.legal();
    let picks = named("1. e2e4, then D2D4 or z9z9; e2e4 again, g1f3 b1c3", &legal);
    let names: Vec<String> = picks.iter().map(|&i| legal[i].to_string()).collect();
    assert_eq!(names, ["e2e4", "d2d4", "g1f3"]);
}

#[test]
fn ratings_hold_the_anchor_and_order_the_rest() {
    let g = |w: &str, b: &str, s: f64| Game { white: w.into(), black: b.into(), score: s };
    let mut games = vec![];
    for _ in 0..30 {
        games.push(g("strong", "stockfish 1500", 0.75));
        games.push(g("stockfish 1500", "weak", 0.75));
    }
    let r = fit(&games);
    assert_eq!(r["stockfish 1500"].elo, 1500.0);
    assert!(r["strong"].elo > 1550.0 && r["strong"].elo < 1750.0, "{}", r["strong"].elo);
    assert!(r["weak"].elo < 1450.0 && r["weak"].elo > 1250.0, "{}", r["weak"].elo);
    assert!(r["strong"].ci > 50.0 && r["strong"].ci < 200.0);
}

#[test]
fn a_decision_answer_scores_each_move_by_its_probability() {
    let b = Board::start();
    let moves: Vec<Move> = ["e2e4", "d2d4", "g1f3"].iter().map(|m| b.parse_move(m).unwrap()).collect();
    let v = serde_json::json!({"answers": {"move": {"type": "choice", "choice": "e2e4", "probabilities": {"e2e4": 0.61, "d2d4": 0.39}}}});
    assert_eq!(chess_ladder::players::decided(&v, &moves).unwrap(), vec![610, 390, 0]);
    assert!(chess_ladder::players::decided(&serde_json::json!({"error": {"code": 400}}), &moves).is_err());
}
