-- The chess ladder's strategy (eval/chess). Each turn the harness enters
-- every legal move as a Move with its facts and the decision model's score,
-- then a Turn; the Turn's Pick takes one Move, ranked by `prefer Move`.
module chessbot version 1, currency EUR, candidates 256

role engine

-- Material is in pawns. `hangs`: the most the other side wins with one
-- capture next; `threat`: the most this side could win with one capture
-- after that; `fork`: the second such target; `wins`/`loses`: the material
-- the move wins or loses after the other side's best reply and this side's
-- best capture after it; `exposing`: the other side has a mate in one.
-- `score` is the decision model's, 0 to 1000; `rank` its order, from 1.
-- The Count attributes from `checks` to `late` are facts as 1 or 0.
kind Move { ply: Count, uci: Text, piece: Text, captures: Count, checking: Bool, mating: Bool, stalemating: Bool, hangs: Count, threat: Count, fork: Count, wins: Count, loses: Count, gains: Count, drops: Count, danger: Count, cornered: Bool, luft: Count, exposing: Bool, castling: Bool, promoting: Bool, developing: Bool, undeveloping: Bool, central: Bool, pawnrank: Count, passed: Bool, trading: Bool, boxed: Count, kings: Count, kingedge: Count, theiredge: Count, replies: Count, up: Count, down: Count, ahead: Bool, behind: Bool, opening: Bool, endgame: Bool, repeating: Bool, checks: Count, castles: Count, develops: Count, undevelops: Count, kingmoves: Count, queenmoves: Count, pushes: Count, trades: Count, nets: Count, stalemates: Count, repeats: Count, traps: Count, early: Count, late: Count, score: Count, rank: Count, material: Money = gains * 1 EUR - drops * 1 EUR, plan: Money = early * (castles * 0.40 EUR + develops * 0.15 EUR - undevelops * 0.20 EUR - kingmoves * 0.40 EUR - queenmoves * 0.10 EUR) + late * (pushes * 0.08 EUR - kingedge * 0.08 EUR - up * (kings * 0.02 EUR + boxed * 0.02 EUR)) + fork * 0.15 EUR + threat * 0.05 EUR - nets * 0.60 EUR, value: Money = material + score * 1.50 EUR / 1000 - traps * 3 EUR }
kind Turn { ply: Count }
kind Pick { turn: Turn }

Turn needs Pick
Pick uses a Move where the Move's ply is the Turn's ply

prefer Move: those that are mating, then those that are not exposing, then those that are not stalemating, then those that are not repeating or its down is at least 1, then highest value

admit enter a new Move by engine
admit start a new Turn by engine
