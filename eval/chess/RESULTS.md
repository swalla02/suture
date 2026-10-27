# Chess: a move scorer, alone and with Suture

Frozen on 2026-10-26 at strategy `269e2871` (the first 8 characters of `git hash-object suture/bot.st`; `suture/STRATEGY.md` explains it). Chess is now a demo. The main test of Suture is the rules experiment in `docs/superpowers/specs/2026-10-26-rule-decisions-experiment.md`.

## How it is measured

- **Players.** On each turn the ladder makes one `Move` unit per legal move, with the facts the engine computes (`engine/src/facts.rs`) and the scorer's score for that move.
  - **Alone:** the player plays the scorer's highest-scored move, ties at random.
  - **With Suture:** `bot.st`'s `prefer Move` chain picks the move, reading the same scores.
  - **No model:** every score is equal, so Suture's rules decide alone.
- **Scorers**, by family:
  - **Chess-trained models:**
    - the Maia nets on lc0, one node each;
    - Stockfish's static NNUE evaluation of the position after each move. That evaluation has no value for a position in check, so every check that does not mate scores as level.
  - **General decision models:** OpenRouter's decisions endpoint, asked "which legal move is the strongest" as a choice over the legal moves, each described in words. The score is the model's probability for the move.
  - **LLMs:** OpenRouter chat with reasoning off. The model is asked for its three best moves from the legal list; they score 1000, 600 and 300. A reply that names no legal move scores nothing, so ties are broken at random.
- **Opponent:** Stockfish with `UCI_LimitStrength` at a fixed `UCI_Elo`, 100 ms per move. Each arm plays two of the levels 1320, 1500, 1700 and 2000; the table lists them.
- **Rating:** maximum likelihood with the Stockfish levels as fixed anchors, plus one virtual draw per pairing so a score of 0% still gives a finite number. The 95% interval is 1.96 × 400 / ln 10 / √(information). `chess-ladder elo results/269e2871/*.jsonl` prints every number on this page (`results/elo.json`).
- **Games:** 80 per arm, 40 per level. There are two exceptions:
  - Laguna, on a free tier with a daily cap: 20 games alone, 16 with Suture.
  - "No model, alone": 40 games, all lost. It is left out below.
  - Seven games forfeited when OpenRouter kept answering 502 after every retry; they were replayed and are not in the records.
- **When:** every arm with Suture was played on this strategy. The alone arms do not read `bot.st`:
  - the chess-trained alone arms are earlier games, from the runs of earlier versions;
  - the API alone arms were played in this version's runs.

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
| Clef (Cloudflare) | General decision model | 1 draw in 80 | **1246 ± 98** | – | −109 ± 126 | 80 / 80 | 1320, 1700 / 1320, 1700 |
| GPT-6 Luna Decisions (OpenAI) | General decision model | lost all 80 | **1217 ± 101** | – | −138 ± 128 | 80 / 80 | 1320, 1700 / 1320, 1700 |
| Laguna S 2.1 (Poolside) | LLM | lost all 20 | **1197 ± 215** | – | −158 ± 229 | 20 / 16 | 1320, 1700 / 1320, 1700 |
| Gemma 4 31B (Google) | LLM | lost all 80 | **1144 ± 112** | – | −211 ± 137 | 80 / 80 | 1320, 1700 / 1320, 1700 |
| Nemotron 3 Ultra (NVIDIA) | LLM | lost all 80 | **1020 ± 140** | – | −335 ± 161 | 80 / 80 | 1320, 1700 / 1320, 1700 |

- **Lift** is with Suture minus alone.
- **vs rules alone** is with Suture minus Suture's rules with no model.
- Both come with the 95% interval of the difference.
- **No rating alone:** an arm that scored less than its virtual draws (half a point per level) would be rated mostly by those draws, so it gets none. That is every general decision model and LLM alone. They are below the 1320 level, by an amount these games cannot measure.

