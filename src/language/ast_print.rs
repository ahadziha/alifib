//! Round-trip pretty-printer for the alifib AST.
//!
//! Produces valid alifib source text that, when re-parsed, yields an
//! equivalent AST.  Comments and original whitespace are not preserved.

use super::ast::*;

// ---- Public entry point ----

/// Pretty-print a [`Program`] as valid alifib source text.
///
/// The output is guaranteed to re-parse to an equivalent AST.
/// Comments and original whitespace are not preserved.
pub fn print_program(program: &Program) -> String {
    let mut p = Printer::new();
    p.program(program);
    p.finish()
}

// ---- Printer ----

struct Printer {
    buf: String,
    /// Current indentation depth; each level is 2 spaces.
    depth: usize,
}

impl Printer {
    fn new() -> Self {
        Self { buf: String::new(), depth: 0 }
    }

    fn finish(self) -> String {
        self.buf
    }

    // ---- Low-level output helpers ----

    fn s(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    /// Emit a newline followed by `depth * 2` spaces.
    fn newline(&mut self) {
        self.buf.push('\n');
        for _ in 0..self.depth {
            self.buf.push_str("  ");
        }
    }

    // ---- Top level ----

    /// Print a program as a sequence of blocks, separated by blank lines.
    fn program(&mut self, program: &Program) {
        for (i, block) in program.blocks.iter().enumerate() {
            if i > 0 {
                self.buf.push_str("\n\n");
            }
            self.block(&block.inner);
        }
    }

    // ---- Blocks ----

    fn block(&mut self, block: &Block) {
        match block {
            Block::TypeBlock(insts) => self.type_block(insts),
            Block::LocalBlock { complex, body } => self.local_block(&complex.inner, body),
        }
    }

    /// `@Type` followed by comma-separated type instructions at the current indent.
    fn type_block(&mut self, insts: &[Spanned<TypeInst>]) {
        self.s("@Type");
        for (i, inst) in insts.iter().enumerate() {
            self.newline();
            self.type_inst(&inst.inner);
            if i + 1 < insts.len() {
                self.s(",");
                // Blank line between type-level entries for readability.
                self.buf.push('\n');
            }
        }
    }

    /// `@Complex` followed by comma-separated local instructions, each indented one level.
    fn local_block(&mut self, complex: &Complex, body: &[Spanned<LocalInst>]) {
        self.s("@");
        self.complex(complex);
        self.depth += 1;
        for (i, inst) in body.iter().enumerate() {
            self.newline();
            self.local_inst(&inst.inner);
            if i + 1 < body.len() {
                self.s(",");
            }
        }
        self.depth -= 1;
    }

    // ---- Type-level instructions ----

    fn type_inst(&mut self, inst: &TypeInst) {
        match inst {
            TypeInst::Generator(g) => self.generator(g),
            TypeInst::LetDiag(ld) => self.let_diag(ld),
            TypeInst::DefPartialMap(dp) => self.def_partial_map(dp),
            TypeInst::IncludeModule(im) => self.include_module(im),
        }
    }

    /// `Name : Boundary <<= Complex`  or  `Name <<= Complex`
    fn generator(&mut self, g: &Generator) {
        self.name_with_boundary(&g.name.inner);
        self.s(" <<= ");
        self.complex(&g.complex.inner);
    }

    /// `include Name`  or  `include Name as Alias`
    fn include_module(&mut self, im: &IncludeModule) {
        self.s("include ");
        self.s(&im.name.inner);
        if let Some(alias) = &im.alias {
            self.s(" as ");
            self.s(&alias.inner);
        }
    }

    // ---- Local instructions ----

    fn local_inst(&mut self, inst: &LocalInst) {
        match inst {
            LocalInst::LetDiag(ld) => self.let_diag(ld),
            LocalInst::DefPartialMap(dp) => self.def_partial_map(dp),
            LocalInst::AssertStmt(a) => {
                self.s("assert ");
                self.diagram(&a.lhs.inner);
                self.s(" = ");
                self.diagram(&a.rhs.inner);
            }
        }
    }

    // ---- Shared instructions ----

    /// `let Name = Diagram`
    fn let_diag(&mut self, ld: &LetDiag) {
        self.s("let ");
        self.s(&ld.name.inner);
        self.s(" = ");
        self.diagram(&ld.value.inner);
    }

    /// `let [total] Name :: Address = PartialMapDef`
    fn def_partial_map(&mut self, dp: &DefPartialMap) {
        self.s("let ");
        if dp.total { self.s("total "); }
        self.s(&dp.name.inner);
        self.s(" :: ");
        self.address(&dp.address.inner);
        self.s(" = ");
        self.partial_map_def(&dp.value.inner);
    }

    // ---- Complex (type body) ----

    fn complex(&mut self, complex: &Complex) {
        match complex {
            Complex::Address(addr) => self.address(addr),
            Complex::Block { address, body } => self.complex_block(address.as_ref(), body),
        }
    }

    /// `Address? { ComplexInstr, ... }` — body is always multi-line.
    fn complex_block(&mut self, address: Option<&Address>, body: &[Spanned<ComplexInstr>]) {
        if let Some(addr) = address {
            self.address(addr);
            self.s(" ");
        }
        if body.is_empty() {
            self.s("{}");
            return;
        }
        self.s("{");
        self.depth += 1;
        for (i, instr) in body.iter().enumerate() {
            self.newline();
            self.complex_instr(&instr.inner);
            if i + 1 < body.len() {
                self.s(",");
            }
        }
        self.depth -= 1;
        self.newline();
        self.s("}");
    }

    fn complex_instr(&mut self, instr: &ComplexInstr) {
        match instr {
            ComplexInstr::NameWithBoundary(nwb) => self.name_with_boundary(nwb),
            ComplexInstr::LetDiag(ld) => self.let_diag(ld),
            ComplexInstr::DefPartialMap(dp) => self.def_partial_map(dp),
            ComplexInstr::AttachStmt(a) => self.attach_stmt(a),
            ComplexInstr::IncludeStmt(inc) => self.include_stmt(inc),
        }
    }

    /// `attach Name :: Address`  or  `attach Name :: Address along PartialMapDef`
    fn attach_stmt(&mut self, a: &AttachStmt) {
        self.s("attach ");
        self.s(&a.name.inner);
        self.s(" :: ");
        self.address(&a.address.inner);
        if let Some(along) = &a.along {
            self.s(" along ");
            self.partial_map_def(&along.inner);
        }
    }

    /// `include Address`  or  `include Address as Name`
    fn include_stmt(&mut self, inc: &IncludeStmt) {
        self.s("include ");
        self.address(&inc.address.inner);
        if let Some(alias) = &inc.alias {
            self.s(" as ");
            self.s(&alias.inner);
        }
    }

    // ---- Names and boundaries ----

    /// `Name`  or  `Name : Source -> Target`
    fn name_with_boundary(&mut self, nwb: &NameWithBoundary) {
        self.s(&nwb.name.inner);
        if let Some(b) = &nwb.boundary {
            self.s(" : ");
            self.boundary(&b.inner);
        }
    }

    /// `Diagram -> Diagram`
    fn boundary(&mut self, b: &Boundary) {
        self.diagram(&b.source.inner);
        self.s(" -> ");
        self.diagram(&b.target.inner);
    }

    /// Dot-separated identifier path.
    fn address(&mut self, addr: &Address) {
        for (i, part) in addr.iter().enumerate() {
            if i > 0 { self.s("."); }
            self.s(&part.inner);
        }
    }

    // ---- Diagrams ----

    fn diagram(&mut self, diagram: &Diagram) {
        match diagram {
            // Space-separated DExpr sequence.
            Diagram::PrincipalPaste(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    if i > 0 { self.s(" "); }
                    self.dexpr(&e.inner);
                }
            }
            // `Diagram #n DExpr+` — left-recursive, no extra parens needed.
            Diagram::Paste { lhs, dim, rhs } => {
                self.diagram(&lhs.inner);
                self.s(" #");
                self.s(&dim.inner);
                for e in rhs {
                    self.s(" ");
                    self.dexpr(&e.inner);
                }
            }
        }
    }

