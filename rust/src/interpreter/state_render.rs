use super::diagram::render_diagram;
use super::global_store::GlobalStore;
use crate::aux::Tag;
use crate::core::complex::{Complex, MapDomain};
use crate::core::diagram::{CellData, Diagram, Sign};
use std::fmt;

fn name_or_empty(s: &str) -> &str {
    if s.is_empty() { "<empty>" } else { s }
}

/// Render a cell's boundary as `name : src -> tgt`, or just `name` for 0-cells.
fn render_cell(name: &str, data: &CellData, complex: &Complex) -> String {
    let label = name_or_empty(name);
    match data {
        CellData::Zero => label.to_owned(),
        CellData::Boundary {
            boundary_in,
            boundary_out,
        } => {
            let src = render_diagram(boundary_in, complex);
            let tgt = render_diagram(boundary_out, complex);
            format!("{} : {} -> {}", label, src, tgt)
        }
    }
}

/// Render a named diagram with its boundary, e.g. `alpha : f g -> h k`.
fn render_named_diagram(name: &str, diag: &Diagram, complex: &Complex) -> String {
    let label = name_or_empty(name);
    let d = diag.dim();
    if d <= 0 {
        return label.to_owned();
    }
    let k = (d - 1) as usize;
    let (Ok(src), Ok(tgt)) = (
        Diagram::boundary(Sign::Source, k, diag),
        Diagram::boundary(Sign::Target, k, diag),
    ) else {
        return label.to_owned();
    };
    format!(
        "{} : {} -> {}",
        label,
        render_diagram(&src, complex),
        render_diagram(&tgt, complex),
    )
}

fn render_domain(domain: &MapDomain, module_complex: &Complex) -> String {
    match domain {
        MapDomain::Type(gid) => {
            let tag = Tag::Global(*gid);
            module_complex
                .find_generator_by_tag(&tag)
                .map(|n| name_or_empty(n).to_owned())
                .unwrap_or_else(|| format!("{}", gid))
        }
        MapDomain::Module(mid) => mid.clone(),
    }
}

impl fmt::Display for GlobalStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} cells, {} types, {} modules",
            self.cells.len(),
            self.types.len(),
            self.modules.len(),
        )?;

        let mut module_entries: Vec<_> = self
            .modules
            .iter()
            .map(|(id, arc)| (id.as_str(), &**arc))
            .collect();
        module_entries.sort_by_key(|(id, _)| *id);

        for (module_id, module_complex) in &module_entries {
            write!(f, "\n* Module {}\n", module_id)?;

            let generator_names = module_complex.generator_names();
            for (i, gen_name) in generator_names.iter().enumerate() {
                if i > 0 {
                    writeln!(f)?;
                }
                let type_label = name_or_empty(gen_name);

                let Some(gen_entry) = module_complex.find_generator(gen_name) else {
                    writeln!(f, "Type {} (missing)", type_label)?;
                    continue;
                };
                let Tag::Global(gid) = &gen_entry.tag else {
                    writeln!(f, "Type {} (local)", type_label)?;
                    continue;
                };
                let Some(type_entry) = self.find_type(*gid) else {
                    writeln!(f, "Type {} (not found)", type_label)?;
                    continue;
                };

                writeln!(f, "Type {}", type_label)?;
                let tc = &type_entry.complex;

                // Cells grouped by dimension, with boundaries
                let mut dims: Vec<usize> = tc
                    .generator_names()
                    .iter()
                    .filter_map(|n| tc.generator_dim(n))
                    .collect();
                dims.sort_unstable();
                dims.dedup();

                if dims.is_empty() {
                    writeln!(f, "  (no cells)")?;
                } else {
                    for dim in &dims {
                        let mut gens = tc.generators_in_dim(*dim);
                        gens.sort();

                        let rendered: Vec<String> = gens
                            .iter()
                            .filter_map(|name| {
                                let entry = tc.find_generator(name)?;
                                let data = self.cell_data_for_tag(tc, &entry.tag)?;
                                Some(render_cell(name, &data, tc))
                            })
                            .collect();

                        if !rendered.is_empty() {
                            writeln!(f, "  [{}] {}", dim, rendered.join(", "))?;
                        }
                    }
                }

                // Diagrams
                let diagram_names = tc.diagram_names();
                if !diagram_names.is_empty() {
                    let diags: Vec<String> = diagram_names
                        .iter()
                        .filter_map(|n| {
                            let diag = tc.find_diagram(n)?;
                            Some(render_named_diagram(n, diag, tc))
                        })
                        .collect();
                    writeln!(f, "  Diagrams: {}", diags.join(", "))?;
                }

                // Maps
                let map_names = tc.map_names();
                if !map_names.is_empty() {
                    let maps: Vec<String> = map_names
                        .iter()
                        .map(|mn| {
                            let dom = tc
                                .find_map(mn)
                                .map(|me| render_domain(&me.domain, module_complex))
                                .unwrap_or_else(|| "?".into());
                            format!("{} :: {}", name_or_empty(mn), dom)
                        })
                        .collect();
                    writeln!(f, "  Maps: {}", maps.join(", "))?;
                }
            }
        }
        Ok(())
    }
}
