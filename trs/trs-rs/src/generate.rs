use std::collections::HashMap;

use alifib::language::ast::{self, Spanned};

use crate::diag::{hseq, seq, syn, Diag};
use crate::encode::{
    encode_term_for_rule, free_vars_ordered, needs_unit, sanitize, term_to_string,
};
use crate::types::TRS;

// ---------------------------------------------------------------------------
// Low-level AST helpers
// ---------------------------------------------------------------------------

/// A simple generator declaration: `name` or `name : src -> tgt`.
fn gen_instr(name: &str, src: Option<Diag>, tgt: Option<Diag>) -> Spanned<ast::ComplexInstr> {
    syn(ast::ComplexInstr::NameWithBoundary(ast::NameWithBoundary {
        name: syn(name.to_owned()),
        boundary: match (src, tgt) {
            (Some(s), Some(t)) => Some(syn(ast::Boundary {
                source: syn(s.into_ast()),
                target: syn(t.into_ast()),
            })),
            _ => None,
        },
    }))
}

/// An attach instruction:
/// `attach name :: type_addr along [gen1 => val1, ...]`
fn attach_instr(
    name: &str,
    type_addr: &[&str],
    clauses: Vec<(&str, Diag)>,
) -> Spanned<ast::ComplexInstr> {
    let address: ast::Address = type_addr
        .iter()
        .map(|s| syn(s.to_string()))
        .collect();
    let pmap_clauses: Vec<Spanned<ast::PartialMapClause>> = clauses
        .into_iter()
        .map(|(gen_name, val)| {
            syn(ast::PartialMapClause {
                lhs: syn(Diag::atom(gen_name).into_ast()),
                rhs: syn(val.into_ast()),
            })
        })
        .collect();
    syn(ast::ComplexInstr::AttachStmt(ast::AttachStmt {
        name: syn(name.to_owned()),
        address: syn(address),
        along: Some(syn(ast::PartialMapDef::Ext(ast::PartialMapExt {
            prefix: None,
            clauses: pmap_clauses,
        }))),
    }))
}

/// A generator type declaration: `Name <<= { body }` inside a `@Type` block.
fn type_generator(
    name: &str,
    body: Vec<Spanned<ast::ComplexInstr>>,
) -> Spanned<ast::TypeInst> {
    syn(ast::TypeInst::Generator(ast::Generator {
        name: syn(ast::NameWithBoundary {
            name: syn(name.to_owned()),
            boundary: None,
        }),
        complex: syn(ast::Complex::Block {
            address: None,
            body,
        }),
    }))
}

/// N copies of `ob` composed vertically (i.e. as a PrincipalPaste).
fn n_obs(n: usize) -> Diag {
    seq((0..n).map(|_| Diag::atom("ob")).collect())
}

// ---------------------------------------------------------------------------
// Main program generation
// ---------------------------------------------------------------------------

