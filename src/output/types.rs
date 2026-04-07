use std::fmt;

// ---- Data types ----

/// A name-keyed view of a [`super::normalize::GlobalStore`], free of opaque IDs.
///
/// Suitable for structural equality tests and as the intermediate form for the
/// string renderer. Produced by [`crate::interpreter::GlobalStore::normalize`].
#[derive(Debug, PartialEq)]
pub struct Store {
    pub cells_count: usize,
    pub types_count: usize,
    pub modules: Vec<Module>,
}

/// A single module section in a [`Store`], in load order.
#[derive(Debug, PartialEq)]
pub struct Module {
    pub path: String,
    /// Types (named generators) sorted by name.
    pub types: Vec<Type>,
}

/// A single type within a [`Module`].
#[derive(Debug, PartialEq)]
pub struct Type {
    /// Empty string for the unnamed root type (displayed as `<empty>`).
    pub name: String,
    /// Generators grouped by dimension in ascending order.
    pub dims: Vec<Dim>,
    /// Named diagrams, sorted by name.
    pub diagrams: Vec<Cell>,
    /// Named maps to other types or modules, sorted by name.
    pub maps: Vec<Map>,
}

/// Generators of a single dimension within a [`Type`].
#[derive(Debug, PartialEq)]
pub struct Dim {
    pub dim: usize,
    /// Generators at this dimension, sorted by name.
    pub cells: Vec<Cell>,
}

/// A named generator or diagram, with its source and target boundary expressed
/// as lists of generator names. Both lists are empty for 0-dimensional cells.
#[derive(Debug, PartialEq)]
pub struct Cell {
    pub name: String,
    pub src: Vec<String>,
    pub tgt: Vec<String>,
}

/// A named map to another type or module.
#[derive(Debug, PartialEq)]
pub struct Map {
    pub name: String,
    pub domain: String,
}

// ---- Display impls ----

impl fmt::Display for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} cells, {} types, {} modules",
                 self.cells_count, self.types_count, self.modules.len())?;
        for module in &self.modules {
            write!(f, "{}", module)?;
        }
        Ok(())
    }
}

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\n* Module {}\n", self.path)?;
        for (i, t) in self.types.iter().enumerate() {
            if i > 0 { writeln!(f)?; }
            write!(f, "{}", t)?;
        }
        Ok(())
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = if self.name.is_empty() { "<empty>" } else { &self.name };
        writeln!(f, "Type {}", label)?;
        if self.dims.is_empty() {
            writeln!(f, "  (no cells)")?;
        } else {
            for dg in &self.dims {
                let cells = dg.cells.iter().map(|c| c.to_string()).collect::<Vec<_>>();
                writeln!(f, "  [{}] {}", dg.dim, cells.join(", "))?;
            }
        }
        if !self.diagrams.is_empty() {
            let diagrams = self.diagrams.iter().map(|d| d.to_string()).collect::<Vec<_>>();
            writeln!(f, "  Diagrams: {}", diagrams.join(", "))?;
        }
        if !self.maps.is_empty() {
            let maps = self.maps.iter().map(|m| m.to_string()).collect::<Vec<_>>();
            writeln!(f, "  Maps: {}", maps.join(", "))?;
        }
        Ok(())
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = if self.name.is_empty() { "<empty>" } else { &self.name };
        if self.src.is_empty() && self.tgt.is_empty() {
            write!(f, "{}", label)
        } else {
            write!(f, "{} : {} -> {}", label, self.src.join(" "), self.tgt.join(" "))
        }
    }
}

impl fmt::Display for Map {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = if self.name.is_empty() { "<empty>" } else { &self.name };
        write!(f, "{} :: {}", label, self.domain)
    }
}
