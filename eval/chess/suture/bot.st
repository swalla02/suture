-- The chess ladder's strategy (eval/chess). Each turn the harness enters
-- every legal move as a Move with its facts and the decision model's score,
-- then a Turn; the Turn's Pick takes one Move, ranked by `prefer Move`.
-- This is the seed strategy; the rule writer grows it.
module chessbot version 1, currency EUR, candidates 256

role engine

-- Material is in pawns. `hangs`: the most the other side wins with one
-- capture next; `threat`: the most this side could win with one capture
-- after that; `exposing`: the other side has a mate in one after it.
-- `score` is the decision model's, 0 to 1000; `rank` its order, from 1.
kind Move { ply: Count, uci: Text, piece: Text, captures: Count, checking: Bool, mating: Bool, stalemating: Bool, hangs: Count, threat: Count, exposing: Bool, castling: Bool, promoting: Bool, developing: Bool, central: Bool, pawnrank: Count, ahead: Bool, behind: Bool, opening: Bool, endgame: Bool, repeating: Bool, score: Count, rank: Count }
kind Turn { ply: Count }
kind Pick { turn: Turn }

Turn needs Pick
Pick uses a Move where the Move's ply is the Turn's ply

define: a Move is safe when its hangs is at most its captures and it is not exposing
define: a Move is drawing when it is stalemating or it is repeating

prefer Move: those that are mating, then those that are not exposing, then those whose hangs is at most its captures, then highest score

admit enter a new Move by engine
admit start a new Turn by engine
