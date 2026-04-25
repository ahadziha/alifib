#!/usr/bin/env bash
# Benchmark `auto N` with parallel mode OFF vs ON.
#
# Usage:
#   scripts/bench_auto.sh <file.ali> <type> <source> <N> [runs]
#
# Example:
#   scripts/bench_auto.sh examples/EckmannHilton.ali TwoCells "A.cell B.cell" 50

set -euo pipefail

FILE="${1:?Usage: bench_auto.sh <file> <type> <source> <N> [runs]}"
TYPE="${2:?}"
SOURCE="${3:?}"
N="${4:?}"
RUNS="${5:-10}"

REPO="$(cd "$(dirname "$0")/.." && pwd)"
ALIFIB="$(command -v alifib 2>/dev/null || echo "$REPO/target/release/alifib")"

if [[ ! -x "$ALIFIB" ]]; then
  echo "error: alifib not found" >&2
  exit 1
fi

echo "file:   $FILE"
echo "type:   $TYPE"
echo "source: $SOURCE"
echo "auto:   $N"
echo "runs:   $RUNS"
echo

TMPDIR_BENCH="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_BENCH"' EXIT

run_once() {
  local parallel="$1" out="$TMPDIR_BENCH/out"
  local in_fifo="$TMPDIR_BENCH/in" out_fifo="$TMPDIR_BENCH/out_fifo"
  mkfifo "$in_fifo" "$out_fifo"

  "$ALIFIB" serve < "$in_fifo" > "$out_fifo" 2>/dev/null &
  local pid=$!

  exec 3>"$in_fifo" 4<"$out_fifo"
  rm "$in_fifo" "$out_fifo"

  send() { echo "$1" >&3; read -r REPLY <&4; }

  send "{\"command\":\"init\",\"source_file\":\"$FILE\",\"type_name\":\"$TYPE\",\"source_diagram\":\"$SOURCE\"}"
  send "{\"command\":\"parallel\",\"on\":$parallel}"

  local t0 t1
  t0=$(date +%s%N)
  send "{\"command\":\"auto\",\"max_steps\":$N}"
  t1=$(date +%s%N)

  AUTO_REPLY="$REPLY"

  echo '{"command":"shutdown"}' >&3
  exec 3>&- 4<&-
  wait "$pid" 2>/dev/null || true

  ELAPSED_NS=$(( t1 - t0 ))
}

for parallel in false true; do
  if [[ "$parallel" == "false" ]]; then
    label="parallel OFF"
  else
    label="parallel ON "
  fi

  best=""
  total=0
  times=()

  for (( i = 0; i < RUNS; i++ )); do
    run_once "$parallel"
    times+=("$ELAPSED_NS")
    total=$(( total + ELAPSED_NS ))
    if [[ -z "$best" ]] || (( ELAPSED_NS < best )); then
      best="$ELAPSED_NS"
    fi
  done

  applied=$(echo "$AUTO_REPLY" | grep -o '"applied":[0-9]*' | cut -d: -f2)
  stop=$(echo "$AUTO_REPLY" | grep -o '"stop_reason":"[^"]*"' | cut -d'"' -f4)

  IFS=$'\n' sorted=($(printf '%s\n' "${times[@]}" | sort -n)); unset IFS
  median="${sorted[$(( RUNS / 2 ))]}"

  best_ms=$(awk "BEGIN { printf \"%.2f\", $best / 1000000 }")
  median_ms=$(awk "BEGIN { printf \"%.2f\", $median / 1000000 }")
  mean_ms=$(awk "BEGIN { printf \"%.2f\", $total / $RUNS / 1000000 }")

  echo "  $label  applied $applied steps ($stop)"
  echo "    best ${best_ms} ms    median ${median_ms} ms    mean ${mean_ms} ms"
  echo
done
