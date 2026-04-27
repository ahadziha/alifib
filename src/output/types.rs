//! Plain data types representing the normalized output of the interpreter.
//!
//! These types form a name-keyed, ID-free view of the interpreter's internal
//! state. They are produced by [`crate::interpreter::GlobalStore::normalize`]
//! and consumed by the [`std::fmt::Display`] impls that render human-readable
//! output.
//!
//! The hierarchy is: [`Store`] → [`Module`] → [`Type`] → [`Dim`] / [`Cell`] / [`Map`].

use std::fmt;

// ---- Data types ----

/// A name-keyed, ID-free snapshot of the interpreter state after a full run.
///
/// Produced by [`crate::interpreter::GlobalStore::normalize`]. Suitable for
/// structural equality tests (`assert_eq!`) and as the source for the
/// human-readable text renderer.
#[derive(Debug, PartialEq)]
pub struct Store {
    /// Total number of non-type cells across all modules.
    pub cells_count: usize,
    /// Total number of named types across all modules.
    pub types_count: usize,
    /// Modules in load order (dependencies before dependents).
    pub modules: Vec<Module>,
}

/// One module's worth of types within a [`Store`], in load order.
#[derive(Debug, PartialEq)]
pub struct Module {
    /// Canonical file-system path to the source file.
    pub path: String,
    /// All named types defined in this module, in definition order.
    pub types: Vec<Type>,
}

/// A single named type (or the unnamed root type) within a [`Module`].
#[derive(Debug, PartialEq)]
pub struct Type {
    /// The type's name. Empty string for the unnamed root type, which is
    /// displayed as `<empty>`.
    pub name: String,
    /// Generators of this type grouped by dimension, in ascending order.
    pub dims: Vec<Dim>,
    /// Diagrams explicitly named inside this type's body, sorted by name.
    pub diagrams: Vec<Cell>,
    /// Maps from this type to other types or modules, sorted by name.
    pub maps: Vec<Map>,
}

/// The generators of a single dimension within a [`Type`].
#[derive(Debug, PartialEq)]
pub struct Dim {
    /// The dimension (0 = points, 1 = arrows, 2 = 2-cells, …).
    pub dim: usize,
    /// Generators at this dimension, sorted by name.
    pub cells: Vec<Cell>,
}

/// A named generator or diagram together with its boundary.
///
/// The boundary is expressed as structured term expressions derived from the
/// paste tree. Both `src` and `tgt` are empty for 0-dimensional generators,
/// which have no boundary.
///
/// The [`std::fmt::Display`] impl renders this as `name : src -> tgt` for
/// higher-dimensional cells and as `name` for 0-dimensional ones.
#[derive(Debug, PartialEq)]
pub struct Cell {
    /// Generator name. Empty string is displayed as `<empty>`.
    pub name: String,
    /// Source boundary as a structured term expression (empty for 0-cells).
    pub src: String,
    /// Target boundary as a structured term expression (empty for 0-cells).
    pub tgt: String,
}

/// A named map from a type to another type or module.
///
/// The [`std::fmt::Display`] impl renders this as `name :: domain`.
#[derive(Debug, PartialEq)]
pub struct Map {
    /// Map name. Empty string is displayed as `<empty>`.
    pub name: String,
    /// Name of the target type or module.
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
                writeln!(f, "  [{}]", dg.dim)?;
                for cell in &dg.cells {
                    writeln!(f, "    {}", cell)?;
                }
            }
        }
        if !self.diagrams.is_empty() {
            writeln!(f, "  Diagrams")?;
            for diagram in &self.diagrams {
                writeln!(f, "    {}", diagram)?;
            }
        }
        if !self.maps.is_empty() {
            writeln!(f, "  Maps")?;
            for map in &self.maps {
                writeln!(f, "    {}", map)?;
            }
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
            write!(f, "{} : {} -> {}", label, self.src, self.tgt)
        }
    }
}

impl fmt::Display for Map {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = if self.name.is_empty() { "<empty>" } else { &self.name };
        write!(f, "{} :: {}", label, self.domain)
    }
}
