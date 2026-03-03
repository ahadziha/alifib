/// Pretty-printer for the AST (mirrors OCaml's ast_pp.ml)
use super::ast::*;
use crate::core::diagram::Sign as DiagramSign;

pub fn program_to_string(p: &Program) -> String {
    let mut out = String::new();
    out.push_str("(Program\n  blocks = [");
    for (i, b) in p.value.blocks.iter().enumerate() {
        if i > 0 { out.push_str("; "); }
        out.push('\n');
        out.push_str(&block_str(b, 4));
    }
    out.push_str("])\n");
    out
}

fn indent(n: usize) -> String {
    " ".repeat(n)
}

fn block_str(b: &Block, ind: usize) -> String {
    let i = indent(ind);
    match &b.value {
        BlockDesc::Type { body } => {
            let body_str = match body {
                None => "None".into(),
                Some(cb) => c_block_type_str(cb, ind + 2),
            };
            format!("{}(Block_type\n{}  body = {})", i, i, body_str)
        }
        BlockDesc::Complex { complex, local } => {
            let local_str = match local {
                None => "None".into(),
                Some(cl) => c_block_local_str(cl, ind + 2),
            };
            format!("{}(Block_complex\n{}  complex = {}\n{}  locals = {})",
                i, i, complex_str(complex, ind + 2), i, local_str)
        }
    }
}

fn complex_str(c: &Complex, ind: usize) -> String {
    let addr = c.value.address.as_ref().map(address_str).unwrap_or_else(|| "None".into());
    let block = c.value.block.as_ref().map(|b| c_block_str(b, ind + 2)).unwrap_or_else(|| "None".into());
    format!("(Complex address = {} block = {})", addr, block)
}

fn address_str(a: &Address) -> String {
    a.value.iter().map(|n| n.value.as_str()).collect::<Vec<_>>().join(".")
}

fn c_block_type_str(b: &CBlockType, ind: usize) -> String {
    let items = b.value.iter().map(|i| c_instr_type_str(i, ind)).collect::<Vec<_>>().join("; ");
    format!("[{}]", items)
}

fn c_block_str(b: &CBlock, ind: usize) -> String {
    let items = b.value.iter().map(|i| c_instr_str(i, ind)).collect::<Vec<_>>().join("; ");
    format!("[{}]", items)
}

fn c_block_local_str(b: &CBlockLocal, ind: usize) -> String {
    let items = b.value.iter().map(|i| c_instr_local_str(i, ind)).collect::<Vec<_>>().join("; ");
    format!("[{}]", items)
}

fn c_instr_type_str(i: &CInstrType, ind: usize) -> String {
    match &i.value {
        CInstrTypeDesc::Generator(gt) => format!("(Generator_type {})", generator_type_str(gt, ind)),
        CInstrTypeDesc::Dnamer(d)     => format!("(Diagram_namer {})", dnamer_str(d, ind)),
        CInstrTypeDesc::Mnamer(m)     => format!("(Morphism_namer {})", mnamer_str(m, ind)),
        CInstrTypeDesc::IncludeModule(im) => format!("(Include_module {})", include_module_str(im)),
    }
}

fn c_instr_str(i: &CInstr, ind: usize) -> String {
    match &i.value {
        CInstrDesc::Generator(g)  => format!("(Generator {})", generator_str(g)),
        CInstrDesc::Dnamer(d)     => format!("(Diagram_namer {})", dnamer_str(d, ind)),
        CInstrDesc::Mnamer(m)     => format!("(Morphism_namer {})", mnamer_str(m, ind)),
        CInstrDesc::Include(inc)  => format!("(Include {})", include_stmt_str(inc)),
        CInstrDesc::Attach(att)   => format!("(Attach {})", attach_stmt_str(att, ind)),
    }
}

fn c_instr_local_str(i: &CInstrLocal, ind: usize) -> String {
    match &i.value {
        CInstrLocalDesc::Dnamer(d)  => format!("(Diagram_namer {})", dnamer_str(d, ind)),
        CInstrLocalDesc::Mnamer(m)  => format!("(Morphism_namer {})", mnamer_str(m, ind)),
        CInstrLocalDesc::Assert(a)  => format!("(Assert {})", assert_str(a, ind)),
    }
}

fn generator_type_str(gt: &GeneratorType, ind: usize) -> String {
    format!("(Generator_type generator = {} definition = {})",
        generator_str(&gt.value.generator),
        complex_str(&gt.value.definition, ind))
}

fn generator_str(g: &Generator) -> String {
    let bounds = g.value.boundaries.as_ref()
        .map(|b| format!(" : {}", boundaries_str(b)))
        .unwrap_or_default();
    format!("(Generator name = {}{})", g.value.name.value, bounds)
}

fn boundaries_str(b: &Boundaries) -> String {
    format!("(Boundaries source = {} target = {})",
        diagram_str(&b.value.source),
        diagram_str(&b.value.target))
}

fn dnamer_str(d: &Dnamer, _ind: usize) -> String {
    let bounds = d.value.boundaries.as_ref()
        .map(|b| format!(" : {}", boundaries_str(b)))
        .unwrap_or_default();
    format!("(DNamer name = {}{} body = {})",
        d.value.name.value, bounds, diagram_str(&d.value.body))
}

fn mnamer_str(m: &Mnamer, ind: usize) -> String {
    format!("(MNamer name = {} address = {} definition = {})",
        m.value.name.value,
        address_str(&m.value.address),
        m_def_str(&m.value.definition, ind))
}

