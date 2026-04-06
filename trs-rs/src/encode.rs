use std::collections::{HashMap, HashSet};

use crate::diag::{hseq, is_atom, seq, seq_flat, Diag};
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

/// Collect ALL variable occurrences in left-to-right leaf order (with repeats).
pub fn leaf_vars(t: &Term) -> Vec<String> {
    match t {
        Term::Var(name) => vec![name.clone()],
        Term::App { args, .. } => {
            let mut result = Vec::new();
            for arg in args {
                result.extend(leaf_vars(arg));
            }
            result
        }
    }
}

pub fn has_constants(funs: &[crate::types::FunDecl]) -> bool {
    funs.iter().any(|f| f.arity == 0)
}

/// Check if any rule erases a variable (appears in LHS but not RHS).
pub fn has_erasing_rules(rules: &[Rule]) -> bool {
    for rule in rules {
        let lhs_vars: HashSet<String> = free_vars_ordered(&rule.lhs).into_iter().collect();
        let rhs_vars: HashSet<String> = free_vars_ordered(&rule.rhs).into_iter().collect();
        for v in &lhs_vars {
            if !rhs_vars.contains(v) {
                return true;
            }
        }
    }
    false
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
/// Both arrays must be permutations of each other.
/// Returns a sequence of adjacent-swap positions.
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

/// Encode a swap network as an alifib Diag.
/// `swaps` is a list of positions where adjacent transpositions happen.
/// `width` is the total number of wires.
/// Returns None if no swaps needed.
pub fn encode_swap_network(swaps: &[usize], width: usize) -> Option<Diag> {
    if swaps.is_empty() {
        return None;
    }

    let mut parts: Vec<Diag> = Vec::new();
    for &s in swaps {
        // Each swap is a single layer
        let mut pieces: Vec<Diag> = Vec::new();
        for _ in 0..s {
            pieces.push(Diag::atom("ob"));
        }
        pieces.push(Diag::atom("swap"));
        for _ in (s + 2)..width {
            pieces.push(Diag::atom("ob"));
        }
        if pieces.len() == 1 {
            parts.push(pieces.remove(0));
        } else {
            // wrap in parens via seq (principal paste) — the seq result is multiple elements
            // and will be wrapped in Paren when used in then()
            // But here we need it as a single unit in the outer seq, so wrap it:
            // Actually, for the outer `parts.join(" ")` -> seq, each part is separate.
            // For the inner "(pieces.join(" "))" -> a seq that will be wrapped in paren.
            // We push the seq-wrapped version as a single Diag entry.
            // When seq(parts) is called, each element of parts is a single item via `then`.
            // The inner pieces are joined with space (principal paste) and need to be wrapped.
            // We represent a parenthesized group as a Diag that, when used in then(), becomes Paren.
            parts.push(make_paren_diag(seq(pieces)));
        }
    }

    Some(seq(parts))
}

/// Wrap a Diag in a Paren DComponent, making it a single PrincipalPaste([Paren(...)]).
fn make_paren_diag(d: Diag) -> Diag {
    use alifib::language::ast::{DComponent, DExpr, Diagram};
    use crate::diag::syn;
    Diag(Diagram::PrincipalPaste(vec![syn(DExpr::Component(
        DComponent::Paren(Box::new(syn(d.0))),
    ))]))
}

/// Encode a duplication network for a variable that needs n copies.
/// Returns None if n <= 1 (no copy needed).
pub fn encode_copy_tree(n: usize) -> Option<Diag> {
    if n <= 1 {
        return None;
    }
    if n == 2 {
        return Some(Diag::atom("copy"));
    }
    // n > 2: build iteratively
    // copy (copy ob) (copy ob ob) ...
    // diagram starts as "copy", width starts at 2
    // each iteration: diagram = diagram + " (" + pieces.join(" ") + ")"
    //   pieces = [copy, ob, ob, ...] (width - 1 obs)
    let mut diagram = Diag::atom("copy");
    let mut width = 2usize;
    for _ in 2..n {
        // pieces = [copy] + [ob] * (width - 1)
        let mut pieces: Vec<Diag> = vec![Diag::atom("copy")];
        for _ in 1..width {
            pieces.push(Diag::atom("ob"));
        }
        // pieces.join(" ") = seq(pieces) — a principal paste
        // wrap in parens for use in outer sequence
        let inner = make_paren_diag(seq(pieces));
        diagram = diagram.then(inner);
        width += 1;
    }
    Some(diagram)
}

/// Build the "gather" phase: from the input variable context to the
/// required leaf-variable order of a term.
///
/// Steps:
/// 1. For each variable used k times, duplicate it k-1 times (copy)
/// 2. For each variable used 0 times, erase it
/// 3. Reorder wires to match the leaf order
///
/// Returns (diagram or None, final_width).
pub fn encode_gather_phase(
    input_vars: &[String],
    leaf_order: &[String],
    use_unit: bool,
) -> (Option<Diag>, usize) {
    let mut use_counts: HashMap<String, usize> = HashMap::new();
    for v in input_vars {
        use_counts.insert(v.clone(), 0);
    }
    for v in leaf_order {
        *use_counts.entry(v.clone()).or_insert(0) += 1;
    }

    let mut parts: Vec<Diag> = Vec::new();

    // Phase 1: copy/erase as needed
    let mut copy_erase_pieces: Vec<Diag> = Vec::new();
    let mut after_copy_erase: Vec<String> = Vec::new();

    for v in input_vars {
        let k = *use_counts.get(v).unwrap_or(&0);
        if k == 0 {
            if use_unit {
                copy_erase_pieces.push(Diag::atom("erase"));
                after_copy_erase.push("__erased__".to_owned());
            } else {
                // Variable unused but no unit cell — skip (caller handles)
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

    let has_copy_erase = copy_erase_pieces
        .iter()
        .any(|p| !is_atom(p, "ob"));

    if has_copy_erase {
        if copy_erase_pieces.len() == 1 {
            parts.push(copy_erase_pieces.remove(0));
        } else {
            // "(" + pieces.join(" #0 ") + ")" = hseq wrapped in parens
            parts.push(make_paren_diag(hseq(copy_erase_pieces)));
        }
    }

    // Phase 2: remove erased wires using unit_l/unit_r
    let mut wip_wires = after_copy_erase.clone();
    if use_unit {
        loop {
            let idx = wip_wires.iter().position(|w| w == "__erased__");
            let Some(idx) = idx else { break };

            if idx == 0 && wip_wires.len() > 1 {
                // unit_l at position 0
                let mut pieces: Vec<Diag> = vec![Diag::atom("unit_l")];
                for _ in 2..wip_wires.len() {
                    pieces.push(Diag::atom("ob"));
                }
                if pieces.len() == 1 {
                    parts.push(pieces.remove(0));
                } else {
                    parts.push(make_paren_diag(seq(pieces)));
                }
                let next = wip_wires[1].clone();
                wip_wires.splice(0..2, [next]);
            } else if idx == wip_wires.len() - 1 && wip_wires.len() > 1 {
                // unit_r at last position
                let mut pieces: Vec<Diag> = Vec::new();
                for _ in 0..(idx - 1) {
                    pieces.push(Diag::atom("ob"));
                }
                pieces.push(Diag::atom("unit_r"));
                if pieces.len() == 1 {
                    parts.push(pieces.remove(0));
                } else {
                    parts.push(make_paren_diag(seq(pieces)));
                }
                let prev = wip_wires[idx - 1].clone();
                wip_wires.splice((idx - 1)..=idx, [prev]);
            } else if wip_wires.len() == 1 {
                break;
            } else {
                // Check ridx logic from TS
                let ridx = wip_wires
                    .iter()
                    .rposition(|w| w == "__erased__")
                    .unwrap_or(0);
                if ridx < wip_wires.len() - 1 {
                    break;
                }
                break;
            }
        }
    }

    let current_wires: Vec<String> = wip_wires
        .into_iter()
        .filter(|w| w != "__erased__")
        .collect();

    // Phase 3: permute to match leaf order
    if !current_wires.is_empty() && !leaf_order.is_empty() {
        let swaps = compute_swaps(&current_wires, leaf_order);
        if let Some(swap_diag) = encode_swap_network(&swaps, current_wires.len()) {
            parts.push(swap_diag);
        }
    }

    if parts.is_empty() {
        (None, leaf_order.len())
    } else {
        (Some(seq_flat(parts)), leaf_order.len())
    }
}

/// Inner encoding: produces a dim-2 diagram.
/// Source wires = leaf_slots(t, fun_map): one wire per leaf.
/// Target: ob
pub fn encode_term_inner(t: &Term, fun_map: &HashMap<String, usize>) -> Diag {
    match t {
        Term::Var(_) => Diag::atom("id_1"),
        Term::App { fun, args } => {
            let f_name = sanitize(fun);
            let arity = *fun_map.get(fun).unwrap_or(&0);
            if arity == 0 {
                return Diag::atom(&f_name);
            }
            let sub_encodings: Vec<Diag> = args
                .iter()
                .map(|arg| encode_term_inner(arg, fun_map))
                .collect();
            compose_horizontal_then_apply(sub_encodings, Diag::atom(&f_name))
        }
    }
}

fn compose_horizontal_then_apply(subs: Vec<Diag>, f: Diag) -> Diag {
    match subs.len() {
        0 => f,
        1 => {
            let sub = subs.into_iter().next().unwrap();
            if is_atom(&sub, "id_1") {
                f
            } else {
                sub.then(f)
            }
        }
        _ => {
            if subs.iter().all(|s| is_atom(s, "id_1")) {
                return f;
            }
            // wrap sub encodings in horizontal composition, then apply f
            // hseq(subs) then f
            // But we need to wrap in parens before applying — the `then` method
            // handles wrapping non-PrincipalPaste diagrams in Paren automatically.
            // hseq produces a Paste diagram, which when used in then() gets wrapped in Paren.
            hseq(subs).then(f)
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
///
/// Returns Ok(Diag) on success, or Err(message) to skip the rule.
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

    let var_slots: Vec<String> = slots
        .iter()
        .filter_map(|s| s.clone())
        .collect();
    let const_count = slots.iter().filter(|s| s.is_none()).count();

    // If no constants, just do the standard gather (copy/erase/swap on ob wires)
    if const_count == 0 {
        let (gather_diag, _) = encode_gather_phase(rule_vars, &var_slots, use_unit);
        return Ok(match gather_diag {
            // TypeScript: gatherDiag + " " + rawEncoding — flat join
            Some(g) => g.then_flat(raw_encoding),
            None => raw_encoding,
        });
    }

    // With constants: produce unit wires from ob wires via erase.
    let mut parts: Vec<Diag> = Vec::new();

    // Count how many copies each variable needs
    let mut var_use_counts: HashMap<String, usize> = HashMap::new();
    for v in rule_vars {
        var_use_counts.insert(v.clone(), 0);
    }
    for s in &var_slots {
        *var_use_counts.entry(s.clone()).or_insert(0) += 1;
    }

    // Add extra copies for constant slots (donated by first variable)
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
    let has_copy = copy_pieces.iter().any(|p| !is_atom(p, "id_1"));
    if has_copy {
        if copy_pieces.len() == 1 {
            parts.push(copy_pieces.remove(0));
        } else {
            parts.push(make_paren_diag(hseq(copy_pieces)));
        }
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

    // Step 2: Permute (all ob wires, swap works)
    let swaps = compute_swaps(&current_wires, &target_wires);
    if let Some(swap_diag) = encode_swap_network(&swaps, current_wires.len()) {
        parts.push(swap_diag);
    }

    // Step 3: Erase wires at constant positions
    let mut erase_pieces: Vec<Diag> = Vec::new();
    for s in &slots {
        if s.is_some() {
            erase_pieces.push(Diag::atom("id_1"));
        } else {
            erase_pieces.push(Diag::atom("erase"));
        }
    }
    let has_erase = erase_pieces.iter().any(|p| !is_atom(p, "id_1"));
    if has_erase {
        if erase_pieces.len() == 1 {
            parts.push(erase_pieces.remove(0));
        } else {
            parts.push(make_paren_diag(hseq(erase_pieces)));
        }
    }

    // Step 4: Apply raw encoding — TypeScript: parts.join(" ") + " " + rawEncoding (flat join)
    if parts.is_empty() {
        Ok(raw_encoding)
    } else {
        // Flat-join parts and raw_encoding into one PrincipalPaste sequence
        parts.push(raw_encoding);
        Ok(seq_flat(parts))
    }
}
