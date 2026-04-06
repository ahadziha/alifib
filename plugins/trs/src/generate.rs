use std::collections::HashMap;

use alifib::codegen::{Diag, Program, TypeDef, obs, par_seq, seq};

use crate::encode::{
    encode_term_for_rule, free_vars_ordered, needs_unit, sanitize, term_to_string,
};
use crate::types::TRS;

pub fn generate_program(trs: &TRS, module_name: &str) -> Program {
    let use_unit = needs_unit(trs);
    let fun_map: HashMap<String, usize> =
        trs.funs.iter().map(|f| (f.name.clone(), f.arity)).collect();

    // ---- Eq type ----
    let eq = TypeDef::new("Eq")
        .cell("pt")
        .cell_bd("dom", Diag::cell("pt"), Diag::cell("pt"))
        .cell_bd("cod", Diag::cell("pt"), Diag::cell("pt"))
        .cell_bd("lhs", Diag::cell("dom"), Diag::cell("cod"))
        .cell_bd("rhs", Diag::cell("dom"), Diag::cell("cod"))
        .cell_bd("dir", Diag::cell("lhs"), Diag::cell("rhs"))
        .cell_bd("inv", Diag::cell("rhs"), Diag::cell("lhs"));

    // ---- TRS type ----
    let mut trs_type = TypeDef::new(module_name)
        .cell("pt")
        .cell_bd("ob", Diag::cell("pt"), Diag::cell("pt"));

    if use_unit {
        trs_type = trs_type.cell_bd("unit", Diag::cell("pt"), Diag::cell("pt"));
    }

    trs_type = trs_type
        .cell_bd("copy", Diag::cell("ob"), obs(2))
        .cell_bd("swap", obs(2), obs(2));

    if use_unit {
        trs_type = trs_type
            .cell_bd("erase", Diag::cell("ob"), Diag::cell("unit"))
            .cell_bd(
                "unit_l",
                seq([Diag::cell("unit"), Diag::cell("ob")]),
                Diag::cell("ob"),
            )
            .cell_bd(
                "unit_r",
                seq([Diag::cell("ob"), Diag::cell("unit")]),
                Diag::cell("ob"),
            );
    }

    // Function symbols
    for f in &trs.funs {
        let name = sanitize(&f.name);
        if f.arity == 0 {
            trs_type = trs_type.cell_bd(&name, Diag::cell("unit"), Diag::cell("ob"));
        } else {
            trs_type = trs_type.cell_bd(&name, obs(f.arity), Diag::cell("ob"));
        }
    }

    // Identity 2-cells
    let id_count = std::cmp::max(trs.funs.iter().map(|f| f.arity).max().unwrap_or(1), 2);
    for i in 1..=id_count {
        trs_type = trs_type.cell_bd(&format!("id_{}", i), obs(i), obs(i));
    }

    // Naturality of copy
    for f in &trs.funs {
        if f.arity == 0 {
            continue;
        }
        let name = sanitize(&f.name);
        let lhs_diag = Diag::cell(&name).then(Diag::cell("copy"));

        if f.arity == 1 {
            let rhs_diag = Diag::cell("copy")
                .then(par_seq([Diag::cell(&name), Diag::cell(&name)]));
            trs_type = trs_type.attach(
                &format!("Copy_{}", name),
                &["Eq"],
                vec![("lhs", lhs_diag), ("rhs", rhs_diag)],
            );
        } else if f.arity == 2 {
            let rhs_diag = par_seq([Diag::cell("copy"), Diag::cell("copy")])
                .then(seq([Diag::cell("ob"), Diag::cell("swap"), Diag::cell("ob")]))
                .then(par_seq([Diag::cell(&name), Diag::cell(&name)]));
            trs_type = trs_type.attach(
                &format!("Copy_{}", name),
                &["Eq"],
                vec![("lhs", lhs_diag), ("rhs", rhs_diag)],
            );
        }
        // Higher arities: TODO
    }

    // Structural equations
    trs_type = trs_type
        .cell_bd(
            "id_1_idem",
            seq([Diag::cell("id_1"), Diag::cell("id_1")]),
            Diag::cell("id_1"),
        )
        .cell_bd(
            "id_1_idem_inv",
            Diag::cell("id_1"),
            seq([Diag::cell("id_1"), Diag::cell("id_1")]),
        )
        .cell_bd(
            "swap_inv",
            seq([Diag::cell("swap"), Diag::cell("swap")]),
            Diag::cell("id_2"),
        )
        .cell_bd(
            "swap_inv_inv",
            Diag::cell("id_2"),
            seq([Diag::cell("swap"), Diag::cell("swap")]),
        )
        .cell_bd(
            "copy_comm",
            seq([Diag::cell("copy"), Diag::cell("swap")]),
            Diag::cell("copy"),
        )
        .cell_bd(
            "copy_comm_inv",
            Diag::cell("copy"),
            seq([Diag::cell("copy"), Diag::cell("swap")]),
        );

    // Rewrite rules
    for (i, rule) in trs.rules.iter().enumerate() {
        let rule_num = i + 1;
        let lhs_vars = free_vars_ordered(&rule.lhs);
        let rhs_vars = free_vars_ordered(&rule.rhs);

        let lhs_set: std::collections::HashSet<&String> = lhs_vars.iter().collect();
        if let Some(v) = rhs_vars.iter().find(|v| !lhs_set.contains(v)) {
            eprintln!("SKIPPED rule {}: RHS has extra variable {}", rule_num, v);
            continue;
        }

        let vars = &lhs_vars;
        let is_ground = vars.is_empty();

        match (
            encode_term_for_rule(&rule.lhs, vars, &fun_map, use_unit, is_ground),
            encode_term_for_rule(&rule.rhs, vars, &fun_map, use_unit, is_ground),
        ) {
            (Ok(lhs_diag), Ok(rhs_diag)) => {
                eprintln!(
                    "Rule {}: {} -> {}",
                    rule_num,
                    term_to_string(&rule.lhs),
                    term_to_string(&rule.rhs)
                );
                trs_type =
                    trs_type.cell_bd(&format!("rule_{}", rule_num), lhs_diag, rhs_diag);
            }
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("SKIPPED rule {}: {}", rule_num, e);
            }
        }
    }

    Program::new().type_def(eq).type_def(trs_type)
}
