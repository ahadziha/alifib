use std::collections::{HashMap, HashSet};

use alifib::codegen::{Diag, compose_or_single, par_seq, seq, seq_flat};
use crate::types::{Rule, Term, TRS};

/// Replace special characters with alphanumeric names.
pub fn sanitize(name: &str) -> String {
    let mut result = String::new();
    for c in name.chars() {
        match c {
            '+' => result.push_str("plus"),
            '-' => result.push_str("minus"),
            '*' => result.push_str("times"),
            '/' => result.push_str("div"),
            '\\' => result.push_str("bslash"),
            '<' => result.push_str("lt"),
            '>' => result.push_str("gt"),
            '=' => result.push_str("eq"),
            '!' => result.push_str("bang"),
            '?' => result.push_str("qmark"),
            '.' => result.push_str("dot"),
            ',' => result.push_str("comma"),
            '\'' => result.push_str("prime"),
            c if c.is_alphanumeric() || c == '_' => result.push(c),
            _ => result.push('_'),
        }
    }
    if result.starts_with(|c: char| c.is_numeric()) {
        result = format!("c_{}", result);
    }
    result
}

/// Collect free variables in left-to-right order of first occurrence.
pub fn free_vars_ordered(t: &Term) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    fn walk(t: &Term, seen: &mut HashSet<String>, result: &mut Vec<String>) {
        match t {
            Term::Var(name) => {
                if seen.insert(name.clone()) {
                    result.push(name.clone());
                }
            }
            Term::App { args, .. } => {
                for arg in args {
                    walk(arg, seen, result);
                }
            }
        }
    }
    walk(t, &mut seen, &mut result);
    result
}

pub fn has_constants(funs: &[crate::types::FunDecl]) -> bool {
    funs.iter().any(|f| f.arity == 0)
}

/// Check if any rule erases a variable (appears in LHS but not RHS).
pub fn has_erasing_rules(rules: &[Rule]) -> bool {
    rules.iter().any(|rule| {
        let rhs_vars = free_vars_ordered(&rule.rhs);
        let rhs_set: HashSet<&str> = rhs_vars.iter().map(String::as_str).collect();
        free_vars_ordered(&rule.lhs)
            .iter()
            .any(|v| !rhs_set.contains(v.as_str()))
    })
}

/// Determine if unit/erase cells are needed (constants or erasing rules).
pub fn needs_unit(trs: &TRS) -> bool {
    has_constants(&trs.funs) || has_erasing_rules(&trs.rules)
}

/// Collect leaf slots of a term: variables as their name, constants as None.
pub fn leaf_slots(t: &Term, fun_map: &HashMap<String, usize>) -> Vec<Option<String>> {
    match t {
        Term::Var(name) => vec![Some(name.clone())],
        Term::App { fun, args } => {
            let arity = *fun_map.get(fun).unwrap_or(&0);
            if arity == 0 {
                vec![None]
            } else {
                let mut result = Vec::new();
                for arg in args {
                    result.extend(leaf_slots(arg, fun_map));
                }
                result
            }
        }
    }
}

/// Build a swap network to permute `from` into `to`.
pub fn compute_swaps(from: &[String], to: &[String]) -> Vec<usize> {
    let mut arr: Vec<String> = from.to_vec();
    let mut swaps = Vec::new();
    for i in 0..to.len() {
        let mut j = i;
        while j < arr.len() && arr[j] != to[i] {
            j += 1;
        }
        while j > i {
            arr.swap(j - 1, j);
            swaps.push(j - 1);
            j -= 1;
        }
    }
    swaps
}

/// Encode a swap network as a Diag. Returns None if no swaps needed.
pub fn encode_swap_network(swaps: &[usize], width: usize) -> Option<Diag> {
    if swaps.is_empty() {
        return None;
    }
    let layers: Vec<Diag> = swaps
        .iter()
        .map(|&s| {
            let pieces: Vec<Diag> = (0..s)
                .map(|_| Diag::atom("ob"))
                .chain(std::iter::once(Diag::atom("swap")))
                .chain((s + 2..width).map(|_| Diag::atom("ob")))
                .collect();
            compose_or_single(pieces, seq)
        })
        .collect();
    Some(seq(layers))
}

/// Encode a duplication network for a variable that needs n copies.
/// Returns None if n <= 1.
pub fn encode_copy_tree(n: usize) -> Option<Diag> {
    if n <= 1 {
        return None;
    }
    if n == 2 {
        return Some(Diag::atom("copy"));
    }
    let mut diagram = Diag::atom("copy");
    let mut width = 2usize;
    for _ in 2..n {
        let pieces: Vec<Diag> = std::iter::once(Diag::atom("copy"))
            .chain((1..width).map(|_| Diag::atom("ob")))
            .collect();
        diagram = diagram.then(compose_or_single(pieces, seq));
        width += 1;
    }
    Some(diagram)
}

