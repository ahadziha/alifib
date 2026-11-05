#!/usr/bin/env bash
# Benchmark `auto N` with parallel mode OFF vs ON.
# Times only `alifib do 'auto N'`: one process spawn and one HTTP round trip.
#
# Usage:
#   scripts/bench_auto.sh <file.ali> <type> <source> <N> [runs]
#
# Example:
#   scripts/bench_auto.sh examples/BubbleSort.ali Sort 'blue white red' 50

set -euo pipefail

FILE="${1:?Usage: bench_auto.sh <file> <type> <source> <N> [runs]}"
TYPE="${2:?}"
SOURCE="${3:?}"
N="${4:?}"
RUNS="${5:-10}"

REPO="$(cd "$(dirname "$0")/.." && pwd)"
ALIFIB="$REPO/target/release/alifib"

if [[ ! -x "$ALIFIB" ]]; then
  echo "error: $ALIFIB not found; run cargo build --release -p alifib-cli" >&2
  exit 1
fi
if [[ ! "$N" =~ ^[0-9]+$ || ! "$RUNS" =~ ^[1-9][0-9]*$ ]]; then
  echo "error: N must be nonnegative and runs must be positive integers" >&2
  exit 1
fi

FILE_DIR="$(cd -- "$(dirname -- "$FILE")" && pwd)"
BASENAME="$(basename -- "$FILE")"
# Encode an alifib path literal, not shell syntax. Both escapes are significant
# to the command parser even though the entire line is one shell argument.
LOAD_PATH="${BASENAME//\\/\\\\}"
LOAD_PATH="${LOAD_PATH//\"/\\\"}"

echo "file:   $FILE"
echo "type:   $TYPE"
echo "source: $SOURCE"
echo "auto:   $N"
echo "runs:   $RUNS"
echo

TMPDIR_BENCH="$(mktemp -d)"
SERVER_PID=""
stop_server() {
  local status
  if [[ -n "$SERVER_PID" ]]; then
    if kill -0 "$SERVER_PID" 2>/dev/null; then
      kill "$SERVER_PID"
    fi
    if wait "$SERVER_PID"; then
      status=0
    else
      status=$?
    fi
    SERVER_PID=""
    # SIGTERM is the expected exit after our kill; any other failure matters.
    if (( status != 0 && status != 143 )); then
      echo "error: server exited with status $status" >&2
      return "$status"
    fi
  fi
}
cleanup() {
  local status=$?
  if stop_server; then :; else
    if (( status == 0 )); then status=1; fi
  fi
  rm -rf "$TMPDIR_BENCH"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

run_once() {
  local parallel="$1" stderr="$TMPDIR_BENCH/server.err"
  local address="" attempt first_line t0 t1
  "$ALIFIB" serve "$FILE_DIR" --bind 127.0.0.1:0 >"$TMPDIR_BENCH/server.out" 2>"$stderr" &
  SERVER_PID=$!

  # Wait for the actual bound-address announcement, not a guessed port.
  for (( attempt = 0; attempt < 1000; attempt++ )); do
    address=$(sed -n 's/^alifib serve listening on http:\/\/\(127\.0\.0\.1:[0-9][0-9]*\)$/\1/p' "$stderr")
    if [[ -n "$address" ]]; then break; fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
      cat "$stderr" >&2
      echo "error: server exited before announcing its address" >&2
      return 1
    fi
    sleep 0.01
  done
  if [[ -z "$address" ]]; then
    cat "$stderr" >&2
    echo "error: timed out waiting for server address" >&2
    return 1
  fi

  "$ALIFIB" do --at "$address" "load \"$LOAD_PATH\"" >/dev/null
  "$ALIFIB" do --at "$address" "start $TYPE ($SOURCE)" >/dev/null
  "$ALIFIB" do --at "$address" "parallel $parallel" >/dev/null

  t0=$(date +%s%N)
  AUTO_REPLY=$("$ALIFIB" do --at "$address" "auto $N")
  t1=$(date +%s%N)
  ELAPSED_NS=$(( t1 - t0 ))

  first_line="${AUTO_REPLY%%$'\n'*}"
  if [[ "$first_line" =~ ^applied\ ([0-9]+)\ steps?(\ \((.*)\))?$ ]]; then
    APPLIED="${BASH_REMATCH[1]}"
    STOP="${BASH_REMATCH[3]}"
  else
    echo "error: unexpected auto reply: $AUTO_REPLY" >&2
    return 1
  fi
  stop_server
}

for parallel in off on; do
  if [[ "$parallel" == "off" ]]; then
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

  mapfile -t sorted < <(printf '%s\n' "${times[@]}" | sort -n)
  median="${sorted[$(( RUNS / 2 ))]}"

  best_ms=$(awk "BEGIN { printf \"%.2f\", $best / 1000000 }")
  median_ms=$(awk "BEGIN { printf \"%.2f\", $median / 1000000 }")
  mean_ms=$(awk "BEGIN { printf \"%.2f\", $total / $RUNS / 1000000 }")

  echo "  $label  applied $APPLIED steps${STOP:+ ($STOP)}"
  echo "    best ${best_ms} ms    median ${median_ms} ms    mean ${mean_ms} ms"
  echo
done
