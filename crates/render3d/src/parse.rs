//! NIF bytes → owned per-shape data (03 §1.1): pure, `Send`, no GPU, so a worker can parse while the render
//! thread keeps drawing. A shape's `world` is the root..parent NiNode chain x its own transform (`Mesh.transform`, local); a block
//! with no parent entry keeps its own transform.
use nif::material::MaterialState;
use std::sync::Arc;

/// Stable for the life of a load: the NIF block index of the shape (`nif::scan::ShapeRow.block`). Built-in
/// shapes use `ShapeId(0)` (block 0 is the root node, never a shape). Storage order is not draw order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShapeId(pub u32);

/// One drawable shape: a BSLightingShaderProperty shape whose geometry decoded and passed `mesh::validate`.
#[derive(Debug, Clone)]
pub struct ParsedShape {
    pub id: ShapeId,
    /// The NIF name, or `"block N"` when it has none (P0's label). Shared with the NIF string table (Note GA1).
    pub name: Arc<str>,
    pub mesh: Arc<nif::geom::Mesh>,
    /// Root … parent chain × `mesh.transform` (3×4 row-major, the `Mesh.transform` convention). The draw's model
    /// matrix and the bounds use it. Skinned shapes carry it too, but their VS ignores World.
    pub world: [[f32; 4]; 3],
    pub material: MaterialState,
}

/// A whole NIF, parsed. `skipped` lists every scanned shape that is not in `shapes`, with its reason; the same
/// text is also a `warnings` line (not a Lighting shape) or an `errors` line (geometry), exactly as P0's
/// `LoadReport` printed them, so a load can copy `warnings` / `errors` unchanged.
#[derive(Debug, Clone, Default)]
pub struct ParsedNif {
    /// BSVER 83: nothing is parsed further (LE meshes are not rendered, 00 §6.5).
    pub le_mesh: bool,
    pub shapes: Vec<ParsedShape>,
    pub skipped: Vec<(ShapeId, String)>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    /// The NIF is a head part (Note BA: a facegendata NIF or an HDPT model): every shape is CS "head". `parse`
    /// cannot know it (it needs the NIF path and the plugins); the caller sets it before loading.
    pub head_part: bool,
}

/// Scans the NIF and decodes the geometry of every BSLightingShaderProperty shape, in block order.
pub fn parse(nif_bytes: &[u8]) -> ParsedNif {
    let mut out = ParsedNif::default();
    let scan = match nif::scan::scan(nif_bytes) {
        Ok(s) => s,
        Err(e) => {
            out.errors.push(format!("NIF: {e}"));
            return out;
        }
    };
    out.warnings
        .extend(scan.problems.iter().map(|p| format!("NIF: {p}")));
    if scan.le_mesh {
        out.le_mesh = true;
        return out;
    }
    let header = match nif::header::Header::parse(nif_bytes) {
        Ok(h) => h,
        Err(e) => {
            out.errors.push(format!("NIF header: {e}"));
            return out;
        }
    };
    let graph = nif::graph::read_graph(&header, nif_bytes);
    out.warnings
        .extend(graph.problems.iter().map(|p| format!("NIF: {p}")));
    for row in &scan.shapes {
        let id = ShapeId(row.block);
        let label: Arc<str> = row
            .head
            .name
            .clone()
            .unwrap_or_else(|| format!("block {}", row.block).into());
        let nif::scan::Shader::Lighting(lighting, slots) = &row.shader else {
            let why = format!("{label}: not a BSLightingShaderProperty shape, not drawn in P0");
            out.warnings.push(why.clone());
            out.skipped.push((id, why));
            continue;
        };
        let mesh = match nif::geom::read_shape(&header, nif_bytes, row.block as usize) {
            Ok(m) => m,
            Err(e) => {
                let why = format!("{label}: {e}");
                out.errors.push(why.clone());
                out.skipped.push((id, why));
                continue;
            }
        };
        if let Err(e) = crate::mesh::validate(&mesh) {
            let why = format!("{label}: {e}");
            out.errors.push(why.clone());
            out.skipped.push((id, why));
            continue;
        }
        if row.block != 0 && !graph.parent.contains_key(&row.block) {
            out.warnings.push(format!(
                "{label}: not under any known node; placed by its own transform"
            ));
        }
        let world = nif::graph::world(&graph, row.block, &mesh.transform);
        out.shapes.push(ParsedShape {
            id,
            name: label,
            world,
            mesh: Arc::new(mesh),
            material: MaterialState::from_lighting(&row.head, lighting, slots, row.alpha),
        });
    }
    out
}
