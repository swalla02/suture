# Chess: a move scorer, alone and with Suture

Frozen on 2026-10-26 at strategy `269e2871` (the hash of `suture/bot.st`, v8 from round 2 of `suture/STRATEGY.md`). Chess is now a demo. The main test of Suture is the rules experiment in `docs/superpowers/specs/2026-10-26-rule-decisions-experiment.md`.

## How it is measured

- **Players.** On each turn the ladder makes one `Move` unit per legal move, with the facts the engine computes (`engine/src/facts.rs`) and the scorer's score for that move.
  - **Alone:** the player plays the scorer's highest-scored move, ties at random.
  - **With Suture:** `bot.st`'s `prefer Move` chain picks the move, reading the same scores.
  - **No model:** every score is equal, so Suture's rules decide alone.
- **Scorers**, by family:
  - **Chess-trained models:** the Maia nets on lc0, one node each; Stockfish's static NNUE evaluation of the position after each move.
  - **General decision models:** OpenRouter's decisions endpoint, asked "which legal move is the strongest" as a choice over the legal moves, each described in words. The score is the model's probability for the move.
  - **LLMs:** OpenRouter chat with reasoning off. The model is asked for its three best moves from the legal list; they score 1000, 600 and 300. A reply that names no legal move scores nothing, so ties are broken at random.
- **Opponent:** Stockfish with `UCI_LimitStrength` at a fixed `UCI_Elo`, 100 ms per move. Each arm plays two levels near its strength.
- **Rating:** maximum likelihood with the Stockfish levels as fixed anchors, plus one virtual draw per pairing so a score of 0% still gives a finite number. The 95% interval is 1.96 × 400 / ln 10 / √(information). `chess-ladder elo results/269e2871/*.jsonl` prints every number on this page (`results/elo.json`).
- **Games:** 80 per arm, 40 per level. The exceptions are Laguna, on a free tier with a daily cap (20 alone, 16 with Suture), and "no model, alone" (40 games, all lost, left out below).

## Results

![Each model alone, then with Suture](results/climb.svg)

| Model | Family | Alone | With Suture | Lift | vs rules alone | Games, alone / with | Stockfish levels, alone / with |
|---|---|---|---|---|---|---|---|
| Suture's rules, no model | – | – | **1355 ± 79** | – | – | – / 80 | – / 1320, 1500 |
| Maia 1900 | Chess-trained model | 1663 ± 90 | **1794 ± 84** | +131 ± 123 | +439 ± 115 | 80 / 80 | 1700, 2000 / 1700, 2000 |
| NNUE, 1 ply | Chess-trained model | 1039 ± 120 | **1747 ± 95** | +708 ± 153 | +392 ± 124 | 80 / 80 | 1320, 1500 / 1320, 1700 |
| Maia 1100 | Chess-trained model | 1360 ± 79 | **1642 ± 80** | +282 ± 112 | +287 ± 112 | 80 / 80 | 1320, 1500 / 1500, 1700 |
| Maia 1500 | Chess-trained model | 1516 ± 81 | **1595 ± 79** | +79 ± 113 | +240 ± 112 | 80 / 80 | 1500, 1700 / 1500, 1700 |
| Jev 1.14 (TypeSafe) | General decision model | lost all 80 | **1367 ± 90** | – | +12 ± 120 | 80 / 80 | 1320, 1700 / 1320, 1700 |
| Decider V1.2 (Perplexity) | General decision model | lost all 80 | **1306 ± 93** | – | −49 ± 122 | 80 / 80 | 1320, 1700 / 1320, 1700 |
| Clef (Cloudflare) | General decision model | 732 ± 286 | **1246 ± 98** | +514 ± 302 | −109 ± 126 | 80 / 80 | 1320, 1700 / 1320, 1700 |
| GPT-6 Luna Decisions (OpenAI) | General decision model | lost all 80 | **1217 ± 101** | – | −138 ± 128 | 80 / 80 | 1320, 1700 / 1320, 1700 |
| Laguna S 2.1 (Poolside) | LLM | lost all 20 | **1197 ± 215** | – | −158 ± 229 | 20 / 16 | 1320, 1700 / 1320, 1700 |
| Gemma 4 31B (Google) | LLM | lost all 80 | **1144 ± 112** | – | −211 ± 137 | 80 / 80 | 1320, 1700 / 1320, 1700 |
| Nemotron 3 Ultra (NVIDIA) | LLM | lost all 80 | **1020 ± 140** | – | −335 ± 161 | 80 / 80 | 1320, 1700 / 1320, 1700 |

