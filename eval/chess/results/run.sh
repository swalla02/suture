#!/bin/bash
# run.sh GAMES PLAYER... : each player against each Stockfish level in $LEVELS,
# GAMES games per level. Results go to $OUT/<strategy>/ (OUT defaults to
# ~/chess-runs), where <strategy> is the first 8 characters of
# `git hash-object eval/chess/suture/bot.st`, so each strategy version keeps
# its own results. A rerun appends to the same files.
#
# Build first: cargo build -p chess-ladder. The engines and nets are found as
# chess-ladder says (CHESS_TOOLS, else ~/chess-tools). An API player reads its
# key from OPENROUTER_API_KEY, else ~/.config/chess-ladder/openrouter.key.
set -eu
[ $# -ge 2 ] || { echo "usage: run.sh GAMES PLAYER..." >&2; exit 2; }
repo=${REPO:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}
# This run's own ladder and strategy, so a rebuild, an edit or another run
# started meanwhile cannot change the games under it.
run=$(mktemp -d); trap 'rm -rf "$run"' EXIT
cp "$repo/target/debug/chess-ladder" "$run/ladder"
mkdir "$run/strategy" && cp "$repo"/eval/chess/suture/*.st "$run/strategy/"
ver=$(git hash-object "$run/strategy/bot.st" | cut -c1-8)
out=${OUT:-~/chess-runs}/$ver; mkdir -p "$out/scratch" "$out/strategy"
cp "$run"/strategy/*.st "$out/strategy/"
key=${OPENROUTER_API_KEY:-$(cat ~/.config/chess-ladder/openrouter.key 2>/dev/null || true)}
n=$1; shift
for p in "$@"; do for sf in ${LEVELS:-1320 1700}; do
  if ! OPENROUTER_API_KEY=$key CHESS_SCRATCH="$out/scratch" CHESS_STRATEGY="$run/strategy" \
      "$run/ladder" match "$p" stockfish:$sf "$n" "$out/$(echo "$p" | tr ':/' '__').jsonl" "${THREADS:-3}" 2>>"$out/log.txt"; then
    echo "run.sh: $p against stockfish:$sf failed; the end of $out/log.txt:" >&2
    tail -n 5 "$out/log.txt" >&2
    exit 1
  fi
done; done
echo "$ver done"
