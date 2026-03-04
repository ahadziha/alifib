#!/usr/bin/env python3
"""Benchmark the OCaml and Rust alifib implementations against each other.

Run from anywhere inside the repo:
    python3 rust/scripts/bench.py

Optional flags:
    -n N      number of runs per batch (default: 30)
    --no-ocaml  skip OCaml (e.g. if not built)
    --no-rust   skip Rust  (e.g. if not built)
"""

import argparse
import os
import subprocess
import sys
import time

RUST_DIR  = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO      = os.path.dirname(RUST_DIR)
OCAML_BIN = os.path.join(REPO, "ocaml", "_build", "default", "src", "main.exe")
RUST_BIN  = os.path.join(RUST_DIR, "target", "release", "alifib")
OCAML_EXAMPLES = os.path.join(REPO, "ocaml", "examples")
RUST_EXAMPLES  = os.path.join(RUST_DIR, "examples")


def bench(cmd, path, n):
    t0 = time.perf_counter()
    for _ in range(n):
        subprocess.run([cmd, path], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return (time.perf_counter() - t0) / n * 1000  # ms per run


def best_of(cmd, path, n, batches=3):
    return min(bench(cmd, path, n) for _ in range(batches))


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("-n", type=int, default=30, metavar="N",
                        help="runs per batch (default: 30)")
    parser.add_argument("--no-ocaml", action="store_true",
                        help="skip OCaml binary")
    parser.add_argument("--no-rust", action="store_true",
                        help="skip Rust binary")
    args = parser.parse_args()

    run_ocaml = not args.no_ocaml
    run_rust  = not args.no_rust

    if run_ocaml and not os.path.isfile(OCAML_BIN):
        print(f"OCaml binary not found: {OCAML_BIN}", file=sys.stderr)
        print("Build with: cd ocaml && dune build", file=sys.stderr)
        run_ocaml = False

    if run_rust and not os.path.isfile(RUST_BIN):
        print(f"Rust binary not found: {RUST_BIN}", file=sys.stderr)
        print("Build with: cd rust && cargo build --release", file=sys.stderr)
        run_rust = False

    if not run_ocaml and not run_rust:
        sys.exit(1)

    # Use Rust examples as the canonical file list; OCaml examples may differ
    files = sorted(f for f in os.listdir(RUST_EXAMPLES) if f.endswith(".ali"))
    if not files:
        print(f"No .ali files found in {RUST_EXAMPLES}", file=sys.stderr)
        sys.exit(1)

    # Header
    if run_ocaml and run_rust:
        print(f"{'File':<22} {'OCaml(ms)':>10} {'Rust(ms)':>10} {'Ratio':>8}")
        print("-" * 58)
    elif run_ocaml:
        print(f"{'File':<22} {'OCaml(ms)':>10}")
        print("-" * 34)
    else:
        print(f"{'File':<22} {'Rust(ms)':>10}")
        print("-" * 34)

    for fname in files:
        name = fname[:-4]
        rust_path  = os.path.join(RUST_EXAMPLES, fname)
        ocaml_path = os.path.join(OCAML_EXAMPLES, fname)

        if run_ocaml and not os.path.isfile(ocaml_path):
            ocaml_ms = None
        else:
            ocaml_ms = best_of(OCAML_BIN, ocaml_path, args.n) if run_ocaml else None

        rust_ms = best_of(RUST_BIN, rust_path, args.n) if run_rust else None

        if run_ocaml and run_rust:
            if ocaml_ms is not None:
                ratio = rust_ms / ocaml_ms if ocaml_ms > 0 else float("nan")
                print(f"{name:<22} {ocaml_ms:>10.1f} {rust_ms:>10.1f} {ratio:>7.1f}x")
            else:
                print(f"{name:<22} {'N/A':>10} {rust_ms:>10.1f} {'N/A':>8}")
        elif run_ocaml:
            if ocaml_ms is not None:
                print(f"{name:<22} {ocaml_ms:>10.1f}")
            else:
                print(f"{name:<22} {'N/A':>10}")
        else:
            print(f"{name:<22} {rust_ms:>10.1f}")


if __name__ == "__main__":
    main()
