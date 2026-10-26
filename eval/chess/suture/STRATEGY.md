# The chess ladder strategy

`bot.st` is the rule layer over a decision model. Every legal move goes into a
pool as a `Move` with facts (eval/chess/engine/src/facts.rs, at most one reply
deep, no search) and the model's `score`, and the `prefer Move:` chain picks
one. This file records what was tried, what the games kept, the ratings, and
where the language got in the way.

Round 1 (v6, written before slice 63) is the first part. Round 2 (v8, the
current `bot.st`, written in slice 63's forms) is the last section, and its
Elo table supersedes round 1's. Both are superseded by `../RESULTS.md`: every
arm replayed on the final `bot.st`, 80 games each, with the decision models
and LLMs added.

## Round 1 (v6), in coach's terms

This is the round 1 strategy. Round 2 (v8, the current `bot.st`) replaced
items 2, 6, 7 and 8; see "Round 2" below.

1. **Mate when you can.** `those that are mating`.
2. **Never allow a mate in one.** `those that are not exposing`.
3. **Never stalemate, and don't repeat unless you are behind.**
4. **Count material after their best reply, not before.** The main term is
   `gains - drops`: what the move wins after the other side's best reply and
   then this side's best capture, every exchange on a square settled by a
   static exchange (SEE) with the cheapest attacker first. This one number
   covers hanging a piece, taking a defended piece, leaving a piece en prise,
   missing a free piece, and forks (a quiet move that leaves two targets
   still wins one after the reply).
5. **A check that takes nothing doesn't save a piece.** In `gains`, a quiet
   check in reply only puts off what this side threatens, so the threat
   still counts. This stopped the bot from shying away from good forks
   because of a spite check.
6. **Fear the back rank.** `traps`: some checking reply leaves this side's
   king no move and no capture of the checker, only blocks. That is the
   classic back-rank mate one move later, beyond the one-reply horizon. It
   costs three pawns.
7. **Within that, trust the model.** The model's score adds up to 1.5
   pawns, so the model picks among moves that are materially equal. This
   is where the Maia models keep their human sense of position.
8. **Then the plan, as a tiebreak.** Castle and develop early, keep king
   cover, attack the squares round their king, push passed pawns late, keep
   your king off the edge and close in on theirs when ahead, trade when
   ahead, and don't leave pieces loose or on the rim. With a model this
   only breaks exact ties. With no model (`none`) it is the whole positional
   game.

## What the games kept and dropped

Score = points share against the Stockfish level, 40 games unless noted.

| version | idea | result | verdict |
|---|---|---|---|
| seed | the starter chain (mating, not hanging more than it captures, highest score) | maia1100 @1320: 0.700 | baseline |
| v0 | seed with SEE `hangs` (the whole exchange, not one capture) | maia1100 @1500: 0.575 | kept |
| v1 | mating, not exposing, then `highest material + score*1.5/1000`, material = `outcome` (after best reply) | @1500: 0.575, but 3 stalemates and 4 repetitions thrown away | kept, fixed next |
| v2 | v1 + draw tiers + a full-weight positional plan | @1500: 0.438, override rate 0.31 | dropped: the plan overrode Maia too often |
| v2a | v1 + never stalemate, repeat only when behind | @1500: 0.613 | kept |
| v3a | v2a, with quiet checks counted as delaying (`outlook`) | @1500: 0.625 | kept |
| v4 | v3a + back-rank `traps` costs 3; the "seek a draw when 3 down" tier dropped (it stalemated a won attack) | @1500: 0.719 (80 games), 0.613 (another 40); @1700: 0.263 | kept |
| v4 s3 / s0.8 | the model's weight 3.0 or 0.8 pawns instead of 1.5 | @1500: 0.650 / 0.600 | no clear difference: kept 1.5 |
| v5 | v4 + the plan at 1/4 weight inside `value` | @1500: 0.475 and 0.537 | dropped: even a quarter-weight plan costs Maia |
| n2 | `none`: material + a positional plan | none @1320: 0.400 (v3a material alone: 0.062) | the plan is everything without a model |
| n3 | n2 + traps, king peril, rim, trades when ahead, no draw seeking | none @1320: 0.613; on maia1100 @1500: 0.487 | good for `none`, bad for Maia |
| v6 | v4 + n3's plan as a *tiebreak after* `value` | see the Elo table | round 1 final, superseded by v8 |

Ideas tried and not kept:
- Seeking a draw by repetition or stalemate when behind by 3: it fired in
  positions that were lost on material but won on attack.
- Fork and threat bonuses in the plan: `gains` already counts forks after
  the reply, so the bonus double-counts them and overrides the model.
- A penalty for every check reply that leaves at most one answer (`nets`):
  too broad, since many such checks are harmless. The narrower `traps`
  (only blocks are left) is what paid.
- A heavier model weight (3 pawns), which let Maia hang pieces again in
  sharp positions, and a lighter one (0.8): neither beat 1.5.

What the losses at 1700 show the rules can't reach without search: mates in
two or more, quiet attacking build-ups against our king, and lost pawn
endgames that need counting.

## Elo

Round 1 final strategy (v6), fitted with `chess-ladder elo` over every file below,
Stockfish levels as anchors. CI is the fit's 95% interval. Each arm has 40
games against each of two levels (Stockfish 100 ms per move, `UCI_Elo`).

| model | alone (naked) | with Suture | gain | games per arm, by level |
|---|---|---|---|---|
| maia1100 | 1360 ± 79 | **1568 ± 80** | +208 | 1320 and 1500 alone; 1500 and 1700 with Suture |
| maia1500 | 1516 ± 81 | **1718 ± 83** | +202 | 1500 and 1700, both arms |
| none (rules alone) | 557 ± 491 (0/40) | **1313 ± 81** | +756 | 1320 alone; 1320 and 1500 with Suture |
| nnue | 1039 ± 120 | **1702 ± 92** | +663 | 1320 and 1500 alone; 1320 and 1700 with Suture |

Points scored per arm:

| arm | @1320 | @1500 | @1700 |
|---|---|---|---|
| maia1100 alone | 0.575 | 0.287 | |
| maia1100 + Suture | | 0.550 | 0.362 |
| maia1500 alone | | 0.438 | 0.338 |
| maia1500 + Suture | | 0.700 | 0.613 |
| none alone | 0.000 | | |
| none + Suture | 0.487 | 0.250 | |
| nnue alone | 0.212 | 0.000 | |
| nnue + Suture | 0.875 | | 0.537 |

Notes:
- `none` alone is a random mover and lost all 40 games at 1320, so it was
  not played at 1500: its rating is only an upper bound.
- The rule layer helps every model, and helps the weakest the most. It
  overrides `nnue` (a static evaluator that hangs material one move later)
  on 55% of moves, Maia on 15–23%.
- Over 160 games at 1500, v4 (no plan tiebreak) scored about 0.62 with
  maia1100. In an earlier fit, now lost to a machine restart, it rated
  1615 ± 65 against 1351 ± 80 alone. v6's maia1100 games at 1500 (0.550)
  are within that noise, and v6 does better at 1700 (0.362 against v4's
  0.263). The tiebreak changes little for Maia and is what makes `none`
  playable.
- The scale is steep: in these runs Stockfish 1320 at 100 ms is already
  stronger than its label for human-like players.

## Language gaps (round 1)

What I wanted to write, and the workaround in `bot.st`. Ordered by how much
each one cost the strategy.

1. **Tolerance tiers.** *"then highest material, but within half a pawn of
   the best, the model's favourite."* A lexicographic chain can't express
   "near-best". Workaround: one linear `value` with the model worth up to
   1.5 pawns, which is never quite the rule I mean.
2. **A Bool or a phase inside a formula.** *"plan: … + castles * 0.5 if it is
   the opening"*, or `those that are castling` as a weight. Workaround: the
   harness enters every Bool a second time as a 0/1 Count (`castles`,
   `early`, `late`), and phases multiply terms by `early` and `(1 - late)`.
3. **Signed numbers.** *"material: Number = wins - loses"*. Count can't go
   below 0. Workaround: Money as a signed score (`gains * 1 EUR - drops * 1
   EUR`), with every signed fact split into two Counts (`gains`/`drops`,
   `up`/`down`). Related: a Money literal has at most 2 decimals, so it reads
   `score * 1.50 EUR / 1000`, and `suture explain` prints a Money between -1
   and 0 without its minus sign (-0.575 shows as "0.58 EUR").
4. **Pool aggregates and context in prefer.** *"those whose material is the
   best among the Turn's Moves"*, *"those that are not the last move's piece
   moving back"*, *"the Turn's Moves' most material"*. A Move can't see its
   pool, its Turn or the game's history. Workaround: the harness computes
   `repeating`, `up`/`down` and `early`/`late` per move.
5. **A define in a prefer.** *"define safe: a Move that is not exposing and
   whose drops is 0"* then *"those that are safe"*. Rejected ("`it` is a shared
   Move"). Workaround: inline the condition, or make it a formula attribute.
6. **`or` after `those whose`.** *"those whose down is at least 1 or that are
   not repeating"*. Workaround: `those that are not repeating or its down is
   at least 1`. (`and`/`or` do work after `those that`, contrary to the
   stated limit.)
7. **max/min in formulas.** *"material: … max(threat, fork)"*. Workaround: the
   adapter computes it.
8. **A kind on one line.** The Move kind is one 1,400-character line with
   33 attributes and 3 formulas. There is no way to break it or comment an
   attribute. Workaround: a comment block above it.
9. **The 4096-byte unit.** With all ~60 facts a Move is refused at runtime.
   Workaround: the harness enters only the attributes the kind declares.
10. **Named weights.** *"a pawn is 1 EUR; the model is worth 1.5 pawns"*. No
    constants, so the weights are bare numbers in formulas.

## Round 2: the strategy in slice 63's forms (v8, the current `bot.st`)

### What the new forms let me say

- **The tolerance tier**, round 1's gap #1, is now the heart of the
  strategy: `highest material within 1.00 EUR, then those that are not
  cornered, then highest score within 100, then highest material, then
  highest score, then highest plan`. In words: never give up more than a
  pawn by our count. Among the moves that keep it, stay out of checks that
  leave the king one answer. Then take the model's near-favourites (within
  a tenth of its scale), and among those the most material. Then the
  model's own favourite. A model without scores falls through to the plan.
  Round 1 had to blend material and the model into one linear `value`,
  which is never quite this rule.
- **Defines in prefer:** `define: a Move is sound when it is not exposing
  and it is not stalemating`, `define: a Move is wasteful when it is
  repeating and its down is 0`, then `those that are sound, then those that
  are not wasteful`.
- **A Bool in arithmetic:** `material: Money = gains * 1 EUR - drops * 1 EUR
  - trapped * 3 EUR`, and the plan reads `castling * 0.50 EUR`,
  `opening * (…)`, `(1 - endgame) * (…)`. The 0/1 Count duplicates are gone
  from the kind. The harness still offers them, for old strategies.
- **A kind over many lines** with a `--` comment on each group of
  attributes. The Move kind is now readable.

### How v8 was tuned

Games against Stockfish are noisy: 40 games give about ±0.15 in score.
Before playing, I scored candidate chains offline:
- `chess-ladder dump <scorer> <games> <out>` writes every position of a set
  of games, with each legal move's attributes and the model's scores.
- Stockfish (depth 9, every move) then gives each move's true value.
- A Python copy of the chain (the margin included) picks a move in each
  position.

The measure is the average centipawn loss of the pick:

| chain | maia1900, 1715 positions | maia1100, 1627 positions |
|---|---|---|
| model alone | 62.1 | 86.4 |
| v6: linear `material + 1.5 * score/1000`, then plan | 57.0 | – |
| v7: `highest material within 0.50 EUR, then highest score` | 56.5 | – |
| `highest material within 1.00 EUR, then highest score` | 55.9 | 65.0 |
| … then `those that are not cornered` | 54.3 | 63.3 |
| **v8**: … then `highest score within 100, then highest material` | **53.3** | **60.2** |
| … and the model's margin at 200 instead of 100 | 55.2 | 61.5 |
| plan within the model's margin (instead of material) | 58.5 | – |

The one-pawn margin is right because one-reply material is wrong by a pawn
often enough: the model knows a pawn sacrifice or a pawn's real price
better. The 1-pawn margin agrees with maia1900 on 93% of moves, against 82%
for the 0.5 margin, and plays no worse.

### Elo, round 2

**Superseded by `../RESULTS.md`.** Some of the games below started before
the final `bot.st` was saved. Every arm was replayed on the final file: maia1900
with Suture is 1794 ± 84 there, not 1887 ± 73. The table is kept as it was
measured.

The final strategy (v8). Naked arms as in round 1, plus maia1900. 40 games
per arm per level; maia1500 with Suture has 80 games at 1700. Fitted
together with `chess-ladder elo`, 95% CI.

| model | alone | with Suture (v8) | gain | round 1 (v6) with Suture | levels: alone / with Suture |
|---|---|---|---|---|---|
| maia1900 | 1663 ± 90 | **1887 ± 73** | +224 | – (v7: 1742 ± 85) | 1700, 2000 / 1700, 2000, 2200 |
| maia1500 | 1516 ± 81 | **1656 ± 64** | +140 | 1718 ± 83 | 1500, 1700 / 1500, 1700 |
| maia1100 | 1360 ± 79 | **1595 ± 79** | +235 | 1568 ± 80 | 1320, 1500 / 1500, 1700 |
| nnue | 1039 ± 120 | **1659 ± 90** | +620 | 1702 ± 92 | 1320, 1500 / 1320, 1700 |
| none | 557 ± 491 | **1293 ± 82** | +736 | 1313 ± 81 | 1320 / 1320, 1500 |

| arm (v8) | @1320 | @1500 | @1700 | @2000 | @2200 |
|---|---|---|---|---|---|
| maia1900 alone | | | 0.425 | 0.138 | |
| maia1900 + Suture | | | 0.650 | 0.400 | 0.175 |
| maia1500 + Suture | | 0.637 | 0.463, 0.487 | | |
| maia1100 + Suture | | 0.613 | 0.375 | | |
| nnue + Suture | 0.900 | | 0.425 | | |
| none + Suture | 0.512 | 0.175 | | | |

Reading it:
- **maia1900 with Suture is about 1890, not yet 2000.** It scores 0.40
  against Stockfish 2000, against the model's 0.14 alone. The model's own
  play is the ceiling here. The rules remove its outright blunders, and its
  remaining losses are mates in two or more and quiet positional losses
  (the dump finds 30 quiet, 17 missed mates while winning, and 18 walks
  into a mate beyond one reply, out of ~100 blunders by v8-like chains).
- **The change helps one model and may hurt another.** v8 beats v6 for
  maia1900 (v7, close to v6, has 1742) and slightly for maia1100. For
  maia1500 it is 60 points below v6, inside both CIs but over 120 games, so
  it may be real. nnue and none are flat. I kept v8 because the brief's
  target is the strongest model, and because the offline measure prefers
  it for both Mais it was run on.
- The override rate fell from 15–23% to 11–16% on Maia. The margin trusts
  the model on one-pawn calls.

### Gaps that remain, ranked

1. **Signed numbers.** *"material: Number = gains - drops - trapped * 3"*.
   Still Money as a stand-in (1 EUR = a pawn), and every signed fact is
   split in two Counts (`gains`/`drops`, `up`/`down`). It is the most
   visible oddity in `bot.st`.
2. **Conditional criteria.** *"then, when it is far ahead, lowest boxed"*
   (squeeze their king when winning). Offline, a 40-point-per-move box
   penalty at 5 pawns up was among the best tweaks for maia1900. It can't
   be one criterion: it needs a signed formula `score - (up >= 5) * boxed *
   40`, and there is no comparison inside arithmetic and no signed Count.
   Workaround: left out.
3. **Pool aggregates.** *"those whose material is at least the model's
   favourite's material minus 1"*: compare with a particular other
   candidate (the model's first choice), not just with the best. Also
   *"the Turn's Moves' most material"*, for a rule like "if every move
   loses material, take the one the model likes". Workaround: margins
   relative to the tier's best.
4. **A margin as a share of the best.** *"highest score within a tenth of
   the best"*. A model whose top move has 900 and one whose top has 120
   deserve different absolute margins. Workaround: a fixed 100 on the
   0–1000 scale.
5. **max/min in formulas.** *"material: … max(threat, fork)"*, or clamp a
   loss at 3 pawns. Workaround: the adapter computes it.
6. **Named weights.** *"a pawn is 1 EUR; the trap is worth 3 pawns"*. The
   weights are bare literals in two formulas.
7. **The 4096-byte unit.** v8's 34 attributes and 2 formulas fit, but a strategy can't
   read all ~60 facts at once. Workaround: the harness enters only the
   attributes the kind declares.
8. **Mixing `and` and `or` in one clause.** Not needed in v8: defines
   carried it.

The rules boundary matters more than the language now. The remaining
blunders need two replies of sight (mate in two), which the brief rules
out of the facts. The next gain for a 1900 model is likely there, or in a
model that rates moves better.
