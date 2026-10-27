# The chess ladder strategy

`bot.st` is the rule layer over a decision model. Every legal move goes into a
pool as a `Move` with its facts and the model's `score`. The facts come from
`eval/chess/engine/src/facts.rs` and look at most one reply deep, with no
search. The `prefer Move:` chain then picks one move. The ratings are in
`../RESULTS.md`; how the strategy got here is in git history.

## What it does, in coach's terms

1. **Mate when you can.** `those that are mating`.
2. **Never allow a mate in one or a stalemate.** `those that are sound`.
3. **Don't repeat unless you are behind.** `those that are not wasteful`.
4. **Never give up more than a pawn, by our count.** `highest material
   within 1.00 EUR`.
   - `material` is `gains - drops - 3 × trapped`. `gains` and `drops` say
     what the move wins or loses after the other side's best reply and this
     side's best capture after that. Each exchange on a square is settled
     cheapest attacker first.
   - A check in reply that takes nothing only delays a threat, so the
     threat still counts.
   - `trapped`: some checking reply can only be blocked, which is the
     back-rank mate one move past the horizon. It costs three pawns.
5. **Stay out of checks that leave the king one answer.** `those that are
   not cornered`.
6. **Then trust the model.** Take its near-favourites (`highest score
   within 100`, a tenth of its scale), and among those the most material.
   Then its own favourite.
7. **The plan breaks what is left.** Castle and develop early, keep king
   cover, press the squares round their king, push passed pawns late, keep
   your king central and close in when ahead, trade when ahead, and don't
   leave pieces loose or on the rim. With a model it only breaks exact
   ties. With no model it is the whole positional game.

The margin is one pawn because one-reply material is often wrong by a pawn:
the model knows a pawn sacrifice or a pawn's real price better. The chain
was chosen offline, by the average centipawn loss of its pick against
Stockfish at depth 9. It took maia1900 from 62 to 53, and maia1100 from 86
to 60.

## What did not work

- **A blended `material + weight × score`, then the plan.** Every blend let
  the model hang pieces or overrode it too often. A plan at even a quarter
  weight cost Maia games.
- **Seeking a draw when 3 pawns down.** It fired in positions that were
  lost on material but won on attack.
- **Fork and threat bonuses.** `gains` already counts a fork after the
  reply, so the bonus counted it twice.
- **Penalising every check reply that leaves one answer.** Many of those
  checks are harmless. Only the narrower `trapped`, and `cornered` as a
  late filter, paid.

The losses left need two replies of sight: mates in two or more, quiet
attacks on our king, and pawn endgames that need counting.

## Language gaps it hit

1. **Signed numbers.** Money stands in (1 EUR is a pawn), and each signed
   fact is split into two Counts (`gains`/`drops`, `up`/`down`).
2. **Conditional criteria.** For example, *"then, when it is far ahead,
   lowest boxed"*. There is no comparison inside arithmetic.
3. **Comparing with one candidate.** For example, *"material at least the
   model's favourite's minus 1"*. Margins are only relative to the tier's
   best.
4. **A margin as a share of the best.** *"highest score within a tenth of
   the best"*: the margin is a fixed 100.
5. **max/min in formulas, and named weights.** The adapter computes them,
   and the weights are bare literals.
6. **The 4096-byte unit.** The Move's 34 facts and 2 formulas fit, with
   little room for more.