/// Build the complete `ast::Program` mirroring `generateAlifib` from the TypeScript.
pub fn generate_program(trs: &TRS, module_name: &str) -> ast::Program {
    let use_unit = needs_unit(trs);
    let fun_map: HashMap<String, usize> =
        trs.funs.iter().map(|f| (f.name.clone(), f.arity)).collect();

    // ---- Build the Eq type body ----
    let eq_body: Vec<Spanned<ast::ComplexInstr>> = vec![
        gen_instr("pt", None, None),
        gen_instr("dom", Some(Diag::atom("pt")), Some(Diag::atom("pt"))),
        gen_instr("cod", Some(Diag::atom("pt")), Some(Diag::atom("pt"))),
        gen_instr(
            "lhs",
            Some(Diag::atom("dom")),
            Some(Diag::atom("cod")),
        ),
        gen_instr(
            "rhs",
            Some(Diag::atom("dom")),
            Some(Diag::atom("cod")),
        ),
        gen_instr(
            "dir",
            Some(Diag::atom("lhs")),
            Some(Diag::atom("rhs")),
        ),
        gen_instr(
            "inv",
            Some(Diag::atom("rhs")),
            Some(Diag::atom("lhs")),
        ),
    ];

    // ---- Build the TRS type body ----
    let mut trs_body: Vec<Spanned<ast::ComplexInstr>> = Vec::new();

    // pt
    trs_body.push(gen_instr("pt", None, None));
    // ob : pt -> pt
    trs_body.push(gen_instr(
        "ob",
        Some(Diag::atom("pt")),
        Some(Diag::atom("pt")),
    ));
    // unit : pt -> pt (if needed)
    if use_unit {
        trs_body.push(gen_instr(
            "unit",
            Some(Diag::atom("pt")),
            Some(Diag::atom("pt")),
        ));
    }

    // Structural cells
    // copy : ob -> ob ob
    trs_body.push(gen_instr(
        "copy",
        Some(Diag::atom("ob")),
        Some(n_obs(2)),
    ));
    // swap : ob ob -> ob ob
    trs_body.push(gen_instr(
        "swap",
        Some(n_obs(2)),
        Some(n_obs(2)),
    ));
    if use_unit {
        // erase : ob -> unit
        trs_body.push(gen_instr(
            "erase",
            Some(Diag::atom("ob")),
            Some(Diag::atom("unit")),
        ));
        // unit_l : unit ob -> ob
        trs_body.push(gen_instr(
            "unit_l",
            Some(seq(vec![Diag::atom("unit"), Diag::atom("ob")])),
            Some(Diag::atom("ob")),
        ));
        // unit_r : ob unit -> ob
        trs_body.push(gen_instr(
            "unit_r",
            Some(seq(vec![Diag::atom("ob"), Diag::atom("unit")])),
            Some(Diag::atom("ob")),
        ));
    }

    // Function symbols
    for f in &trs.funs {
        let name = sanitize(&f.name);
        if f.arity == 0 {
            // name : unit -> ob
            trs_body.push(gen_instr(
                &name,
                Some(Diag::atom("unit")),
                Some(Diag::atom("ob")),
            ));
        } else {
            // name : ob ob ... -> ob  (f.arity obs)
            trs_body.push(gen_instr(
                &name,
                Some(n_obs(f.arity)),
                Some(Diag::atom("ob")),
            ));
        }
    }

    // Identity 2-cells
    // for i in 1..=max(maxArity, 2)
    let max_arity = trs.funs.iter().map(|f| f.arity).max().unwrap_or(1);
    let id_count = std::cmp::max(max_arity, 2);
    for i in 1..=id_count {
        // id_i : ob^i -> ob^i
        trs_body.push(gen_instr(
            &format!("id_{}", i),
            Some(n_obs(i)),
            Some(n_obs(i)),
        ));
    }

    // Naturality of copy
    for f in &trs.funs {
        if f.arity == 0 {
            continue;
        }
        let name = sanitize(&f.name);
        // lhs = f copy (vertical: name then copy)
        let lhs_diag = Diag::atom(&name).then(Diag::atom("copy"));

        if f.arity == 1 {
            // rhs = copy (f #0 f)
            // i.e. copy then (f par f)
            let rhs_diag = Diag::atom("copy").then(
                // (f #0 f) — hseq of two f atoms
                hseq(vec![Diag::atom(&name), Diag::atom(&name)]),
            );
            trs_body.push(attach_instr(
                &format!("Copy_{}", name),
                &["Eq"],
                vec![("lhs", lhs_diag), ("rhs", rhs_diag)],
            ));
        } else if f.arity == 2 {
            // rhs = (copy #0 copy) (ob swap ob) (f #0 f)
            let copy_par_copy = hseq(vec![Diag::atom("copy"), Diag::atom("copy")]);
            let ob_swap_ob = seq(vec![
                Diag::atom("ob"),
                Diag::atom("swap"),
                Diag::atom("ob"),
            ]);
            let f_par_f = hseq(vec![Diag::atom(&name), Diag::atom(&name)]);
            let rhs_diag = copy_par_copy.then(ob_swap_ob).then(f_par_f);
            trs_body.push(attach_instr(
                &format!("Copy_{}", name),
                &["Eq"],
                vec![("lhs", lhs_diag), ("rhs", rhs_diag)],
            ));
        }
        // Higher arities: TODO (same as TypeScript)
    }

    // Structural equations
    trs_body.push(gen_instr(
        "id_1_idem",
        Some(seq(vec![Diag::atom("id_1"), Diag::atom("id_1")])),
        Some(Diag::atom("id_1")),
    ));
    trs_body.push(gen_instr(
        "id_1_idem_inv",
        Some(Diag::atom("id_1")),
        Some(seq(vec![Diag::atom("id_1"), Diag::atom("id_1")])),
    ));
    trs_body.push(gen_instr(
        "swap_inv",
        Some(seq(vec![Diag::atom("swap"), Diag::atom("swap")])),
        Some(Diag::atom("id_2")),
    ));
    trs_body.push(gen_instr(
        "swap_inv_inv",
        Some(Diag::atom("id_2")),
        Some(seq(vec![Diag::atom("swap"), Diag::atom("swap")])),
    ));
    trs_body.push(gen_instr(
        "copy_comm",
        Some(seq(vec![Diag::atom("copy"), Diag::atom("swap")])),
        Some(Diag::atom("copy")),
    ));
    trs_body.push(gen_instr(
        "copy_comm_inv",
        Some(Diag::atom("copy")),
        Some(seq(vec![Diag::atom("copy"), Diag::atom("swap")])),
    ));

    // Rewrite rules
    for (i, rule) in trs.rules.iter().enumerate() {
        let rule_num = i + 1;
        let lhs_vars = free_vars_ordered(&rule.lhs);
        let rhs_vars = free_vars_ordered(&rule.rhs);

        // Check for extra variables on RHS
        let lhs_set: std::collections::HashSet<&String> = lhs_vars.iter().collect();
        if let Some(v) = rhs_vars.iter().find(|v| !lhs_set.contains(v)) {
            eprintln!("SKIPPED rule {}: RHS has extra variable {}", rule_num, v);
            continue;
        }

        let vars = &lhs_vars;
        let is_ground = vars.is_empty();

        let lhs_result =
            encode_term_for_rule(&rule.lhs, vars, &fun_map, use_unit, is_ground);
        let rhs_result =
            encode_term_for_rule(&rule.rhs, vars, &fun_map, use_unit, is_ground);

        match (lhs_result, rhs_result) {
            (Ok(lhs_diag), Ok(rhs_diag)) => {
                eprintln!(
                    "Rule {}: {} -> {}",
                    rule_num,
                    term_to_string(&rule.lhs),
                    term_to_string(&rule.rhs)
                );
                trs_body.push(gen_instr(
                    &format!("rule_{}", rule_num),
                    Some(lhs_diag),
                    Some(rhs_diag),
                ));
            }
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("SKIPPED rule {}: {}", rule_num, e);
            }
        }
    }

    // ---- Assemble the @Type block ----
    let type_block = syn(ast::Block::TypeBlock(vec![
        type_generator("Eq", eq_body),
        type_generator(&sanitize(module_name), trs_body),
    ]));

    ast::Program {
        blocks: vec![type_block],
    }
}
