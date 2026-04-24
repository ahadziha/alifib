#!/usr/bin/env bash
# ======================================================================
#   The Shape of Concurrency — alifib demo
# ----------------------------------------------------------------------
#   Computes the cellular homology of four polygraphic rewriting systems
#   and checks that each one matches the integer homology of a named
#   topological space.
# ======================================================================

set -euo pipefail

# Colours — degrade gracefully if the terminal can't handle them.
if [[ -t 1 ]] && command -v tput >/dev/null 2>&1 && [[ "$(tput colors 2>/dev/null || echo 0)" -ge 8 ]]; then
    BOLD="$(tput bold)"; DIM="$(tput dim)"; RST="$(tput sgr0)"
    RED="$(tput setaf 1)"; GRN="$(tput setaf 2)"; YLW="$(tput setaf 3)"
    BLU="$(tput setaf 4)"; MAG="$(tput setaf 5)"; CYN="$(tput setaf 6)"
else
    BOLD=""; DIM=""; RST=""; RED=""; GRN=""; YLW=""; BLU=""; MAG=""; CYN=""
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"
ALIFIB="$REPO/target/release/alifib"
EXAMPLE="$REPO/examples/ShapeOfConcurrency.ali"

if [[ ! -x "$ALIFIB" ]]; then
    echo "${RED}Building alifib...${RST}"
    (cd "$REPO" && cargo build --release)
fi

# ---------------------------------------------------------------------
# Utility: run `homology <type>` against the example file and print the
# resulting H_* / χ lines, without the REPL's `>> ` prefix.
# ---------------------------------------------------------------------
homology_of() {
    local type_name="$1"
    printf 'homology %s\nquit\n' "$type_name" \
        | "$ALIFIB" repl "$EXAMPLE" 2>/dev/null \
        | awk '/^>> +H_[0-9]+ =|^>> +χ =/ { sub(/^>> +/, ""); print }'
}

# ---------------------------------------------------------------------
# Compact "expected vs actual" check.  Takes the type name and a blob of
# expected homology text, and reports whether they match.
# ---------------------------------------------------------------------
check() {
    local type_name="$1"
    local expected="$2"
    local label="$3"

    local actual
    actual="$(homology_of "$type_name")"

    echo "  ${DIM}expected homology of ${label}:${RST}"
    printf '%s\n' "$expected" | sed "s/^/    ${CYN}/;s/\$/${RST}/"
    echo
    echo "  ${DIM}alifib homology of ${BOLD}$type_name${RST}${DIM}:${RST}"
    printf '%s\n' "$actual" | sed "s/^/    ${MAG}/;s/\$/${RST}/"
    echo

    if [[ "$(echo "$expected" | sort)" == "$(echo "$actual" | sort)" ]]; then
        echo "  ${GRN}${BOLD}✓${RST} match"
    else
        echo "  ${RED}${BOLD}✗${RST} mismatch"
        return 1
    fi
}

# ---------------------------------------------------------------------
# Typography
# ---------------------------------------------------------------------
hr() { printf "${DIM}%s${RST}\n" "──────────────────────────────────────────────────────────────────────"; }

