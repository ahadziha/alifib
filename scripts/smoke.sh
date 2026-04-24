#!/usr/bin/env bash
# ======================================================================
#   The Shape of Concurrency — alifib smoke-and-mirrors demo
# ----------------------------------------------------------------------
#   Computes the cellular homology of four polygraphic rewriting systems
#   and checks that each one matches the integer homology of a named
#   topological space.
#
#   This is a feature no 1-D term rewriter (Maude, ELAN, Stratego, CafeOBJ)
#   can reproduce: the homology *is* the topology of the rewriting
#   complex, and it detects coherence obstructions that term rewriters
#   cannot see.
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
#   The show
# ======================================================================

[[ -t 1 ]] && { clear 2>/dev/null || true; }

banner "The Shape of Concurrency" \
       "cellular homology of polygraphic rewriting systems"

cat <<EOF
${DIM}Polygraphs — Burroni's generalisation of term-rewriting systems to higher
dimensions — are CW complexes. Every rewriting system has an underlying
topology, and that topology records information no 1-D tool can see.

Tonight, alifib will compute the homology of four concurrent-rewriting
presentations. Each one turns out to be the homology of a classical
topological space. The punchline: when a presentation is *missing* a
coherence cell, the topology ${RST}${BOLD}${YLW}screams it${RST}${DIM}.${RST}
EOF

# ----------------------------------------------------------------------
act "I" "Two concurrent threads weave a torus"

show_snippet "Pair <<= {
    pt,
    a : pt -> pt,
    b : pt -> pt,
    comm : a b -> b a     (* Mazurkiewicz independence square *)
}"

say "Two threads doing one atomic action each. The 2-cell 'comm' says they"
say "commute: abba is abab rearranged. Abelianising the chain complex, d_2"
say "of comm is (b+a) - (a+b) = 0 — so H_2 inherits the whole 2-cell."
echo

check Pair \
"H_0 = Z
H_1 = Z^2
H_2 = Z
χ = 0" \
"T² (torus)"

say "Two generators glued by one commuter = the torus. This is textbook"
say "higher-dimensional-automata: the space of concurrent traces of"
say "two independent threads IS a 2-torus."

# ----------------------------------------------------------------------
act "II" "Three threads, naively"

show_snippet "Triple <<= {
    pt,
    a, b, c : pt -> pt,
    comm_ab : a b -> b a,
    comm_ac : a c -> c a,
    comm_bc : b c -> c b
}"

say "Three threads, pairwise commuting. If this were the 3-torus T³ we"
say "would expect H_0=Z, H_1=Z³, H_2=Z³, H_3=Z, χ=0."
echo

printf "  ${DIM}alifib homology of ${BOLD}Triple${RST}${DIM}:${RST}\n"
homology_of Triple | sed "s/^/    ${MAG}/;s/\$/${RST}/"
echo

say "${BOLD}${RED}No H_3 cell — and the Euler characteristic is 1, not 0.${RST}"
say "This ${BOLD}isn't${RST} the 3-torus. The presentation is complete at dimension 2"
say "but topologically broken at dimension 3. Something is missing."

# ----------------------------------------------------------------------
act "III" "The Zamolodchikov tetrahedron"

show_snippet "TripleCoh <<= {
    pt,
    a, b, c : pt -> pt,
    comm_ab, comm_ac, comm_bc  (as before),
    zamo : (comm_ab c)(b comm_ac)(comm_bc a)
         -> (a comm_bc)(comm_ac b)(c comm_ab)
}"

say "From abc to cba there are two 'reduction strategies' — two pastings"
say "of three 2-cells apiece. Asserting they're equal is a 3-cell — the"
say "Zamolodchikov tetrahedron. ${BOLD}${YLW}Adding this single 3-cell fills the hole.${RST}"
echo

check TripleCoh \
"H_0 = Z
H_1 = Z^3
H_2 = Z^3
H_3 = Z
χ = 0" \
"T³ (3-torus)"

echo
say "${BOLD}The coherence cell wasn't a stylistic choice.${RST} It's forced on us by"
say "algebraic topology. Squier's homological finiteness theorem made this"
say "rigorous in 1994; alifib computes it in milliseconds."

# ----------------------------------------------------------------------
act "IV" "A rewriting system with torsion"

show_snippet "Torsion <<= {
    pt,
    a : pt -> pt,
    double : a a -> a a a a
}"

say "One generator, one 2-cell sending a² to a⁴. Abelianising, d_2(double)"
say "= 4·a - 2·a = 2·a. The cokernel of 'multiplication by 2' is Z/2."
echo

check Torsion \
"H_0 = Z
H_1 = Z/2
H_2 = 0
χ = 1" \
"a 2-torsion group"

say "alifib computes integer homology with torsion via Smith Normal Form."
say "A term rewriter would see a² ↦ a⁴ as 'non-terminating modulo power';"
say "alifib sees it as a Moore space with Z/2 in H_1."

# ----------------------------------------------------------------------
banner "Recap" "alifib did what term rewriters cannot"

cat <<EOF
${BOLD}Summary.${RST}
  Pair       → H_*(T²) — two concurrent threads weave a torus
  Triple     → missing H_3, χ = 1 — presentation is not coherent
  TripleCoh  → H_*(T³) — the Zamolodchikov 3-cell fills the hole
  Torsion    → H_1 = Z/2 — Smith Normal Form detects torsion

${BOLD}Why this is beyond 1-D term rewriting.${RST}
  Maude and friends see rewriting sequences. alifib sees the CW complex
  whose 0-cells are objects, 1-cells are morphisms, 2-cells are rewrite
  rules, and 3-cells are coherences. Its homology invariants pick up on
  the Squier obstruction to finite convergent presentations — something
  that is, definitionally, invisible to a 1-D engine.

${BOLD}Try it yourself.${RST}
  alifib repl examples/ShapeOfConcurrency.ali
  >> homology Pair
  >> homology Triple       ${DIM}(the one that's wrong)${RST}
  >> homology TripleCoh    ${DIM}(the fix)${RST}
  >> homology Torsion

EOF
