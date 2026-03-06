import * as fs from "fs";

// --- AST types ---

interface FunDecl {
  name: string;
  arity: number;
}

type Term =
  | { type: "var"; name: string }
  | { type: "app"; fun: string; args: Term[] };

interface Rule {
  lhs: Term;
  rhs: Term;
}

interface TRS {
  funs: FunDecl[];
  rules: Rule[];
}

// --- S-expression parser ---

type SExpr = string | SExpr[];

function tokenize(input: string): string[] {
  const tokens: string[] = [];
  let i = 0;
  while (i < input.length) {
    // skip whitespace
    if (/\s/.test(input[i])) { i++; continue; }
    // skip comments
    if (input[i] === ";") {
      while (i < input.length && input[i] !== "\n") i++;
      continue;
    }
    // parens
    if (input[i] === "(" || input[i] === ")") {
      tokens.push(input[i]);
      i++;
      continue;
    }
    // quoted symbol |...|
    if (input[i] === "|") {
      let s = "";
      i++; // skip opening |
      while (i < input.length && input[i] !== "|") {
        s += input[i];
        i++;
      }
      i++; // skip closing |
      tokens.push(s);
      continue;
    }
    // regular symbol
    let s = "";
    while (i < input.length && !/[\s();]/.test(input[i])) {
      // handle | inside a symbol
      if (input[i] === "|") break;
      s += input[i];
      i++;
    }
    if (s) tokens.push(s);
  }
  return tokens;
}

function parseSExprs(tokens: string[]): SExpr[] {
  const result: SExpr[] = [];
  let pos = 0;

  function parse(): SExpr {
    if (tokens[pos] === "(") {
      pos++; // skip (
      const list: SExpr[] = [];
      while (pos < tokens.length && tokens[pos] !== ")") {
        list.push(parse());
      }
      pos++; // skip )
      return list;
    } else {
      return tokens[pos++];
    }
  }

  while (pos < tokens.length) {
    result.push(parse());
  }
  return result;
}

// --- ARI format interpreter ---

function parseARI(input: string): TRS {
  const tokens = tokenize(input);
  const sexprs = parseSExprs(tokens);

  const funs: FunDecl[] = [];
  const funNames = new Set<string>();
  const rules: Rule[] = [];

  for (const sexpr of sexprs) {
    if (!Array.isArray(sexpr)) continue;
    const head = sexpr[0];
    if (head === "format") continue; // skip format declaration
    if (head === "fun") {
      const name = sexpr[1] as string;
      const arity = parseInt(sexpr[2] as string, 10);
      funs.push({ name, arity });
      funNames.add(name);
    }
    if (head === "rule") {
      const lhs = parseTerm(sexpr[1], funNames);
      const rhs = parseTerm(sexpr[2], funNames);
      rules.push({ lhs, rhs });
    }
  }

  return { funs, rules };
}

function parseTerm(sexpr: SExpr, funNames: Set<string>): Term {
  if (typeof sexpr === "string") {
    if (funNames.has(sexpr) ) {
      // 0-ary function symbol
      return { type: "app", fun: sexpr, args: [] };
    }
    return { type: "var", name: sexpr };
  }
  // (f t1 ... tn)
  const fun = sexpr[0] as string;
  const args = sexpr.slice(1).map((s) => parseTerm(s, funNames));
  return { type: "app", fun, args };
}

// --- Term analysis ---

/** Collect free variables in left-to-right order of first occurrence. */
function freeVarsOrdered(t: Term): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  function walk(t: Term) {
    if (t.type === "var") {
      if (!seen.has(t.name)) {
        seen.add(t.name);
        result.push(t.name);
      }
    } else {
      for (const arg of t.args) walk(arg);
    }
  }
  walk(t);
  return result;
}

/** Collect ALL variable occurrences in left-to-right leaf order (with repeats). */
function leafVars(t: Term): string[] {
  if (t.type === "var") return [t.name];
  const result: string[] = [];
  for (const arg of t.args) result.push(...leafVars(arg));
  return result;
}

// --- Name sanitization ---