fn include_module_str(im: &IncludeModule) -> String {
    let alias = im.value.alias.as_ref().map(|n| format!(" as {}", n.value)).unwrap_or_default();
    format!("(Include_module name = {}{})", im.value.name.value, alias)
}

fn include_stmt_str(inc: &IncludeStatement) -> String {
    let alias = inc.value.alias.as_ref().map(|n| format!(" as {}", n.value)).unwrap_or_default();
    format!("(Include address = {}{})", address_str(&inc.value.address), alias)
}

fn attach_stmt_str(att: &AttachStatement, ind: usize) -> String {
    let along = att.value.along.as_ref()
        .map(|m| format!(" along {}", m_def_str(m, ind)))
        .unwrap_or_default();
    format!("(Attach name = {} address = {}{})",
        att.value.name.value, address_str(&att.value.address), along)
}

fn assert_str(a: &AssertStatement, _ind: usize) -> String {
    format!("(Assert left = {} right = {})",
        pasting_str(&a.value.left), pasting_str(&a.value.right))
}

fn m_def_str(m: &MDef, ind: usize) -> String {
    match &m.value {
        MDefDesc::Morphism(morph) => format!("(MDef_morphism {})", morphism_str(morph)),
        MDefDesc::Ext(ext)        => format!("(MDef_ext {})", m_ext_str(ext, ind)),
    }
}

fn morphism_str(m: &Morphism) -> String {
    match &m.value {
        MorphismDesc::Single(mc)  => format!("(Morphism_single {})", m_comp_str(mc)),
        MorphismDesc::Concat { left, right } =>
            format!("(Morphism_concat left = {} right = {})", morphism_str(left), m_comp_str(right)),
    }
}

fn m_comp_str(mc: &MComp) -> String {
    match &mc.value {
        MCompDesc::Term(t) => format!("(MTerm {})", m_term_str(t)),
        MCompDesc::Name(n) => format!("(MName {})", n.value),
    }
}

fn m_term_str(t: &MTerm) -> String {
    format!("(MTerm ext = {} target = {})", m_ext_str(&t.value.ext, 0), complex_str(&t.value.target, 0))
}

fn m_ext_str(e: &MExt, _ind: usize) -> String {
    let prefix = e.value.prefix.as_ref().map(|m| format!(" prefix = {}", morphism_str(m))).unwrap_or_default();
    let block: String = e.value.block.as_ref().map(|_| " block = [...]".to_string()).unwrap_or_default();
    format!("(MExt{}{})", prefix, block)
}

fn diagram_str(d: &Diagram) -> String {
    match &d.value {
        DiagramDesc::Single(c) => format!("(Diagram_single {})", d_concat_str(c)),
        DiagramDesc::Paste { left, nat, right } =>
            format!("(Diagram_paste left = {} nat = {} right = {})",
                diagram_str(left), nat.value, d_concat_str(right)),
    }
}

fn d_concat_str(c: &DConcat) -> String {
    match &c.value {
        DConcatDesc::Single(e)          => format!("(DConcat_single {})", d_expr_str(e)),
        DConcatDesc::Concat { left, right } =>
            format!("(DConcat_concat left = {} right = {})", d_concat_str(left), d_expr_str(right)),
    }
}

fn d_expr_str(e: &DExpr) -> String {
    match &e.value {
        DExprDesc::Single(c)         => format!("(DExpr_single {})", d_comp_str(c)),
        DExprDesc::Dot { left, right } =>
            format!("(DExpr_dot left = {} right = {})", d_expr_str(left), d_comp_str(right)),
    }
}

fn d_comp_str(c: &DComp) -> String {
    match &c.value {
        DCompDesc::Mterm(t)  => format!("(DComp_mterm {})", m_term_str(t)),
        DCompDesc::Dterm(t)  => format!("(DComp_dterm {})", d_term_str(t)),
        DCompDesc::Name(n)   => format!("(DComp_name {})", n.value),
        DCompDesc::Bd(b)     => format!("(DComp_bd {})", bd_str(b)),
        DCompDesc::Hole      => "DComp_hole".into(),
    }
}

fn d_term_str(t: &DTerm) -> String {
    match &t.value {
        DTermDesc::Indexed { diagram, nat, tail } =>
            format!("(DTerm_indexed diagram = {} nat = {} tail = {})",
                diagram_str(diagram), nat.value, d_concat_str(tail)),
        DTermDesc::Pair { concat, expr } =>
            format!("(DTerm_pair concat = {} expr = {})", d_concat_str(concat), d_expr_str(expr)),
    }
}

fn pasting_str(p: &Pasting) -> String {
    match &p.value {
        PastingDesc::Single(c) => format!("(Pasting_single {})", concat_str(c)),
        PastingDesc::Paste { left, nat, right } =>
            format!("(Pasting_paste left = {} nat = {} right = {})",
                pasting_str(left), nat.value, concat_str(right)),
    }
}

fn concat_str(c: &Concat) -> String {
    match &c.value {
        ConcatDesc::Single(e)          => format!("(Concat_single {})", d_expr_str(e)),
        ConcatDesc::Concat { left, right } =>
            format!("(Concat_concat left = {} right = {})", concat_str(left), d_expr_str(right)),
    }
}

fn bd_str(b: &Bd) -> String {
    match b.value {
        DiagramSign::Input  => "in".into(),
        DiagramSign::Output => "out".into(),
    }
}
