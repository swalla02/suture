#!/bin/bash
# run.sh GAMES PLAYER... : each player against each Stockfish level in $LEVELS,
# GAMES games per level. Results go to $OUT/<strategy>/ (OUT defaults to
# ~/chess-runs), where <strategy> is the short hash of suture/bot.st, so each
# strategy version keeps its own results. A rerun appends to the same files.
#
# Build first: cargo build -p chess-ladder. The engines and nets are found as
# chess-ladder says (CHESS_TOOLS, else ~/chess-tools). An API player reads its
# key from OPENROUTER_API_KEY, else ~/.config/chess-ladder/openrouter.key.
repo=${REPO:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}
ver=$(git -C "$repo" hash-object eval/chess/suture/bot.st | cut -c1-8)
out=${OUT:-~/chess-runs}/$ver; mkdir -p "$out/scratch"
cp "$repo/target/debug/chess-ladder" "$out/ladder-bin"
mkdir -p "$out/strategy" && cp "$repo"/eval/chess/suture/*.st "$out/strategy/"
key=${OPENROUTER_API_KEY:-$(cat ~/.config/chess-ladder/openrouter.key 2>/dev/null)}
n=$1; shift
for p in "$@"; do for sf in ${LEVELS:-1320 1700}; do
  OPENROUTER_API_KEY=$key CHESS_SCRATCH="$out/scratch" CHESS_STRATEGY="$out/strategy" \
    "$out/ladder-bin" match "$p" stockfish:$sf "$n" "$out/$(echo "$p" | tr ':/' '__').jsonl" "${THREADS:-3}" 2>>"$out/log.txt"
done; done
echo "$ver done"
