-- The chess ladder's strategy (eval/chess). Each turn the harness enters
-- every legal move as a Move with its facts and the decision model's score,
-- then a Turn; the Turn's Pick takes one Move, ranked by `prefer Move`.
-- The harness enters only the attributes this Move declares; the full list
-- is `attributes` in eval/chess/ladder/src/players.rs, and STRATEGY.md says
-- what was tried and what the games kept.
module chessbot version 1, currency EUR, candidates 256

role engine

kind Move {
  ply: Count,
  uci: Text,
  -- Safety, all one reply deep.
  mating: Bool,
  -- the other side then has a mate in one
  exposing: Bool,
  stalemating: Bool,
  -- the position after it has been seen before in this game
  repeating: Bool,
  -- some check in reply leaves this side's king no move and no capture of
  -- the checker, only blocks: the back-rank mate one move later
  trapped: Bool,
  -- some check in reply leaves this side's king at most one answer
  cornered: Bool,
  -- Material, in pawns: what the move wins or loses after the other side's
  -- best reply and this side's best capture then, every exchange settled
  -- cheapest attacker first; a check that takes nothing only delays.
  gains: Count,
  drops: Count,
  -- pawns ahead or behind before the move
  up: Count,
  down: Count,
  -- The decision model's score, 0 to 1000.
  score: Count,
  -- The plan, for when the model can't tell moves apart.
  -- this side's moves after it, and the other side's replies
  mobility: Count,
  replies: Count,
  -- pawns before this side's king; squares by their king this side attacks;
  -- squares by this side's king they attack
  shield: Count,
  pressure: Count,
  peril: Count,
  -- a passed pawn's rank if it pushes one
  pushes: Count,
  -- this king's nearness to the edge; the kings' distance; their king's moves
  kingedge: Count,
  kings: Count,
  boxed: Count,
  -- this side's undefended pieces; a piece put on the edge; an even trade
  loose: Count,
  rim: Count,
  trading: Bool,
  checking: Bool,
  castling: Bool,
  developing: Bool,
  undeveloping: Bool,
  central: Bool,
  queenmoves: Count,
  kingmoves: Count,
  -- the first 20 plies; few pieces left
  opening: Bool,
  endgame: Bool,
  -- Money stands in for a signed number: 1 EUR is a pawn.
  material: Money = gains * 1 EUR - drops * 1 EUR - trapped * 3 EUR,
  plan: Money = mobility * 0.03 EUR - replies * 0.02 EUR + opening * (castling * 0.50 EUR + developing * 0.20 EUR + central * 0.20 EUR - kingmoves * 0.50 EUR - queenmoves * 0.15 EUR - undeveloping * 0.20 EUR) + (1 - endgame) * (shield * 0.06 EUR + pressure * 0.06 EUR - peril * 0.08 EUR) + endgame * (pushes * 0.10 EUR - kingedge * 0.10 EUR - up * (kings * 0.02 EUR + boxed * 0.02 EUR)) - loose * 0.05 EUR + checking * 0.05 EUR - rim * 0.08 EUR + up * trading * 0.05 EUR
}
kind Turn { ply: Count }
kind Pick { turn: Turn }

Turn needs Pick
Pick uses a Move where the Move's ply is the Turn's ply

define: a Move is sound when it is not exposing and it is not stalemating
define: a Move is wasteful when it is repeating and its down is 0

-- Mate first; never allow a mate in one or stalemate; repeat only when
-- behind. Then material: the moves within a pawn of the most, since one
-- pawn is inside what one reply deep can judge and the model knows better.
-- Among those, keep out of checks that leave the king one answer; then
-- the model's near-favourites (within a tenth of its scale), and among
-- them the most material; then the model; the plan breaks the ties a model
-- without scores leaves.
prefer Move: those that are mating, then those that are sound, then those that are not wasteful, then highest material within 1.00 EUR, then those that are not cornered, then highest score within 100, then highest material, then highest score, then highest plan

admit enter a new Move by engine
admit start a new Turn by engine
