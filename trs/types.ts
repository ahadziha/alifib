export interface FunDecl {
  name: string;
  arity: number;
}

export type Term =
  | { type: "var"; name: string }
  | { type: "app"; fun: string; args: Term[] };

export interface Rule {
  lhs: Term;
  rhs: Term;
}

export interface TRS {
  funs: FunDecl[];
  rules: Rule[];
}

export type SExpr = string | SExpr[];
