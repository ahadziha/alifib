import { TRS } from "./types";
import {
  sanitize,
  needsUnit,
  hasConstants,
  freeVarsOrdered,
  encodeTermForRule,
  termToString,
} from "./encode";

export function generateAlifib(trs: TRS, moduleName: string): string {
  const lines: string[] = [];
  const useUnit = needsUnit(trs);
  const funMap = new Map<string, number>();
  for (const f of trs.funs) funMap.set(f.name, f.arity);

  lines.push(`(* Generated from ARI format: ${moduleName} *)`);
  lines.push("");
  lines.push("@Type");

  // Equation type
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

  // Main TRS complex
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

  // Identity 2-cells
  const maxArity = Math.max(1, ...trs.funs.map((f) => f.arity));
  lines.push("");
  lines.push("  (* Identity 2-cells *)");
  for (let i = 1; i <= Math.max(maxArity, 2); i++) {
    const obs = Array(i).fill("ob").join(" ");
    lines.push(`  id_${i} : ${obs} -> ${obs},`);
  }

  // Naturality of copy
  lines.push("");
  lines.push("  (* Naturality of copy *)");
  for (const f of trs.funs) {
    if (f.arity === 0) continue;
    const name = sanitize(f.name);
    const lhs = `${name} copy`;

    if (f.arity === 1) {
      const rhs = `copy (${name} #0 ${name})`;
      lines.push(
        `  attach Copy_${name} :: Eq along [` +
          ` lhs => ${lhs}, rhs => ${rhs} ],`
      );
    } else if (f.arity === 2) {
      const rhs = `(copy #0 copy) (ob swap ob) (${name} #0 ${name})`;
      lines.push(
        `  attach Copy_${name} :: Eq along [` +
          ` lhs => ${lhs}, rhs => ${rhs} ],`
      );
    }
    // Higher arities: TODO
  }

  // Structural equations
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

    const lhsSet = new Set(lhsVars);
    for (const v of rhsVars) {
      if (!lhsSet.has(v)) {
        lines.push(
          `  (* SKIPPED rule ${i + 1}: RHS has extra variable ${v} *)`
        );
        continue;
      }
    }

    const vars = lhsVars;
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

  const result = lines.join("\n").replace(/,\n\}/, "\n}");
  return result;
}