function sanitize(name: string): string {
  let result = name
    .replace(/\+/g, "plus")
    .replace(/-/g, "minus")
    .replace(/\*/g, "times")
    .replace(/\//g, "div")
    .replace(/\\/g, "bslash")
    .replace(/</g, "lt")
    .replace(/>/g, "gt")
    .replace(/=/g, "eq")
    .replace(/!/g, "bang")
    .replace(/\?/g, "qmark")
    .replace(/\./g, "dot")
    .replace(/,/g, "comma")
    .replace(/'/g, "prime")
    .replace(/[^a-zA-Z0-9_]/g, "_");
  // Ensure valid identifier (can't start with digit)
  if (/^[0-9]/.test(result)) result = "c_" + result;
  return result;
}

// --- Diagram encoding ---

/**
 * Encode a term as an alifib 2-morphism diagram expression.
 *
 * Given a term t and an ordered list of input variables (the "context"),
 * produce a string representing an alifib diagram from ob^|vars| to ob.
 *
 * For constants (0-ary), the input is `unit` instead of nothing.
 *
 * Returns: { diagram: string, sourceWires: string[] }
 * where sourceWires describes the type of the source (each element is "ob" or "unit").
 */

interface Encoding {
  diagram: string;
  /** Wire types in order: "ob" for variable wires, "unit" for constant-introduced wires */
  wires: string[];
}

function hasConstants(funs: FunDecl[]): boolean {
  return funs.some((f) => f.arity === 0);
}

/** Check if any rule erases a variable (appears in LHS but not RHS). */
function hasErasingRules(rules: Rule[]): boolean {
  for (const rule of rules) {
    const lhsVars = new Set(freeVarsOrdered(rule.lhs));
    const rhsVars = new Set(freeVarsOrdered(rule.rhs));
    for (const v of lhsVars) {
      if (!rhsVars.has(v)) return true;
    }
  }
  return false;
}

/** Determine if unit/erase cells are needed (constants or erasing rules). */
function needsUnit(trs: TRS): boolean {
  return hasConstants(trs.funs) || hasErasingRules(trs.rules);
}

/**
 * Build a swap network to permute `from` into `to`.
 * Both arrays must be permutations of each other.
 * Returns a sequence of adjacent-swap instructions: [i, i+1] pairs.
 */
function computeSwaps(from: string[], to: string[]): number[] {
  const arr = [...from];
  const swaps: number[] = [];
  for (let i = 0; i < to.length; i++) {
    let j = i;
    while (j < arr.length && arr[j] !== to[i]) j++;
    // bubble arr[j] to position i
    while (j > i) {
      [arr[j - 1], arr[j]] = [arr[j], arr[j - 1]];
      swaps.push(j - 1);
      j--;
    }
  }
  return swaps;
}

/**
 * Encode a swap network as alifib diagram.
 * `swaps` is a list of positions where adjacent transpositions happen.
 * `width` is the total number of wires.
 */
function encodeSwapNetwork(swaps: number[], width: number): string | null {
  if (swaps.length === 0) return null;
  // Group consecutive non-overlapping swaps into layers
  const layers: number[][] = [];
  for (const s of swaps) {
    layers.push([s]); // simple: one swap per layer for correctness
  }
  const parts: string[] = [];
  for (const layer of layers) {
    const s = layer[0];
    // build: ob^s #0 swap #0 ob^(width-s-2)
    const pieces: string[] = [];
    for (let i = 0; i < s; i++) pieces.push("ob");
    pieces.push("swap");
    for (let i = s + 2; i < width; i++) pieces.push("ob");
    if (pieces.length === 1) {
      parts.push(pieces[0]);
    } else {
      // swap is dim-2, ob is dim-1: auto-tensors, but #0 is also safe
      parts.push("(" + pieces.join(" ") + ")");
    }
  }
  return parts.join(" ");
}

/**
 * Encode a duplication network for a variable that needs n copies.
 * Returns diagram from ob -> ob^n using copy cells.
 * For n=1: identity (null)
 * For n=2: copy
 * For n=3: copy (copy #0 ob)
 * etc.
 */
function encodeCopyTree(n: number): string | null {
  if (n <= 1) return null;
  if (n === 2) return "copy";
  // left-associative: copy, then copy the left copy, etc.
  let diagram = "copy";
  let width = 2;
  for (let i = 2; i < n; i++) {
    // copy the leftmost wire: copy #0 ob #0 ob ...
    const pieces: string[] = ["copy"];
    for (let j = 1; j < width; j++) pieces.push("ob");
    // copy is dim-2, ob is dim-1, so they auto-tensor. No #0 needed here.
    diagram = diagram + " (" + pieces.join(" ") + ")";
    width++;
  }
  return diagram;
}

/**
 * Build the "gather" phase: from the input variable context to the
 * required leaf-variable order of a term.
 *
 * Steps:
 * 1. For each variable used k times, duplicate it k-1 times (copy)
 * 2. For each variable used 0 times, erase it
 * 3. Reorder wires to match the leaf order
 */
function encodeGatherPhase(
  inputVars: string[],
  leafOrder: string[],
  useUnit: boolean
): { diagram: string | null; width: number } {
  // Count uses of each variable
  const useCounts = new Map<string, number>();
  for (const v of inputVars) useCounts.set(v, 0);
  for (const v of leafOrder) {
    useCounts.set(v, (useCounts.get(v) || 0) + 1);
  }

  const parts: string[] = [];
  let currentWires: string[] = [...inputVars];

  // Phase 1: copy/erase as needed
  // Process each input variable: if used k times, replace with k copies; if unused, erase
  const copyErasePieces: string[] = [];
  const afterCopyErase: string[] = [];
  for (const v of inputVars) {
    const k = useCounts.get(v)!;
    if (k === 0) {
      if (useUnit) {
        copyErasePieces.push("erase");
        afterCopyErase.push("__erased__");
      } else {
        // Without unit, can't erase. This is an error for non-linear rules.
        throw new Error(`Variable ${v} unused but no unit cell available`);
      }
    } else if (k === 1) {
      copyErasePieces.push("ob");
      afterCopyErase.push(v);
    } else {
      const ct = encodeCopyTree(k);
      copyErasePieces.push(ct || "ob");
      for (let i = 0; i < k; i++) afterCopyErase.push(v);
    }
  }

  // Check if any copy/erase happened
  const hasCopyErase = copyErasePieces.some((p) => p !== "ob");
  if (hasCopyErase) {
    if (copyErasePieces.length === 1) {
      parts.push(copyErasePieces[0]);
    } else {
      // Use #0 between pieces since copy/erase are dim-2
      // (ob is dim-1, auto-tensors, but copy/erase are dim-2)
      parts.push("(" + copyErasePieces.join(" #0 ") + ")");
    }
  }

  // Phase 2: remove erased wires using unit_l/unit_r
  let wipWires = [...afterCopyErase];
  if (useUnit) {
    // Remove __erased__ entries by applying unit_l/unit_r
    while (wipWires.includes("__erased__")) {
      const idx = wipWires.indexOf("__erased__");
      if (idx === 0 && wipWires.length > 1) {
        // unit on left: use unit_l
        const pieces: string[] = ["unit_l"];
        for (let i = 2; i < wipWires.length; i++) pieces.push("ob");
        if (pieces.length === 1) parts.push(pieces[0]);
        else parts.push("(" + pieces.join(" ") + ")");
        wipWires.splice(0, 2, wipWires[1]); // remove erased, keep next
      } else if (idx === wipWires.length - 1 && wipWires.length > 1) {
        // unit on right: use unit_r
        const pieces: string[] = [];
        for (let i = 0; i < idx - 1; i++) pieces.push("ob");
        pieces.push("unit_r");
        if (pieces.length === 1) parts.push(pieces[0]);
        else parts.push("(" + pieces.join(" ") + ")");
        wipWires.splice(idx - 1, 2, wipWires[idx - 1]); // keep prev, remove erased
      } else if (wipWires.length === 1) {
        // Just erased -> unit. Can't reduce further, but shouldn't happen
        // in a valid TRS (rule output should be a term, not empty)
        break;
      } else {
        // erased in middle: swap it toward an edge first
        // swap with right neighbor
        const swapPieces: string[] = [];
        for (let i = 0; i < idx; i++) swapPieces.push("ob");
        swapPieces.push("swap");
        for (let i = idx + 2; i < wipWires.length; i++) swapPieces.push("ob");
        // Wait, the erased wire is "unit" type, not "ob". swap is ob ob -> ob ob.
        // We need a unit_ob_swap or just use unit_swap: unit ob -> ob unit
        // For simplicity, use unit_swap for now
        // Actually let's just move all erased wires to the right first
        // using a different strategy: just remove rightmost erased first
        const ridx = wipWires.lastIndexOf("__erased__");
        if (ridx < wipWires.length - 1) {
          // Not at right edge, need to move it there
          // For now, swap right
          // Actually this is getting too complex. Let me use a simpler strategy.
          // Just shift erased to the nearest edge.
          break; // bail out, we'll handle this better later
        }
      }
    }
  }

  // Remove remaining __erased__ markers
  currentWires = wipWires.filter((w) => w !== "__erased__");

  // Phase 3: permute to match leaf order
  if (currentWires.length > 0 && leafOrder.length > 0) {
    const swaps = computeSwaps(currentWires, leafOrder);
    const swapDiag = encodeSwapNetwork(swaps, currentWires.length);
    if (swapDiag) parts.push(swapDiag);
  }

  if (parts.length === 0) return { diagram: null, width: leafOrder.length };
  return { diagram: parts.join(" "), width: leafOrder.length };
}

/**
 * Collect leaf slots of a term: variables as their name, constants as null.
 */
function leafSlots(t: Term, funMap: Map<string, number>): (string | null)[] {
  if (t.type === "var") return [t.name];
  const arity = funMap.get(t.fun)!;
  if (arity === 0) return [null]; // constant: one unit slot
  const result: (string | null)[] = [];
  for (const arg of t.args) result.push(...leafSlots(arg, funMap));
  return result;
}

/**
 * Encode a term as an alifib diagram expression.
 *
 * For terms with only variables (no constants), produces ob^|leafVars| -> ob.
 * For terms with constants, the constant slots expect unit wires.
 *
 * The `sourceSlots` parameter describes what each input wire is:
 * variable names get "ob" wires, null means a "unit" wire (for a constant).
 */
function encodeTerm(
  t: Term,
  vars: string[],
  funMap: Map<string, number>,
  useUnit: boolean
): string {
  return encodeTermInner(t, funMap);
}

/**
 * Inner encoding: produces a dim-2 diagram.
 * Source wires = leafSlots(t, funMap): one wire per leaf.
 *   - Variable leaf: ob wire (identity id_1)
 *   - Constant leaf: unit wire (constant cell c_name)
 * Target: ob
 */
function encodeTermInner(t: Term, funMap: Map<string, number>): string {
  if (t.type === "var") {
    return "id_1"; // identity 2-cell on ob
  }

  const f = sanitize(t.fun);
  const arity = funMap.get(t.fun)!;

  if (arity === 0) {
    // Constant: unit -> ob
    return f;
  }

  // Recursively encode subterms
  const subEncodings: string[] = [];
  for (const arg of t.args) {
    subEncodings.push(encodeTermInner(arg, funMap));
  }

  // Horizontal composition of sub-encodings, then apply f
  return composeHorizontalThenApply(subEncodings, f);
}

function wrapForHoriz(s: string): string {
  if (s === "ob") return "ob";
  if (s.includes(" ") || s.includes("#")) return "(" + s + ")";
  return s;
}

function composeHorizontalThenApply(subs: string[], f: string): string {
  if (subs.length === 0) return f;
  if (subs.length === 1) {
    const sub = subs[0];
    if (sub === "id_1") return f; // id ; f = f
    return sub + " " + f;
  }
  const parts = subs.map(wrapForHoriz);
  const allId = parts.every((p) => p === "id_1");
  if (allId) return f;
  // Use #0 for horizontal composition
  return "(" + parts.join(" #0 ") + ") " + f;
}

/**
 * Encode a term for use in a rule, given the rule's variable context.
 *
 * The rule's source wires are: ob^|vars| (for non-ground rules) or unit (for ground rules).
 *
 * Algorithm:
 * 1. Copy variables as needed (all copies are ob wires)
 * 2. Permute copies to match the target slot order (swap works on ob x ob)
 * 3. Erase copies at constant positions (ob → unit)
 * 4. Apply the raw term encoding
 */
function encodeTermForRule(
  t: Term,
  ruleVars: string[],
  funMap: Map<string, number>,
  useUnit: boolean,
  isGround: boolean
): string {
  const slots = leafSlots(t, funMap);
  const rawEncoding = encodeTermInner(t, funMap);

  // For ground rules (no variables), the source is unit
  if (isGround) {
    if (slots.length === 1) return rawEncoding;
    // Multiple constants need multiple unit wires but we only have one.
    // For now, just return the raw encoding (may need unit duplication).
    return rawEncoding;
  }

  // Non-ground rule: source is ob^|ruleVars|
  const varSlots = slots.filter((s) => s !== null) as string[];
  const constCount = slots.filter((s) => s === null).length;

  // If no constants, just do the standard gather (copy/erase/swap on ob wires)
  if (constCount === 0) {
    const { diagram: gatherDiag } = encodeGatherPhase(
      ruleVars, varSlots, useUnit
    );
    if (gatherDiag) return gatherDiag + " " + rawEncoding;
    return rawEncoding;
  }

  // With constants: produce unit wires from ob wires via erase.
  // Strategy:
  // 1. Copy variables to get enough ob wires for all slots
  //    (one per variable slot + one per constant slot to be erased)
  // 2. Permute all ob wires to match target slot order
  // 3. Erase wires at constant positions
  // 4. Apply raw encoding

  const parts: string[] = [];

  // Count how many copies each variable needs
  const varUseCounts = new Map<string, number>();
  for (const v of ruleVars) varUseCounts.set(v, 0);
  for (const s of varSlots) {
    varUseCounts.set(s, (varUseCounts.get(s) || 0) + 1);
  }
  // Add extra copies for constant slots (donated by first variable)
  const donorVar = ruleVars[0];
  varUseCounts.set(donorVar, (varUseCounts.get(donorVar) || 0) + constCount);

  // Step 1: Copy phase
  const copyPieces: string[] = [];
  const afterCopy: string[] = [];
  for (const v of ruleVars) {
    const k = varUseCounts.get(v)!;
    if (k === 0) {
      // Variable unused: erase it (we're still in all-ob phase,
      // but unused vars need to be handled)
      // For now, just keep an id and mark for later erasure
      copyPieces.push("id_1");
      afterCopy.push("__unused_" + v);
    } else if (k === 1) {
      copyPieces.push("id_1");
      afterCopy.push(v);
    } else {
      const ct = encodeCopyTree(k);
      copyPieces.push(ct || "id_1");
      for (let i = 0; i < k; i++) afterCopy.push(v);
    }
  }
  const hasCopy = copyPieces.some((p) => p !== "id_1");
  if (hasCopy) {
    if (copyPieces.length === 1) {
      parts.push(copyPieces[0]);
    } else {
      parts.push("(" + copyPieces.join(" #0 ") + ")");
    }
  }

  // After copy: all wires are ob. Now assign labels to match target slots.
  // Target slots: each slot is either a variable name (ob) or null (unit from erase).
  // We need to permute afterCopy to match the slots (with extra donor copies at const positions).

  // Build target wire assignment
  const targetWires: string[] = [];
  const donorCopiesNeeded = varUseCounts.get(donorVar)! - constCount;
  // donorCopiesNeeded = actual uses of donor variable in term
  let donorForVarCount = 0;
  let donorForConstCount = 0;
  for (const s of slots) {
    if (s !== null) {
      if (s === donorVar) {
        targetWires.push(`${s}_var_${donorForVarCount++}`);
      } else {
        const idx = targetWires.filter((w) => w.startsWith(s + "_")).length;
        targetWires.push(`${s}_var_${idx}`);
      }
    } else {
      targetWires.push(`${donorVar}_const_${donorForConstCount++}`);
    }
  }

  // Build current wire labels
  const currentWires: string[] = [];
  const varCounters = new Map<string, number>();
  const constCounters = new Map<string, number>();
  for (const v of afterCopy) {
    if (v.startsWith("__unused_")) {
      currentWires.push(v);
      continue;
    }
    const vc = varCounters.get(v) || 0;
    const totalVarUses = varSlots.filter((s) => s === v).length;
    if (vc < totalVarUses) {
      currentWires.push(`${v}_var_${vc}`);
      varCounters.set(v, vc + 1);
    } else {
      const cc = constCounters.get(v) || 0;
      currentWires.push(`${v}_const_${cc}`);
      constCounters.set(v, cc + 1);
    }
  }

  // Step 2: Permute (all ob wires, swap works)
  // Filter out unused wires (shouldn't have any if all vars are used)
  const swaps = computeSwaps(currentWires, targetWires);
  const swapDiag = encodeSwapNetwork(swaps, currentWires.length);
  if (swapDiag) parts.push(swapDiag);

  // Step 3: Erase wires at constant positions
  const erasePieces: string[] = [];
  for (const s of slots) {
    if (s !== null) {
      erasePieces.push("id_1");
    } else {
      erasePieces.push("erase");
    }
  }
  const hasErase = erasePieces.some((p) => p !== "id_1");
  if (hasErase) {
    if (erasePieces.length === 1) {
      parts.push(erasePieces[0]);
    } else {
      parts.push("(" + erasePieces.join(" #0 ") + ")");
    }
  }

  // Step 4: Apply raw encoding
  if (parts.length === 0) return rawEncoding;
  return parts.join(" ") + " " + rawEncoding;
}

// --- Generate alifib output ---

function generateAlifib(trs: TRS, moduleName: string): string {
  const lines: string[] = [];
  const useUnit = needsUnit(trs);
  const funMap = new Map<string, number>();
  for (const f of trs.funs) funMap.set(f.name, f.arity);

  lines.push(`(* Generated from ARI format: ${moduleName} *)`);
  lines.push("");
  lines.push("@Type");

  // Equation type: an invertible 3-cell between parallel 2-morphisms
  lines.push("Eq <<= {");
  lines.push("  pt,");
  lines.push("  dom : pt -> pt,");
  lines.push("  cod : pt -> pt,");
  lines.push("  lhs : dom -> cod,");
  lines.push("  rhs : dom -> cod,");
  lines.push("  dir : lhs -> rhs,");
  lines.push("  inv : rhs -> lhs");
  lines.push("},");
  lines.push("");

  // Build the main TRS complex
  lines.push(`${sanitize(moduleName)} <<= {`);
  lines.push("  pt,");
  lines.push("  ob : pt -> pt,");
  if (useUnit) {
    lines.push("  unit : pt -> pt,");
  }

  // Structural cells
  lines.push("");
  lines.push("  (* Structural cells *)");
  lines.push("  copy : ob -> ob ob,");
  lines.push("  swap : ob ob -> ob ob,");
  if (useUnit) {
    lines.push("  erase : ob -> unit,");
    lines.push("  unit_l : unit ob -> ob,");
    lines.push("  unit_r : ob unit -> ob,");
  }

  // Function symbols
  lines.push("");
  lines.push("  (* Function symbols *)");
  for (const f of trs.funs) {
    const name = sanitize(f.name);
    if (f.arity === 0) {
      lines.push(`  ${name} : unit -> ob,`);
    } else {
      const src = Array(f.arity).fill("ob").join(" ");
      lines.push(`  ${name} : ${src} -> ob,`);
    }
  }

  // Identity 2-cells for each width (needed for structural equations).
  // id_n : ob^n -> ob^n. We need these as generators because
  // #0 composites like (id #0 id) are non-globular in alifib.
  const maxArity = Math.max(1, ...trs.funs.map((f) => f.arity));
  lines.push("");
  lines.push("  (* Identity 2-cells *)");
  for (let i = 1; i <= Math.max(maxArity, 2); i++) {
    const obs = Array(i).fill("ob").join(" ");
    lines.push(`  id_${i} : ${obs} -> ${obs},`);
  }

  // Naturality of copy: for each f of arity n,
  // f ; copy = (copy^n) ; permute ; (f ⊗ f)
  lines.push("");
  lines.push("  (* Naturality of copy *)");
  for (const f of trs.funs) {
    if (f.arity === 0) continue;
    const name = sanitize(f.name);
    const lhs = `${name} copy`;

    if (f.arity === 1) {
      // f copy = copy (f #0 f)
      const rhs = `copy (${name} #0 ${name})`;
      lines.push(
        `  attach Copy_${name} :: Eq along [` +
          ` lhs => ${lhs}, rhs => ${rhs} ],`
      );
    } else if (f.arity === 2) {
      // f copy = (copy #0 copy) (ob swap ob) (f #0 f)
      const rhs = `(copy #0 copy) (ob swap ob) (${name} #0 ${name})`;
      lines.push(
        `  attach Copy_${name} :: Eq along [` +
          ` lhs => ${lhs}, rhs => ${rhs} ],`
      );
    }
    // Higher arities: TODO
  }

  // Structural equations (as direct 3-cells)
  lines.push("");
  lines.push("  (* Structural equations *)");
  lines.push("  id_1_idem : id_1 id_1 -> id_1,");
  lines.push("  id_1_idem_inv : id_1 -> id_1 id_1,");
  lines.push("  swap_inv : swap swap -> id_2,");
  lines.push("  swap_inv_inv : id_2 -> swap swap,");
  lines.push("  copy_comm : copy swap -> copy,");
  lines.push("  copy_comm_inv : copy -> copy swap,");

  // Rewrite rules
  lines.push("");
  lines.push("  (* Rewrite rules *)");
  for (let i = 0; i < trs.rules.length; i++) {
    const rule = trs.rules[i];
    const lhsVars = freeVarsOrdered(rule.lhs);
    const rhsVars = freeVarsOrdered(rule.rhs);

    // Check that RHS vars ⊆ LHS vars
    const lhsSet = new Set(lhsVars);
    for (const v of rhsVars) {
      if (!lhsSet.has(v)) {
        lines.push(
          `  (* SKIPPED rule ${i + 1}: RHS has extra variable ${v} *)`
        );
        continue;
      }
    }

    const vars = lhsVars; // rule variables = LHS variables
    const isGround = vars.length === 0;

    try {
      const lhsDiag = encodeTermForRule(rule.lhs, vars, funMap, useUnit, isGround);
      const rhsDiag = encodeTermForRule(rule.rhs, vars, funMap, useUnit, isGround);

      lines.push(`  (* Rule ${i + 1}: ${termToString(rule.lhs)} -> ${termToString(rule.rhs)} *)`);
      lines.push(`  rule_${i + 1} : ${lhsDiag} -> ${rhsDiag},`);
    } catch (e: any) {
      lines.push(`  (* SKIPPED rule ${i + 1}: ${e.message} *)`);
    }
  }

  lines.push("}");

  // Clean up trailing comma before closing brace
  const result = lines.join("\n").replace(/,\n\}/, "\n}");
  return result;
}

function termToString(t: Term): string {
  if (t.type === "var") return t.name;
  if (t.args.length === 0) return t.fun;
  return `${t.fun}(${t.args.map(termToString).join(", ")})`;
}

// --- Main ---

function main() {
  const args = process.argv.slice(2);
  if (args.length < 1) {
    console.error("Usage: ari2ali <input.ari> [output.ali]");
    process.exit(1);
  }

  const inputFile = args[0];
  const outputFile = args[1] || inputFile.replace(/\.ari$/, ".ali");
  let moduleName = inputFile
    .replace(/.*\//, "")
    .replace(/\.ari$/, "")
    .replace(/[^a-zA-Z0-9]/g, "_");
  // Ensure valid identifier (can't start with digit)
  if (/^[0-9]/.test(moduleName)) moduleName = "TRS_" + moduleName;

  const input = fs.readFileSync(inputFile, "utf-8");
  const trs = parseARI(input);

  console.error(`Parsed: ${trs.funs.length} function symbols, ${trs.rules.length} rules`);
  for (const f of trs.funs) {
    console.error(`  ${f.name}/${f.arity}`);
  }

  const output = generateAlifib(trs, moduleName);

  if (outputFile === "-") {
    console.log(output);
  } else {
    fs.writeFileSync(outputFile, output + "\n");
    console.error(`Written to ${outputFile}`);
  }
}

main();