    fn dexpr(&mut self, dexpr: &DExpr) {
        match dexpr {
            DExpr::Component(c) => self.dcomponent(c),
            // Left-recursive dot chain: print base then `.field`.
            DExpr::Dot { base, field } => {
                self.dexpr(&base.inner);
                self.s(".");
                self.dcomponent(&field.inner);
            }
        }
    }

    fn dcomponent(&mut self, dc: &DComponent) {
        match dc {
            DComponent::PartialMap(pmb) => self.partial_map_basic(pmb),
            DComponent::In => self.s("in"),
            DComponent::Out => self.s("out"),
            DComponent::Hole => self.s("?"),
            DComponent::Paren(inner) => {
                self.s("(");
                self.diagram(&inner.inner);
                self.s(")");
            }
        }
    }

    // ---- Partial maps ----

    fn partial_map_def(&mut self, pmd: &PartialMapDef) {
        match pmd {
            PartialMapDef::PartialMap(pm) => self.partial_map(pm),
            PartialMapDef::Ext(ext) => self.partial_map_ext(ext),
        }
    }

    /// `Prefix? [ Clause, Clause, ... ]`
    ///
    /// Each clause is placed on its own indented line.
    fn partial_map_ext(&mut self, ext: &PartialMapExt) {
        if let Some(prefix) = &ext.prefix {
            self.partial_map(&prefix.inner);
            self.s(" ");
        }
        if ext.clauses.is_empty() {
            self.s("[]");
            return;
        }
        self.s("[");
        self.depth += 1;
        for (i, clause) in ext.clauses.iter().enumerate() {
            self.newline();
            self.partial_map_clause(&clause.inner);
            if i + 1 < ext.clauses.len() {
                self.s(",");
            }
        }
        self.depth -= 1;
        self.newline();
        self.s("]");
    }

    fn partial_map(&mut self, pm: &PartialMap) {
        match pm {
            PartialMap::Basic(b) => self.partial_map_basic(b),
            // Right-recursive dot: `Base.Rest`
            PartialMap::Dot { base, rest } => {
                self.partial_map_basic(base);
                self.s(".");
                self.partial_map(&rest.inner);
            }
        }
    }

    fn partial_map_basic(&mut self, pmb: &PartialMapBasic) {
        match pmb {
            PartialMapBasic::Name(n) => self.s(n),
            PartialMapBasic::Paren(pm) => {
                self.s("(");
                self.partial_map(&pm.inner);
                self.s(")");
            }
            PartialMapBasic::AnonMap { def, target } => {
                self.s("(map ");
                self.partial_map_def(&def.inner);
                self.s(" :: ");
                self.complex(&target.inner);
                self.s(")");
            }
        }
    }

    /// `Diagram => Diagram`
    fn partial_map_clause(&mut self, clause: &PartialMapClause) {
        self.diagram(&clause.lhs.inner);
        self.s(" => ");
        self.diagram(&clause.rhs.inner);
    }
}
