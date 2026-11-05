//! String-diagram views of the loaded module and the running session.

use serde::Deserialize;
use serde_json::json;

use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, Sign};
use crate::core::partial_map::PartialMap;
use crate::analysis::strdiag::{StrDiag, VertexKind};
use crate::interpreter::GlobalStore;
use crate::output::render_diagram;

use super::kernel::Kernel;
use super::response::resolve_domain_complex;
use super::type_info::global_tag;

/// What a canvas draws: asked for in a page's `draw`, never on its own.
#[derive(Deserialize)]
#[serde(tag = "of", rename_all = "snake_case")]
pub enum View {
    Item { module: String, type_name: String, item: String, boundary: Option<Boundary> },
    MapImage { module: String, type_name: String, map: String, generator: String, boundary: Option<Boundary> },
    Current,
    Target,
    Preview { choice: usize },
    Proof,
}

#[derive(Deserialize)]
pub struct Boundary {
    pub dim: usize,
    pub side: Side,
}

/// The wire's boundary side; the core's `Sign` has no serde dependency.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Input,
    Output,
}

impl Boundary {
    fn into_boundary(self) -> (usize, Sign) {
        let sign = match self.side {
            Side::Input => Sign::Input,
            Side::Output => Sign::Output,
        };
        (self.dim, sign)
    }
}

impl Kernel {
    /// Every view answers an object carrying its `strdiag`.
    pub fn view(&mut self, v: View) -> Result<serde_json::Value, String> {
        match v {
            View::Item { module, type_name, item, boundary } => {
                let s = self.session().ok_or("no source loaded")?;
                build_strdiag_response(
                    s.store(), &module, &type_name, &item,
                    boundary.map(Boundary::into_boundary),
                )
            }
            View::MapImage { module, type_name, map, generator, boundary } => {
                let s = self.session().ok_or("no source loaded")?;
                build_map_image_strdiag(
                    s.store(), &module, &type_name, &map, &generator,
                    boundary.map(Boundary::into_boundary),
                )
            }
            View::Current => {
                let engine = self.session().and_then(|s| s.engine())
                    .ok_or("no session active")?;
                Ok(session_strdiag(engine.current_diagram(), engine.type_complex()))
            }
            View::Target => {
                let engine = self.session().and_then(|s| s.engine())
                    .ok_or("no session active")?;
                let target = engine.target_diagram().ok_or("no target set for this session")?;
                Ok(session_strdiag(target, engine.type_complex()))
            }
            View::Preview { choice } => {
                let engine = self.session().and_then(|s| s.engine())
                    .ok_or("no session active")?;
                let rewrite = engine.rewrites().get(choice)
                    .ok_or_else(|| format!("choice {} out of range", choice))?;
                let preview = engine.advance(&rewrite.step)?;
                Ok(session_strdiag(&preview, engine.type_complex()))
            }
            View::Proof => {
                let engine = self.session_mut().and_then(|s| s.engine_mut())
                    .ok_or("no session active")?;
                let proof = engine.proof_diagram()?;
                let scope = engine.type_complex();
                let step_count = engine.step_count();
                let n = engine.initial_diagram().top_dim();
                let current = engine.current_diagram().clone();

                let sd = StrDiag::from_diagram_at_dim(&proof, scope, n + 1);

                let current_sign = if engine.backward() { Sign::Input } else { Sign::Output };
                let boundary_map = Diagram::boundary_correspondence(current_sign, n, &proof, &current)
                    .map_err(|e| format!("boundary map: {}", e))?;

                Ok(json!({
                    "strdiag": strdiag_to_json(&sd),
                    "step_count": step_count,
                    "output_boundary_map": boundary_map,
                }))
            }
        }
    }
}

