//! the reference format `paint/scene.assbin` (Assimp dump 1.7) to a triangle mesh.

mod header;
mod scene;
mod scene_blob;

use glam::{Mat3, Mat4, Vec2, Vec3};

use header::{assbin_header, payload_offset};
use scene::ParsedMesh;

#[derive(Debug)]
pub enum ImportError {
    Assbin(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Assbin(msg) => write!(f, "{msg}"),
        }
    }
}

/// Mesh decoded from the file's scene dump. UVs use TextureCraft's convention (v down).
#[derive(Clone, Debug)]
pub struct SceneMesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub materials: Vec<String>,
    /// Part name, material index, first index, index count.
    pub parts: Vec<(String, usize, u32, u32)>,
}

/// Load a the reference format 1.7 `scene.assbin`.
pub fn import_assbin(bytes: &[u8]) -> Result<SceneMesh, String> {
    let header = assbin_header(bytes).map_err(|e| e.to_string())?;
    if !header.is_format_v17() {
        return Err(format!("not an assbin 1.7 mesh (format {}.{})", header.format_major, header.format_minor));
    }
    scene::import_scene_v17(bytes, &header).map_err(|e| e.to_string())
}

pub(crate) fn v17_tail_offset(bytes: &[u8]) -> Result<usize, ImportError> {
    let header = assbin_header(bytes)?;
    if !header.is_format_v17() {
        return Err(ImportError::Assbin("not format 1.7".into()));
    }
    let off = payload_offset(&header);
    if bytes.len() < off + 4 {
        return Err(ImportError::Assbin("block length truncated".into()));
    }
    let block_len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap_or([0; 4])) as usize;
    Ok(off + 4 + block_len)
}

pub(crate) fn scene_to_mesh(name: &str, instances: &[(Mat4, u32)], meshes: &[ParsedMesh]) -> Result<SceneMesh, ImportError> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    let mut materials = Vec::new();
    let mut parts = Vec::new();
    for (world, mesh_idx) in instances {
        let mesh = meshes.get(*mesh_idx as usize).ok_or_else(|| ImportError::Assbin(format!("node references missing mesh {mesh_idx}")))?;
        let material = if let Some(i) = materials.iter().position(|m| m == &mesh.material_name) {
            i
        } else {
            materials.push(mesh.material_name.clone());
            materials.len() - 1
        };
        let start = indices.len() as u32;
        let normal_m = Mat3::from_mat4(*world).inverse().transpose();
        let flip = world.determinant() < 0.0;
        let base = positions.len() as u32;
        for (i, p) in mesh.positions.iter().enumerate() {
            positions.push(world.transform_point3(*p).to_array());
            let n = mesh.normals.get(i).copied().unwrap_or(Vec3::Y);
            let n = (normal_m * n).normalize_or_zero();
            normals.push((if n == Vec3::ZERO { Vec3::Y } else { n }).to_array());
            let uv = mesh.uvs.get(i).copied().unwrap_or(Vec2::ZERO);
            uvs.push([uv.x, uv.y]);
        }
        for tri in &mesh.triangles {
            let t = if flip { [tri[0], tri[2], tri[1]] } else { *tri };
            indices.extend([base + t[0], base + t[1], base + t[2]]);
        }
        let count = indices.len() as u32 - start;
        if count > 0 {
            parts.push((mesh.mesh_name.clone(), material, start, count));
        }
    }
    if positions.is_empty() || indices.is_empty() {
        return Err(ImportError::Assbin("assbin 1.7: no triangles decoded".into()));
    }
    if materials.is_empty() {
        materials.push("Default".into());
    }
    Ok(SceneMesh { name: name.to_string(), positions, normals, uvs, indices, materials, parts })
}
