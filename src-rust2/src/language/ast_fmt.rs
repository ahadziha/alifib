use std::fmt;
use super::ast::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn pad(f: &mut fmt::Formatter, n: usize) -> fmt::Result {
    for _ in 0..n {
        f.write_str("  ")?;
    }
    Ok(())
}

struct FmtAddress<'a>(&'a Address);

impl fmt::Display for FmtAddress<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (i, part) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            f.write_str(&part.inner)?;
        }
        Ok(())
    }
}

// ─── Compact Display: value/leaf types ──────────────────────────────────────

impl fmt::Display for DComponent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Name(n) => f.write_str(n),
            Self::In => f.write_str("in"),
            Self::Out => f.write_str("out"),
            Self::Paren(d) => write!(f, "({})", d.inner),
            Self::Hole => f.write_str("?"),
        }
    }
}

impl fmt::Display for DExpr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Component(c) => write!(f, "{c}"),
            Self::Dot { base, field } => write!(f, "{}.{}", base.inner, field.inner),
        }
    }
}

impl fmt::Display for Diagram {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Principal(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" ")?;
                    }
                    write!(f, "{}", e.inner)?;
                }
                Ok(())
            }
            Self::Paste { lhs, dim, rhs } => {
                write!(f, "{} #{}", lhs.inner, dim.inner)?;
                for e in rhs {
                    write!(f, " {}", e.inner)?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for Boundary {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} -> {}", self.source.inner, self.target.inner)
    }
}

impl fmt::Display for NameWithBoundary {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.name.inner)?;
        if let Some(b) = &self.boundary {
            write!(f, " : {}", b.inner)?;
        }
        Ok(())
    }
}

impl fmt::Display for PMapClause {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} => {}", self.lhs.inner, self.rhs.inner)
    }
}

impl fmt::Display for PMSystem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ext) = &self.extend {
            write!(f, "{}", ext.inner)?;
        }
        f.write_str("[")?;
        for (i, c) in self.clauses.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{}", c.inner)?;
        }
        f.write_str("]")
    }
}

impl fmt::Display for PMapBasic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Name(n) => f.write_str(n),
            Self::System(s) => write!(f, "{s}"),
        }
    }
}

impl fmt::Display for PMap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Basic(b) => write!(f, "{b}"),
            Self::Dot { base, rest } => write!(f, "{base}.{}", rest.inner),
        }
    }
}

// ─── Tree Display: structural types ─────────────────────────────────────────

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Program")?;
        for block in &self.blocks {
            pp_block(f, &block.inner, 1)?;
        }
        Ok(())
    }
}

fn pp_block(f: &mut fmt::Formatter, block: &Block, d: usize) -> fmt::Result {
    match block {
        Block::TypeBlock(insts) => {
            pad(f, d)?;
            writeln!(f, "TypeBlock")?;
            for inst in insts {
                pp_type_inst(f, &inst.inner, d + 1)?;
            }
        }
        Block::LocalBlock { complex, body } => {
            pad(f, d)?;
            write!(f, "LocalBlock ")?;
            pp_complex_header(f, &complex.inner)?;
            writeln!(f)?;
            for inst in body {
                pp_local_inst(f, &inst.inner, d + 1)?;
            }
        }
    }
    Ok(())
}

fn pp_type_inst(f: &mut fmt::Formatter, inst: &TypeInst, d: usize) -> fmt::Result {
    match inst {
        TypeInst::Generator(g) => {
            pad(f, d)?;
            writeln!(f, "Generator {}", g.name.inner)?;
            pp_complex_tree(f, &g.complex.inner, d + 1)
        }
        TypeInst::LetDiag(l) => pp_let_diag(f, l, d),
        TypeInst::DefPMap(p) => pp_def_pmap(f, p, d),
        TypeInst::IncludeModule(im) => {
            pad(f, d)?;
            write!(f, "IncludeModule {}", im.name.inner)?;
            if let Some(a) = &im.alias {
                write!(f, " as {}", a.inner)?;
            }
            writeln!(f)
        }
    }
}

fn pp_cinstr(f: &mut fmt::Formatter, instr: &CInstr, d: usize) -> fmt::Result {
    match instr {
        CInstr::NameWithBoundary(nwb) => {
            pad(f, d)?;
            writeln!(f, "{nwb}")
        }
        CInstr::LetDiag(l) => pp_let_diag(f, l, d),
        CInstr::DefPMap(p) => pp_def_pmap(f, p, d),
        CInstr::AttachStmt(a) => {
            pad(f, d)?;
            write!(f, "attach {} :: {}", a.name.inner, FmtAddress(&a.address.inner))?;
            if let Some(along) = &a.along {
                write!(f, " along {}", along.inner)?;
            }
            writeln!(f)
        }
        CInstr::IncludeStmt(inc) => {
            pad(f, d)?;
            write!(f, "include {}", FmtAddress(&inc.address.inner))?;
            if let Some(a) = &inc.alias {
                write!(f, " as {}", a.inner)?;
            }
            writeln!(f)
        }
    }
}

fn pp_local_inst(f: &mut fmt::Formatter, inst: &LocalInst, d: usize) -> fmt::Result {
    match inst {
        LocalInst::LetDiag(l) => pp_let_diag(f, l, d),
        LocalInst::DefPMap(p) => pp_def_pmap(f, p, d),
        LocalInst::AssertStmt(a) => {
            pad(f, d)?;
            writeln!(f, "assert {} = {}", a.lhs.inner, a.rhs.inner)
        }
    }
}

fn pp_let_diag(f: &mut fmt::Formatter, l: &LetDiag, d: usize) -> fmt::Result {
    pad(f, d)?;
    write!(f, "let {}", l.name.inner)?;
    if let Some(b) = &l.boundary {
        write!(f, " : {}", b.inner)?;
    }
    writeln!(f, " = {}", l.value.inner)
}

fn pp_def_pmap(f: &mut fmt::Formatter, p: &DefPMap, d: usize) -> fmt::Result {
    pad(f, d)?;
    writeln!(
        f,
        "def {} :: {} = {}",
        p.name.inner,
        FmtAddress(&p.address.inner),
        p.value.inner
    )
}

fn pp_complex_header(f: &mut fmt::Formatter, c: &Complex) -> fmt::Result {
    match c {
        Complex::Address(addr) => write!(f, "{}", FmtAddress(addr)),
        Complex::Block { address, body } => {
            if let Some(addr) = address {
                write!(f, "{} ", FmtAddress(addr))?;
            }
            write!(f, "{{...{} items}}", body.len())
        }
    }
}

fn pp_complex_tree(f: &mut fmt::Formatter, c: &Complex, d: usize) -> fmt::Result {
    match c {
        Complex::Address(addr) => {
            pad(f, d)?;
            writeln!(f, "{}", FmtAddress(addr))
        }
        Complex::Block { address, body } => {
            pad(f, d)?;
            if let Some(addr) = address {
                writeln!(f, "{} {{", FmtAddress(addr))?;
            } else {
                writeln!(f, "{{")?;
            }
            for instr in body {
                pp_cinstr(f, &instr.inner, d + 1)?;
            }
            pad(f, d)?;
            writeln!(f, "}}")
        }
    }
}
