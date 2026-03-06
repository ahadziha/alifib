import { SExpr, Term, TRS } from "./types";

export function tokenize(input: string): string[] {
  const tokens: string[] = [];
  let i = 0;
  while (i < input.length) {
    if (/\s/.test(input[i])) { i++; continue; }
    if (input[i] === ";") {
      while (i < input.length && input[i] !== "\n") i++;
      continue;
    }
    if (input[i] === "(" || input[i] === ")") {
      tokens.push(input[i]);
      i++;
      continue;
    }
    if (input[i] === "|") {
      let s = "";
      i++;
      while (i < input.length && input[i] !== "|") {
        s += input[i];
        i++;
      }
      i++;
      tokens.push(s);
      continue;
    }
    let s = "";
    while (i < input.length && !/[\s();]/.test(input[i])) {
      if (input[i] === "|") break;
      s += input[i];
      i++;
    }
    if (s) tokens.push(s);
  }
  return tokens;
}

export function parseSExprs(tokens: string[]): SExpr[] {
  const result: SExpr[] = [];
  let pos = 0;

  function parse(): SExpr {
    if (tokens[pos] === "(") {
      pos++;
      const list: SExpr[] = [];
      while (pos < tokens.length && tokens[pos] !== ")") {
        list.push(parse());
      }
      pos++;
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

export function parseTerm(sexpr: SExpr, funNames: Set<string>): Term {
  if (typeof sexpr === "string") {
    if (funNames.has(sexpr)) {
      return { type: "app", fun: sexpr, args: [] };
    }
    return { type: "var", name: sexpr };
  }
  const fun = sexpr[0] as string;
  const args = sexpr.slice(1).map((s) => parseTerm(s, funNames));
  return { type: "app", fun, args };
}

export function parseARI(input: string): TRS {
  const tokens = tokenize(input);
  const sexprs = parseSExprs(tokens);

  const funs: { name: string; arity: number }[] = [];
  const funNames = new Set<string>();
  const rules: { lhs: Term; rhs: Term }[] = [];

  for (const sexpr of sexprs) {
    if (!Array.isArray(sexpr)) continue;
    const head = sexpr[0];
    if (head === "format") continue;
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
