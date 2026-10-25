-- The chess ladder's strategy (eval/chess). Each turn the harness enters
-- every legal move as a Move with its facts and the decision model's score,
-- then a Turn; the Turn's Pick takes one Move, ranked by `prefer Move`.
-- The harness enters only the attributes this Move declares; the full list
-- is `attributes` in eval/chess/ladder/src/players.rs, and STRATEGY.md says
-- what was tried and what the games kept.
module chessbot version 1, currency EUR, candidates 256

role engine

-- Material is in pawns, Money standing in for a signed number (1 EUR = 1
-- pawn). `gains`/`drops`: what the move wins or loses after the other
-- side's best reply and this side's best capture then (a check that takes
-- nothing counts as only putting off this side's threat). `exposing`: the
-- other side then has a mate in one. `traps`: some check in reply leaves
-- this side's king no move and no capture of the checker, only blocks
-- (the back-rank mate one move later). `down`: pawns behind before the
-- move. `score` is the decision model's, 0 to 1000.
-- The plan, a tiebreak after `value`: `mobility` is this side's moves
-- after it, `replies` the other side's; `shield` pawns before this side's
-- king, `pressure` squares by their king this side attacks, `peril` squares
-- by this side's king they attack; `pushes` a passed pawn's rank; `kingedge`
-- this king's nearness to the edge, `kings` the kings' distance, `boxed`
-- their king's moves; `loose` this side's undefended pieces; `rim` a piece
-- on the edge; `trades` an even trade. The Counts `checks` to `late` are
-- facts as 1 or 0: `early` the first 20 plies, `late` few pieces left.
kind Move { ply: Count, uci: Text, mating: Bool, exposing: Bool, stalemating: Bool, repeating: Bool, down: Count, up: Count, gains: Count, drops: Count, traps: Count, score: Count, mobility: Count, replies: Count, shield: Count, pressure: Count, peril: Count, pushes: Count, kingedge: Count, kings: Count, boxed: Count, loose: Count, rim: Count, trades: Count, checks: Count, castles: Count, develops: Count, centers: Count, kingmoves: Count, queenmoves: Count, undevelops: Count, early: Count, late: Count, material: Money = gains * 1 EUR - drops * 1 EUR, value: Money = material + score * 1.50 EUR / 1000 - traps * 3 EUR, plan: Money = mobility * 0.03 EUR - replies * 0.02 EUR + early * (castles * 0.50 EUR + develops * 0.20 EUR + centers * 0.20 EUR - kingmoves * 0.50 EUR - queenmoves * 0.15 EUR - undevelops * 0.20 EUR) + (1 - late) * (shield * 0.06 EUR + pressure * 0.06 EUR - peril * 0.08 EUR) + late * (pushes * 0.10 EUR - kingedge * 0.10 EUR - up * (kings * 0.02 EUR + boxed * 0.02 EUR)) - loose * 0.05 EUR + checks * 0.05 EUR - rim * 0.08 EUR + up * trades * 0.05 EUR }
kind Turn { ply: Count }
kind Pick { turn: Turn }

Turn needs Pick
Pick uses a Move where the Move's ply is the Turn's ply

-- Mate first; never allow a mate in one; never stalemate; repeat only when
-- behind; then material after the best reply, with the model's score worth
-- up to a pawn and a half, and a back-rank net worth three; then the plan.
prefer Move: those that are mating, then those that are not exposing, then those that are not stalemating, then those that are not repeating or its down is at least 1, then highest value, then highest plan

admit enter a new Move by engine
admit start a new Turn by engine