/// Serialize a [`StrDiag`] to a JSON value; a vertex's `tag` is its global id,
/// or `null` for any other tag.
fn strdiag_to_json(sd: &StrDiag) -> serde_json::Value {
    fn edges_json(graph: &crate::aux::graph::DiGraph) -> Vec<[usize; 2]> {
        let mut edges = Vec::new();
        for (u, succs) in graph.successors.iter().enumerate() {
            for &v in succs {
                edges.push([u, v]);
            }
        }
        edges
    }

    let vertices: Vec<serde_json::Value> = (0..sd.num_vertices())
        .map(|i| {
            json!({
                "index": i,
                "kind": match sd.kinds[i] { VertexKind::Wire => "wire", VertexKind::Node => "node" },
                "label": sd.labels[i],
                "tag": sd.tags[i].as_ref().and_then(global_tag),
            })
        })
        .collect();

    json!({
        "num_wires": sd.num_wires,
        "num_nodes": sd.num_nodes,
        "vertices": vertices,
        "height": { "edges": edges_json(&sd.height) },
        "width": { "edges": edges_json(&sd.width) },
        "depth": { "edges": edges_json(&sd.depth) },
    })
}

/// The string diagram of `diagram`, or of its `(sign, dim)`-boundary when
/// `boundary` is given, with its `dim`, `label`, `input`, and `output`.
fn diagram_strdiag_json(
    diagram: &Diagram,
    scope: &Complex,
    boundary: Option<(usize, Sign)>,
) -> Result<serde_json::Value, String> {
    let rendered = match boundary {
        None => diagram.clone(),
        Some((k, sign)) => {
            Diagram::boundary(sign, k, diagram)
                .map_err(|e| format!("boundary extraction failed: {}", e))?
        }
    };

    let dim = rendered.top_dim();
    let (input, output) = if dim >= 1 {
        let inp = Diagram::boundary(Sign::Input, dim - 1, &rendered)
            .map_err(|e| format!("{}", e))?;
        let out = Diagram::boundary(Sign::Output, dim - 1, &rendered)
            .map_err(|e| format!("{}", e))?;
        (
            render_diagram(&inp, scope),
            render_diagram(&out, scope),
        )
    } else {
        (String::new(), String::new())
    };

    let label = render_diagram(&rendered, scope);
    let sd = StrDiag::from_diagram(&rendered, scope);

    Ok(json!({
        "strdiag": strdiag_to_json(&sd),
        "dim": dim,
        "label": label,
        "input": input,
        "output": output,
    }))
}

/// The view of a named item within a type: a named diagram first, else a
/// generator's classifier.
fn build_strdiag_response(
    store: &GlobalStore,
    module_path: &str,
    type_name: &str,
    item_name: &str,
    boundary: Option<(usize, Sign)>,
) -> Result<serde_json::Value, String> {
    let type_complex = super::engine::resolve_type(store, module_path, type_name)?;
    let diagram = type_complex.find_diagram(item_name)
        .or_else(|| type_complex.classifier(item_name))
        .ok_or_else(|| format!("'{}' not found in type '{}'", item_name, type_name))?;
    diagram_strdiag_json(diagram, &type_complex, boundary)
}

/// The view of a domain generator's image under a map.
fn build_map_image_strdiag(
    store: &GlobalStore,
    module_path: &str,
    type_name: &str,
    map_name: &str,
    gen_name: &str,
    boundary: Option<(usize, Sign)>,
) -> Result<serde_json::Value, String> {
    let type_complex = super::engine::resolve_type(store, module_path, type_name)?;
    let (pmap, domain) = type_complex.find_map(map_name)
        .ok_or_else(|| format!("map '{}' not found in type '{}'", map_name, type_name))?;

    let domain_complex = resolve_domain_complex(store, domain)
        .ok_or_else(|| format!("domain of map '{}' could not be resolved", map_name))?;

    let gen_classifier = domain_complex.classifier(gen_name)
        .ok_or_else(|| format!("generator '{}' not found in domain of '{}'", gen_name, map_name))?;

    let image = PartialMap::apply(pmap, gen_classifier)
        .map_err(|e| format!("failed to apply map: {}", e))?;

    diagram_strdiag_json(&image, &type_complex, boundary)
}

/// The view of a session diagram: its string diagram alone.
fn session_strdiag(diagram: &Diagram, scope: &Complex) -> serde_json::Value {
    json!({ "strdiag": strdiag_to_json(&StrDiag::from_diagram(diagram, scope)) })
}
