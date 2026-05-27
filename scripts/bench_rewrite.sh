#!/usr/bin/env bash
# Benchmark 2d diagram rewriting (lambda-sigma calculus normalisation).
#
# Usage:
#   scripts/bench_rewrite.sh [runs] [iters]
#
# Arguments:
#   runs   number of timed runs to report (default: 5)
#   iters  --bench iteration count per run  (default: 10)

set -euo pipefail

RUNS="${1:-5}"
ITERS="${2:-10}"

REPO="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURE="$REPO/tests/fixtures/bench_rewrite.ali"
ALIFIB="$REPO/target/release/alifib"
export ALIFIB_PATH="$REPO/examples"

if [[ ! -f "$FIXTURE" ]]; then
  echo "error: $FIXTURE not found" >&2
  exit 1
fi

if [[ ! -x "$ALIFIB" ]]; then
  echo "Building release binary..."
  cargo build --release --manifest-path "$REPO/Cargo.toml" 2>&1 | tail -1
fi

echo "fixture: tests/fixtures/bench_rewrite.ali"
echo "runs:    $RUNS × $ITERS iterations"
echo

times=()
for (( i = 0; i < RUNS; i++ )); do
  ms=$("$ALIFIB" "$FIXTURE" --bench "$ITERS")
  times+=("$ms")
  printf "  run %d:  %s ms\n" $((i + 1)) "$ms"
done

IFS=$'\n' sorted=($(printf '%s\n' "${times[@]}" | sort -n)); unset IFS
echo
echo "  median: ${sorted[$(( RUNS / 2 ))]} ms"
