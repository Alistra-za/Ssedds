//! NIF bytes → owned per-shape data (03 §1.1): pure, `Send`, no GPU, so a worker can parse while the render
//! thread keeps drawing. P1-A places each shape by its own transform (`Mesh.transform`) only; parent NiNode
//! transforms are P1-B.
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
    /// The NIF name, or `"block N"` when it has none (P0's label).
    pub name: String,
    pub mesh: Arc<nif::geom::Mesh>,
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
    for row in &scan.shapes {
        let id = ShapeId(row.block);
        let label = row
            .head
            .name
            .clone()
            .unwrap_or_else(|| format!("block {}", row.block));
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
        out.shapes.push(ParsedShape {
            id,
            name: label,
            mesh: Arc::new(mesh),
            material: MaterialState::from_lighting(&row.head, lighting, slots, row.alpha),
        });
    }
    out
}