"Lift" is with Suture minus alone. "vs rules alone" is with Suture minus Suture's rules with no model. Each has the 95% interval of the difference. "Lost all" has no rating, only "below the 1320 level": a fit of zero points is the virtual draw talking.

![With Suture, compared with Suture's rules and no model](results/beyond.svg)

An interactive version of these charts is the [Suture chess ladder dashboard](https://claude.ai/artifact/9KU5MwqcvEwG5YRBHVVfLz). It is private to its owner unless shared.

## What it shows

1. **Suture lifts a chess-trained scorer.** NNUE gains 708 ± 153 and Maia 1100 gains 282 ± 112. The rules mostly remove outright blunders, as one reply deep can see them: missing a mate in one, allowing one, and giving up more than a pawn compared with the best move.
2. **The lift shrinks as the scorer gets stronger.** Maia 1900 gains 131 ± 123, barely clear of zero. Maia 1500 gains 79 ± 113, not clear of zero, and with Suture it is not above Maia 1100 with Suture. v6 had Maia 1500 with Suture at 1718 ± 83, so v8's changes likely cost it (`STRATEGY.md`, round 2).
3. **Suture's rules alone reach 1355,** with no model at all.
4. **General decision models and LLMs add nothing to the rules, and some make them worse.**
   - Jev, Decider and Clef land within noise of the rules alone.
   - Luna, Gemma and Nemotron land below the rules alone, with intervals clear of zero.
   - The likely cause is the order of `bot.st`'s chain. Among the moves within a pawn of the best material, the model's near-favourites are kept before exact material and the plan get a say. With no model every score is equal, so material and the plan decide. A poor score takes their place.
5. **Alone, every general model loses every game but one.** Clef drew once in 80. They are below the lowest level on the ladder, so their "lift" cannot be measured.

## What it does not show

- **Most of the strength is in the facts, not the language.** "Mating", "sound", "wasteful" and "trapped" are computed by the Rust engine. `bot.st` orders them. A skeptic can call Suture the sorting step here.
- **There is no held-out test.** The strategy was tuned against the same Stockfish levels it is rated against.
- **The yardstick is soft.**
  - Stockfish is limited by time on a small, shared machine, so its strength moves with the load.
  - Its `UCI_Elo` labels are not ratings from other pools. Nor are Maia's names, which come from the Lichess band of the training games.
  - Seeds come from the clock, so a rerun plays different games.
- **The API arms cannot be replayed exactly.** Model answers were not recorded, and a model name on OpenRouter can serve a new version.
- **1794 is weak for chess.** A short alpha-beta search plays well past 2000. Reaching 2000 here would say little about Suture.
- **Not run on this strategy:** Leela, the Claude models, Mammoth (Baseten) and our own move ranker.

## Corrections

- Round 2 of `STRATEGY.md` reported Maia 1900 with Suture at 1887 ± 73. Some of those games started before the final `bot.st` was saved. Every arm was replayed on the final file, and the replay gives 1794 ± 84. `STRATEGY.md` keeps the old table, marked as superseded.
- Seven games against OpenRouter forfeited when the API kept answering 502 after every retry. They were set aside and replayed, and are not in the records.

## Reproduce

```sh
cargo build -p chess-ladder
# engines and nets: CHESS_TOOLS, else ~/chess-tools (stockfish/, lc0/, nets/)
LEVELS="1700 2000" eval/chess/results/run.sh 40 naked:maia1900 suture:maia1900   # appends to ~/chess-runs/<strategy>/
LEVELS="1320 1500" eval/chess/results/run.sh 40 suture:none
target/debug/chess-ladder elo eval/chess/results/269e2871/*.jsonl > eval/chess/results/elo.json
python3 eval/chess/results/figures.py                               # redraws the charts, prints the table
```

- **Player names:** `naked:` or `suture:`, then a scorer:
  - `none`, `nnue` or `maia1100|1500|1900`;
  - `decide:⟨model⟩` for a decision model on OpenRouter;
  - `llm:openrouter:⟨model⟩` for an LLM.
- **The API arms** read the key from `OPENROUTER_API_KEY` or `~/.config/chess-ladder/openrouter.key`. Everything this ladder spent on OpenRouter, across every version, replays and trials included, came to about $3.80.
- **The records** (`results/269e2871/*.jsonl`) hold one game per line: the players, the opening, the moves in UCI, the result, why it ended, and how often Suture overrode the scorer's favourite.