/// Build the gather phase: copy/erase/swap to route input vars to leaf order.
pub fn encode_gather_phase(
    input_vars: &[String],
    leaf_order: &[String],
    use_unit: bool,
) -> (Option<Diag>, usize) {
    let mut use_counts: HashMap<String, usize> =
        input_vars.iter().map(|v| (v.clone(), 0)).collect();
    for v in leaf_order {
        *use_counts.entry(v.clone()).or_insert(0) += 1;
    }

    let mut parts: Vec<Diag> = Vec::new();

    // Phase 1: copy/erase
    let mut copy_erase_pieces: Vec<Diag> = Vec::new();
    let mut after_copy_erase: Vec<String> = Vec::new();

    for v in input_vars {
        let k = *use_counts.get(v).unwrap_or(&0);
        if k == 0 {
            if use_unit {
                copy_erase_pieces.push(Diag::atom("erase"));
                after_copy_erase.push("__erased__".to_owned());
            } else {
                copy_erase_pieces.push(Diag::atom("ob"));
                after_copy_erase.push(v.clone());
            }
        } else if k == 1 {
            copy_erase_pieces.push(Diag::atom("ob"));
            after_copy_erase.push(v.clone());
        } else {
            let ct = encode_copy_tree(k).unwrap_or_else(|| Diag::atom("ob"));
            copy_erase_pieces.push(ct);
            for _ in 0..k {
                after_copy_erase.push(v.clone());
            }
        }
    }

    if copy_erase_pieces.iter().any(|p| !p.is_atom("ob")) {
        parts.push(compose_or_single(copy_erase_pieces, par_seq));
    }

    // Phase 2: eliminate erased wires via unit_l / unit_r
    let mut wip_wires = after_copy_erase;
    if use_unit {
        loop {
            let Some(idx) = wip_wires.iter().position(|w| w == "__erased__") else {
                break;
            };
            if idx == 0 && wip_wires.len() > 1 {
                let pieces: Vec<Diag> = std::iter::once(Diag::atom("unit_l"))
                    .chain((2..wip_wires.len()).map(|_| Diag::atom("ob")))
                    .collect();
                parts.push(compose_or_single(pieces, seq));
                let next = wip_wires[1].clone();
                wip_wires.splice(0..2, [next]);
            } else if idx == wip_wires.len() - 1 && wip_wires.len() > 1 {
                let pieces: Vec<Diag> = (0..idx - 1)
                    .map(|_| Diag::atom("ob"))
                    .chain(std::iter::once(Diag::atom("unit_r")))
                    .collect();
                parts.push(compose_or_single(pieces, seq));
                let prev = wip_wires[idx - 1].clone();
                wip_wires.splice((idx - 1)..=idx, [prev]);
            } else {
                break;
            }
        }
    }

    let current_wires: Vec<String> =
        wip_wires.into_iter().filter(|w| w != "__erased__").collect();

    // Phase 3: permute to match leaf order
    if !current_wires.is_empty() && !leaf_order.is_empty() {
        if let Some(swap_diag) =
            encode_swap_network(&compute_swaps(&current_wires, leaf_order), current_wires.len())
        {
            parts.push(swap_diag);
        }
    }

    if parts.is_empty() {
        (None, leaf_order.len())
    } else {
        (Some(seq_flat(parts)), leaf_order.len())
    }
}

/// Inner encoding: produces the dim-2 diagram for a term.
pub fn encode_term_inner(t: &Term, fun_map: &HashMap<String, usize>) -> Diag {
    match t {
        Term::Var(_) => Diag::atom("id_1"),
        Term::App { fun, args } => {
            let f_name = sanitize(fun);
            let arity = *fun_map.get(fun).unwrap_or(&0);
            if arity == 0 {
                return Diag::atom(&f_name);
            }
            let subs: Vec<Diag> =
                args.iter().map(|arg| encode_term_inner(arg, fun_map)).collect();
            compose_subs_then_apply(subs, Diag::atom(&f_name))
        }
    }
}

fn compose_subs_then_apply(subs: Vec<Diag>, f: Diag) -> Diag {
    match subs.len() {
        0 => f,
        1 => {
            let sub = subs.into_iter().next().unwrap();
            if sub.is_atom("id_1") { f } else { sub.then(f) }
        }
        _ => {
            if subs.iter().all(|s| s.is_atom("id_1")) {
                return f;
            }
            par_seq(subs).then(f)
        }
    }
}

pub fn term_to_string(t: &Term) -> String {
    match t {
        Term::Var(name) => name.clone(),
        Term::App { fun, args } => {
            if args.is_empty() {
                fun.clone()
            } else {
                let arg_strs: Vec<String> = args.iter().map(term_to_string).collect();
                format!("{}({})", fun, arg_strs.join(", "))
            }
        }
    }
}