![With Suture, compared with Suture's rules and no model](results/beyond.svg)

## What it shows

1. **Suture lifts a chess-trained scorer.** NNUE gains 708 ± 153 and Maia 1100 gains 282 ± 112.
   - The rules mostly remove outright blunders, as far as one reply deep can see them: missing a mate in one, allowing one, and giving up more than a pawn more than another move would.
   - Part of NNUE's gain covers a hole in its scores. Every check short of mate counts as level, so alone, in a worse position, it prefers any check, even one that loses a piece.
2. **The lift shrinks as the scorer gets stronger.**
   - Maia 1900 gains 131 ± 123, barely clear of zero.
   - Maia 1500 gains 79 ± 113, not clear of zero, and with Suture it is not above Maia 1100 with Suture.
   - An earlier version of the strategy rated Maia 1500 with Suture at 1718 ± 83, so the final chain may have cost it.
3. **Suture's rules alone reach 1355,** with no model at all.
4. **General decision models and LLMs add nothing to the rules, and some make them worse.**
   - Jev, Decider and Clef land within noise of the rules alone.
   - Luna, Gemma and Nemotron land below the rules alone, with intervals clear of zero.
   - The likely cause is the order of `bot.st`'s chain. With no model, every score is equal, so material and the plan decide. With a model, among the moves within a pawn of the best material, its near-favourites are kept before exact material and the plan get a say, so a poor score takes their place.
5. **Alone, the general decision models and LLMs lost every game but one,** a draw by Clef in 80. They are below the lowest level on the ladder, so their lift cannot be measured.

## What it does not show

- **The facts carry much of the chess.**
  - The strategy reads 34 facts per move, computed one reply deep by the Rust engine (`engine/src/facts.rs`). They include whether the move mates, allows a mate in one, stalemates or repeats, what it wins or drops, and whether a check in reply could only be blocked.
  - `bot.st` combines them (`sound`, `wasteful`, and the `material` and `plan` formulas) and orders them.
  - Nothing here compares the same strategy written in plain Rust, so this does not show that the language itself adds strength. A skeptic can call Suture the sorting step here.
- **There is no held-out test.** The strategy was tuned against the same Stockfish levels it is rated against.
- **The yardstick is soft.**
  - Stockfish is limited by time on a small, shared machine, so its strength moves with the load.
  - The chess-trained models' alone games are older than their games with Suture, so their lift also carries any change in load between the two. "vs rules alone" compares games from this version's runs.
  - Stockfish's `UCI_Elo` labels are not ratings from other pools. Nor are Maia's names, which come from the Lichess band of the training games.
  - Seeds come from the clock, so a rerun plays different games.
- **The API arms cannot be replayed exactly.**
  - Model answers were not recorded, and a model name on OpenRouter can serve a new version.
  - How often an answer gave no usable move was not counted. Such a turn is played at random, so some of a general model's moves alone may be random.
- **1794 is weak for chess.** A short alpha-beta search plays well past 2000. Reaching 2000 here would say little about Suture.
- **Not run:** Leela, the Claude models, Mammoth (Baseten) and our own move ranker.

## Reproduce

The numbers and charts on this page, from the committed games:

```sh
cargo build -p chess-ladder
target/debug/chess-ladder elo eval/chess/results/269e2871/*.jsonl > eval/chess/results/elo.json
python3 eval/chess/results/figures.py   # redraws climb.svg and beyond.svg, prints the table
```

New games go to `~/chess-runs/<strategy>/`, and a rerun appends to them:

```sh
LEVELS="1700 2000" eval/chess/results/run.sh 40 naked:maia1900 suture:maia1900
LEVELS="1320 1500" eval/chess/results/run.sh 40 suture:none
target/debug/chess-ladder elo ~/chess-runs/269e2871/*.jsonl > ~/chess-runs/elo.json
python3 eval/chess/results/figures.py ~/chess-runs/elo.json ~/chess-runs/269e2871   # over the committed charts
```

- **Engines and nets** come from `CHESS_TOOLS`, else `~/chess-tools`:
  - Stockfish at `stockfish/stockfish/stockfish-ubuntu-x86-64-avx2`;
  - the nets in `nets/` (`maia-1100.pb.gz` and so on);
  - the Windows build `lc0/lc0.exe`, which the ladder runs through WSL (`wslpath -w`).

  The ladder was run on WSL2. On plain Linux the Maia arms fail at start.
- **Player names:** `naked:` or `suture:`, then a scorer:
  - `none`, `nnue` or `maia1100|1500|1900`;
  - `decide:⟨model⟩` for a decision model on OpenRouter;
  - `llm:⟨model⟩` for an LLM on OpenRouter.
- **The API arms** read the key from `OPENROUTER_API_KEY` or `~/.config/chess-ladder/openrouter.key`. All the OpenRouter spending for this ladder came to about $3.80. That covers every version, the replays and the trials.
- **The records** (`results/269e2871/*.jsonl`) hold one game per line:
  - the players and the opening;
  - the moves in UCI;
  - the result and why the game ended;
  - how often Suture overrode the scorer's favourite.