banner() {
    echo
    hr
    printf "${BOLD}${YLW}  %s${RST}\n" "$1"
    if [[ $# -gt 1 ]]; then
        printf "${DIM}  %s${RST}\n" "$2"
    fi
    hr
    echo
}

act() {
    local num="$1"; shift
    local title="$1"; shift
    echo
    printf "${BOLD}${BLU}Act ${num}.${RST}  ${BOLD}%s${RST}\n" "$title"
    echo
}

say() { printf "${DIM}%s${RST}\n" "$*"; }
raw() { printf '%s\n' "$*"; }

# ---------------------------------------------------------------------
# Show a concise snippet of the polygraphic presentation
# ---------------------------------------------------------------------
show_snippet() {
    printf "${DIM}  presentation:${RST}\n"
    printf '%s\n' "$1" | sed 's/^/    /'
    echo
}

# ======================================================================

[[ -t 1 ]] && { clear 2>/dev/null || true; }

banner "The Shape of Concurrency" \
       "cellular homology of polygraphic rewriting systems"

cat <<EOF
${DIM}Polygraphs carry a regular directed complex on the nose. Forget the
source/target distinction on each cell and you obtain an ordinary CW
complex; alifib computes its integer cellular homology.

The four examples below each produce the homology of a classical
topological space. Where a coherence cell is absent the homology
detects the gap.${RST}
EOF

# ----------------------------------------------------------------------
act "I" "Pair — two concurrent threads"

show_snippet "Pair <<= {
    pt,
    a : pt -> pt,
    b : pt -> pt,
    comm : a b -> b a     (* Mazurkiewicz independence square *)
}"

say "Two threads, one atomic action each, with a 2-cell witnessing"
say "commutativity. Abelianising, d_2(comm) = (b+a)-(a+b) = 0, so H_2"
say "is free on comm. This is the concurrent-trace space of two independent"
say "threads — a 2-torus."
echo

check Pair \
"H_0 = Z
H_1 = Z^2
H_2 = Z
χ = 0" \
"T² (torus)"

# ----------------------------------------------------------------------
act "II" "Triple — three threads, pairwise commuting"

show_snippet "Triple <<= {
    pt,
    a, b, c : pt -> pt,
    comm_ab : a b -> b a,
    comm_ac : a c -> c a,
    comm_bc : b c -> c b
}"

say "Three threads, pairwise commuting. The 3-torus T³ would give"
say "H_0=Z, H_1=Z³, H_2=Z³, H_3=Z, χ=0."
echo

printf "  ${DIM}alifib homology of ${BOLD}Triple${RST}${DIM}:${RST}\n"
homology_of Triple | sed "s/^/    ${MAG}/;s/\$/${RST}/"
echo

say "No H_3, and χ=1 instead of 0. The presentation is complete at"
say "dimension 2 but has a 3-dimensional gap in its homology."

# ----------------------------------------------------------------------
act "III" "TripleCoh — adding the Zamolodchikov tetrahedron"

show_snippet "TripleCoh <<= {
    pt,
    a, b, c : pt -> pt,
    comm_ab, comm_ac, comm_bc  (as before),
    zamo : (comm_ab c)(b comm_ac)(comm_bc a)
         -> (a comm_bc)(comm_ac b)(c comm_ab)
}"

say "From abc to cba there are two pastings of three 2-cells. The 3-cell"
say "zamo identifies them — the Zamolodchikov tetrahedron. Adding it"
say "closes the homological gap."
echo

check TripleCoh \
"H_0 = Z
H_1 = Z^3
H_2 = Z^3
H_3 = Z
χ = 0" \
"T³ (3-torus)"

echo
say "The coherence cell is required: any coherent extension must identify"
say "those two pastings. This is Squier's homological finiteness theorem"
say "(Squier-Otto-Kobayashi 1994) in concrete form."

# ----------------------------------------------------------------------
act "IV" "Torsion — Smith Normal Form"

show_snippet "Torsion <<= {
    pt,
    a : pt -> pt,
    double : a a -> a a a a
}"

say "One generator, one 2-cell: d_2(double) = 4a - 2a = 2a. The cokernel"
say "is Z/2 — integer torsion detected via Smith Normal Form."
echo

check Torsion \
"H_0 = Z
H_1 = Z/2
H_2 = 0
χ = 1" \
"a 2-torsion group"

# ----------------------------------------------------------------------
banner "Summary"

cat <<EOF
  Pair       → H_*(T²)          two commuting threads
  Triple     → no H_3, χ = 1   dimension-2 coherence gap
  TripleCoh  → H_*(T³)          Zamolodchikov 3-cell closes it
  Torsion    → H_1 = Z/2        Smith Normal Form detects torsion

${DIM}Try it yourself:${RST}
  alifib repl examples/ShapeOfConcurrency.ali
  >> homology Pair
  >> homology Triple
  >> homology TripleCoh
  >> homology Torsion

EOF