/// Encode a term for use in a rule, given the rule's variable context.
pub fn encode_term_for_rule(
    t: &Term,
    rule_vars: &[String],
    fun_map: &HashMap<String, usize>,
    use_unit: bool,
    is_ground: bool,
) -> Result<Diag, String> {
    let slots = leaf_slots(t, fun_map);
    let raw_encoding = encode_term_inner(t, fun_map);

    if is_ground {
        return Ok(raw_encoding);
    }

    let var_slots: Vec<String> = slots.iter().flatten().cloned().collect();
    let const_count = slots.iter().filter(|s| s.is_none()).count();

    if const_count == 0 {
        let (gather_diag, _) = encode_gather_phase(rule_vars, &var_slots, use_unit);
        return Ok(match gather_diag {
            Some(g) => seq_flat([g, raw_encoding]),
            None => raw_encoding,
        });
    }

    let mut parts: Vec<Diag> = Vec::new();

    let mut var_use_counts: HashMap<String, usize> =
        rule_vars.iter().map(|v| (v.clone(), 0)).collect();
    for s in &var_slots {
        *var_use_counts.entry(s.clone()).or_insert(0) += 1;
    }

    let donor_var = rule_vars[0].clone();
    *var_use_counts.entry(donor_var.clone()).or_insert(0) += const_count;

    // Step 1: Copy phase
    let mut copy_pieces: Vec<Diag> = Vec::new();
    let mut after_copy: Vec<String> = Vec::new();
    for v in rule_vars {
        let k = *var_use_counts.get(v).unwrap_or(&0);
        if k == 0 {
            copy_pieces.push(Diag::atom("id_1"));
            after_copy.push(format!("__unused_{}", v));
        } else if k == 1 {
            copy_pieces.push(Diag::atom("id_1"));
            after_copy.push(v.clone());
        } else {
            let ct = encode_copy_tree(k).unwrap_or_else(|| Diag::atom("id_1"));
            copy_pieces.push(ct);
            for _ in 0..k {
                after_copy.push(v.clone());
            }
        }
    }
    if copy_pieces.iter().any(|p| !p.is_atom("id_1")) {
        parts.push(compose_or_single(copy_pieces, par_seq));
    }

    // Build target wire assignment
    let mut target_wires: Vec<String> = Vec::new();
    let mut donor_for_var_count = 0usize;
    let mut donor_for_const_count = 0usize;
    for s in &slots {
        if let Some(var_name) = s {
            if var_name == &donor_var {
                target_wires.push(format!("{}_var_{}", var_name, donor_for_var_count));
                donor_for_var_count += 1;
            } else {
                let idx = target_wires
                    .iter()
                    .filter(|w| w.starts_with(&format!("{}_", var_name)))
                    .count();
                target_wires.push(format!("{}_var_{}", var_name, idx));
            }
        } else {
            target_wires.push(format!("{}_const_{}", donor_var, donor_for_const_count));
            donor_for_const_count += 1;
        }
    }

    // Build current wire labels
    let mut current_wires: Vec<String> = Vec::new();
    let mut var_counters: HashMap<String, usize> = HashMap::new();
    let mut const_counters: HashMap<String, usize> = HashMap::new();
    for v in &after_copy {
        if v.starts_with("__unused_") {
            current_wires.push(v.clone());
            continue;
        }
        let vc = *var_counters.get(v).unwrap_or(&0);
        let total_var_uses = var_slots.iter().filter(|s| s.as_str() == v).count();
        if vc < total_var_uses {
            current_wires.push(format!("{}_var_{}", v, vc));
            var_counters.insert(v.clone(), vc + 1);
        } else {
            let cc = *const_counters.get(v).unwrap_or(&0);
            current_wires.push(format!("{}_const_{}", v, cc));
            const_counters.insert(v.clone(), cc + 1);
        }
    }

    // Step 2: Permute
    if let Some(swap_diag) =
        encode_swap_network(&compute_swaps(&current_wires, &target_wires), current_wires.len())
    {
        parts.push(swap_diag);
    }

    // Step 3: Erase constant positions
    let erase_pieces: Vec<Diag> = slots
        .iter()
        .map(|s| if s.is_some() { Diag::atom("id_1") } else { Diag::atom("erase") })
        .collect();
    if erase_pieces.iter().any(|p| !p.is_atom("id_1")) {
        parts.push(compose_or_single(erase_pieces, par_seq));
    }

    // Step 4: Apply raw encoding
    if parts.is_empty() {
        Ok(raw_encoding)
    } else {
        parts.push(raw_encoding);
        Ok(seq_flat(parts))
    }
}
