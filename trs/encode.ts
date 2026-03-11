import { Term, TRS } from "./types";

/** Collect free variables in left-to-right order of first occurrence. */
export function freeVarsOrdered(t: Term): string[] {
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
export function leafVars(t: Term): string[] {
  if (t.type === "var") return [t.name];
  const result: string[] = [];
  for (const arg of t.args) result.push(...leafVars(arg));
  return result;
}

export function sanitize(name: string): string {
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
  if (/^[0-9]/.test(result)) result = "c_" + result;
  return result;
}

export function hasConstants(funs: { arity: number }[]): boolean {
  return funs.some((f) => f.arity === 0);
}

/** Check if any rule erases a variable (appears in LHS but not RHS). */
export function hasErasingRules(rules: { lhs: Term; rhs: Term }[]): boolean {
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
export function needsUnit(trs: TRS): boolean {
  return hasConstants(trs.funs) || hasErasingRules(trs.rules);
}

/**
 * Build a swap network to permute `from` into `to`.
 * Both arrays must be permutations of each other.
 * Returns a sequence of adjacent-swap positions.
 */
export function computeSwaps(from: string[], to: string[]): number[] {
  const arr = [...from];
  const swaps: number[] = [];
  for (let i = 0; i < to.length; i++) {
    let j = i;
    while (j < arr.length && arr[j] !== to[i]) j++;
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
export function encodeSwapNetwork(swaps: number[], width: number): string | null {
  if (swaps.length === 0) return null;
  const layers: number[][] = [];
  for (const s of swaps) {
    layers.push([s]);
  }
  const parts: string[] = [];
  for (const layer of layers) {
    const s = layer[0];
    const pieces: string[] = [];
    for (let i = 0; i < s; i++) pieces.push("ob");
    pieces.push("swap");
    for (let i = s + 2; i < width; i++) pieces.push("ob");
    if (pieces.length === 1) {
      parts.push(pieces[0]);
    } else {
      parts.push("(" + pieces.join(" ") + ")");
    }
  }
  return parts.join(" ");
}

/**
 * Encode a duplication network for a variable that needs n copies.
 * Returns diagram from ob -> ob^n using copy cells.
 */
export function encodeCopyTree(n: number): string | null {
  if (n <= 1) return null;
  if (n === 2) return "copy";
  let diagram = "copy";
  let width = 2;
  for (let i = 2; i < n; i++) {
    const pieces: string[] = ["copy"];
    for (let j = 1; j < width; j++) pieces.push("ob");
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
export function encodeGatherPhase(
  inputVars: string[],
  leafOrder: string[],
  useUnit: boolean
): { diagram: string | null; width: number } {
  const useCounts = new Map<string, number>();
  for (const v of inputVars) useCounts.set(v, 0);
  for (const v of leafOrder) {
    useCounts.set(v, (useCounts.get(v) || 0) + 1);
  }

  const parts: string[] = [];
  let currentWires: string[] = [...inputVars];

  // Phase 1: copy/erase as needed
  const copyErasePieces: string[] = [];
  const afterCopyErase: string[] = [];
  for (const v of inputVars) {
    const k = useCounts.get(v)!;
    if (k === 0) {
      if (useUnit) {
        copyErasePieces.push("erase");
        afterCopyErase.push("__erased__");
      } else {
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

  const hasCopyErase = copyErasePieces.some((p) => p !== "ob");
  if (hasCopyErase) {
    if (copyErasePieces.length === 1) {
      parts.push(copyErasePieces[0]);
    } else {
      parts.push("(" + copyErasePieces.join(" #0 ") + ")");
    }
  }

  // Phase 2: remove erased wires using unit_l/unit_r
  let wipWires = [...afterCopyErase];
  if (useUnit) {
    while (wipWires.includes("__erased__")) {
      const idx = wipWires.indexOf("__erased__");
      if (idx === 0 && wipWires.length > 1) {
        const pieces: string[] = ["unit_l"];
        for (let i = 2; i < wipWires.length; i++) pieces.push("ob");
        if (pieces.length === 1) parts.push(pieces[0]);
        else parts.push("(" + pieces.join(" ") + ")");
        wipWires.splice(0, 2, wipWires[1]);
      } else if (idx === wipWires.length - 1 && wipWires.length > 1) {
        const pieces: string[] = [];
        for (let i = 0; i < idx - 1; i++) pieces.push("ob");
        pieces.push("unit_r");
        if (pieces.length === 1) parts.push(pieces[0]);
        else parts.push("(" + pieces.join(" ") + ")");
        wipWires.splice(idx - 1, 2, wipWires[idx - 1]);
      } else if (wipWires.length === 1) {
        break;
      } else {
        const ridx = wipWires.lastIndexOf("__erased__");
        if (ridx < wipWires.length - 1) {
          break;
        }
      }
    }
  }

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
export function leafSlots(t: Term, funMap: Map<string, number>): (string | null)[] {
  if (t.type === "var") return [t.name];
  const arity = funMap.get(t.fun)!;
  if (arity === 0) return [null];
  const result: (string | null)[] = [];
  for (const arg of t.args) result.push(...leafSlots(arg, funMap));
  return result;
}

/**
 * Inner encoding: produces a dim-2 diagram.
 * Source wires = leafSlots(t, funMap): one wire per leaf.
 *   - Variable leaf: ob wire (identity id_1)
 *   - Constant leaf: unit wire (constant cell)
 * Target: ob
 */
export function encodeTermInner(t: Term, funMap: Map<string, number>): string {
  if (t.type === "var") {
    return "id_1";
  }

  const f = sanitize(t.fun);
  const arity = funMap.get(t.fun)!;

  if (arity === 0) {
    return f;
  }

  const subEncodings: string[] = [];
  for (const arg of t.args) {
    subEncodings.push(encodeTermInner(arg, funMap));
  }

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
    if (sub === "id_1") return f;
    return sub + " " + f;
  }
  const parts = subs.map(wrapForHoriz);
  const allId = parts.every((p) => p === "id_1");
  if (allId) return f;
  return "(" + parts.join(" #0 ") + ") " + f;
}

/**
 * Encode a term for use in a rule, given the rule's variable context.
 *
 * Algorithm:
 * 1. Copy variables as needed (all copies are ob wires)
 * 2. Permute copies to match the target slot order (swap works on ob x ob)
 * 3. Erase copies at constant positions (ob -> unit)
 * 4. Apply the raw term encoding
 */
export function encodeTermForRule(
  t: Term,
  ruleVars: string[],
  funMap: Map<string, number>,
  useUnit: boolean,
  isGround: boolean
): string {
  const slots = leafSlots(t, funMap);
  const rawEncoding = encodeTermInner(t, funMap);

  if (isGround) {
    if (slots.length === 1) return rawEncoding;
    return rawEncoding;
  }

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

  // Build target wire assignment
  const targetWires: string[] = [];
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

export function termToString(t: Term): string {
  if (t.type === "var") return t.name;
  if (t.args.length === 0) return t.fun;
  return `${t.fun}(${t.args.map(termToString).join(", ")})`;
}
