use glam::Mat4;
use std::path::Path;

/// Skinned 3D vertex with bone weight indices and influence factors for GPU skinning.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinnedVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub joints: [u32; 4],
    pub weights: [f32; 4],
}

impl SkinnedVertex {
    pub const fn new(
        position: [f32; 3],
        normal: [f32; 3],
        uv: [f32; 2],
        joints: [u32; 4],
        weights: [f32; 4],
    ) -> Self {
        Self {
            position,
            normal,
            uv,
            joints,
            weights,
        }
    }
}

/// Extracted GLTF 2.0 Mesh data ready for GPU buffers.
#[derive(Debug, Clone)]
pub struct GltfMesh {
    pub name: String,
    pub vertices: Vec<SkinnedVertex>,
    pub indices: Vec<u32>,
    pub inverse_bind_matrices: Vec<Mat4>,
}

/// Load a `.gltf` or `.glb` asset from disk.
pub fn load_gltf(path: &Path) -> Result<Vec<GltfMesh>, Box<dyn std::error::Error>> {
    let (document, buffers, _images) = gltf::import(path)?;
    let mut result_meshes = Vec::new();

    for mesh in document.meshes() {
        let mesh_name = mesh.name().unwrap_or("UnnamedMesh").to_string();
        let mut all_vertices = Vec::new();
        let mut all_indices = Vec::new();

        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

            let positions: Vec<[f32; 3]> = reader
                .read_positions()
                .map(|p| p.collect())
                .unwrap_or_default();
            let normals: Vec<[f32; 3]> = reader
                .read_normals()
                .map(|n| n.collect())
                .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
            let uvs: Vec<[f32; 2]> = reader
                .read_tex_coords(0)
                .map(|t| t.into_f32().collect())
                .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);
            let joints: Vec<[u32; 4]> = reader
                .read_joints(0)
                .map(|j| {
                    j.into_u16()
                        .map(|v| [v[0] as u32, v[1] as u32, v[2] as u32, v[3] as u32])
                        .collect()
                })
                .unwrap_or_else(|| vec![[0, 0, 0, 0]; positions.len()]);
            let weights: Vec<[f32; 4]> = reader
                .read_weights(0)
                .map(|w| w.into_f32().collect())
                .unwrap_or_else(|| vec![[1.0, 0.0, 0.0, 0.0]; positions.len()]);

            let base_vertex = all_vertices.len() as u32;

            for i in 0..positions.len() {
                all_vertices.push(SkinnedVertex::new(
                    positions[i],
                    normals[i],
                    uvs[i],
                    joints[i],
                    weights[i],
                ));
            }

            if let Some(indices_reader) = reader.read_indices() {
                for idx in indices_reader.into_u32() {
                    all_indices.push(base_vertex + idx);
                }
            } else {
                for i in 0..positions.len() as u32 {
                    all_indices.push(base_vertex + i);
                }
            }
        }

        result_meshes.push(GltfMesh {
            name: mesh_name,
            vertices: all_vertices,
            indices: all_indices,
            inverse_bind_matrices: Vec::new(),
        });
    }

    Ok(result_meshes)
}

/// Generates a procedural 4-joint rigged monolith / character mesh
/// for fallback and instant GPU skeletal verification without external assets.
pub fn generate_procedural_rigged_mesh() -> GltfMesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // 4 vertical articulated segments (Bone 0: Base, Bone 1: Waist, Bone 2: Chest, Bone 3: Head)
    let segment_height = 0.8f32;
    let width = 0.5f32;

    for seg in 0..4 {
        let y_min = seg as f32 * segment_height;
        let y_max = y_min + segment_height;
        let bone_id = seg as u32;

        let _base_idx = vertices.len() as u32;

        // 8 box vertices per segment
        let corners = [
            [-width, y_min, -width],
            [width, y_min, -width],
            [width, y_min, width],
            [-width, y_min, width],
            [-width, y_max, -width],
            [width, y_max, -width],
            [width, y_max, width],
            [-width, y_max, width],
        ];

        let normals = [
            [0.0, 0.0, -1.0], // Front
            [1.0, 0.0, 0.0],  // Right
            [0.0, 0.0, 1.0],  // Back
            [-1.0, 0.0, 0.0], // Left
            [0.0, 1.0, 0.0],  // Top
            [0.0, -1.0, 0.0], // Bottom
        ];

        // 6 faces
        let face_indices = [
            (0, 1, 5, 4, normals[0]),
            (1, 2, 6, 5, normals[1]),
            (2, 3, 7, 6, normals[2]),
            (3, 0, 4, 7, normals[3]),
            (4, 5, 6, 7, normals[4]),
            (0, 3, 2, 1, normals[5]),
        ];

        for (i0, i1, i2, i3, norm) in face_indices {
            let start = vertices.len() as u32;
            vertices.push(SkinnedVertex::new(
                corners[i0],
                norm,
                [0.0, 0.0],
                [bone_id, (bone_id + 1).min(3), 0, 0],
                [0.85, 0.15, 0.0, 0.0],
            ));
            vertices.push(SkinnedVertex::new(
                corners[i1],
                norm,
                [1.0, 0.0],
                [bone_id, (bone_id + 1).min(3), 0, 0],
                [0.85, 0.15, 0.0, 0.0],
            ));
            vertices.push(SkinnedVertex::new(
                corners[i2],
                norm,
                [1.0, 1.0],
                [bone_id, (bone_id + 1).min(3), 0, 0],
                [0.85, 0.15, 0.0, 0.0],
            ));
            vertices.push(SkinnedVertex::new(
                corners[i3],
                norm,
                [0.0, 1.0],
                [bone_id, (bone_id + 1).min(3), 0, 0],
                [0.85, 0.15, 0.0, 0.0],
            ));

            indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        }
    }

    GltfMesh {
        name: "ProceduralSkinnedMonolith".to_string(),
        vertices,
        indices,
        inverse_bind_matrices: vec![Mat4::IDENTITY; 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_procedural_rigged_mesh() {
        let mesh = generate_procedural_rigged_mesh();
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.indices.is_empty());
        assert_eq!(mesh.inverse_bind_matrices.len(), 4);
        assert_eq!(mesh.indices.len() % 3, 0); // Valid triangles
    }
}
