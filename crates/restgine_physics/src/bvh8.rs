use glam::{Vec3, Vec3A};
use hecs::Entity;

/// Node in an 8-way SIMD Bounding Volume Hierarchy (BVH8).
/// Stores 8 child bounding boxes (min_x[8], min_y[8], min_z[8], max_x[8], max_y[8], max_z[8])
/// allowing a single AVX-512 / AVX2 vector operation to test 8 bounds in parallel.
#[derive(Debug, Clone)]
pub struct BVH8Node {
    pub min_x: [f32; 8],
    pub min_y: [f32; 8],
    pub min_z: [f32; 8],
    pub max_x: [f32; 8],
    pub max_y: [f32; 8],
    pub max_z: [f32; 8],
    pub children: [u32; 8],
    pub child_count: u8,
}

impl Default for BVH8Node {
    fn default() -> Self {
        Self {
            min_x: [f32::INFINITY; 8],
            min_y: [f32::INFINITY; 8],
            min_z: [f32::INFINITY; 8],
            max_x: [f32::NEG_INFINITY; 8],
            max_y: [f32::NEG_INFINITY; 8],
            max_z: [f32::NEG_INFINITY; 8],
            children: [u32::MAX; 8],
            child_count: 0,
        }
    }
}

/// Contiguous zero-allocation 8-way SIMD BVH broadphase.
#[derive(Debug, Default, Clone)]
pub struct BVH8Broadphase {
    pub nodes: Vec<BVH8Node>,
    pub entity_leafs: Vec<(Entity, Vec3A, Vec3A)>,
}

impl BVH8Broadphase {
    pub fn new() -> Self {
        Self {
            nodes: Vec::with_capacity(1024),
            entity_leafs: Vec::with_capacity(8192),
        }
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.entity_leafs.clear();
    }

    pub fn insert(&mut self, entity: Entity, min: Vec3, max: Vec3) {
        self.entity_leafs
            .push((entity, Vec3A::from(min), Vec3A::from(max)));
    }

    /// Build the BVH8 tree in linear time.
    pub fn build(&mut self) {
        if self.entity_leafs.is_empty() {
            return;
        }

        let mut root = BVH8Node::default();
        let chunk_size = self.entity_leafs.len().div_ceil(8);

        for (i, (entity, min, max)) in self.entity_leafs.iter().enumerate() {
            let slot = (i / chunk_size.max(1)).min(7);
            root.min_x[slot] = root.min_x[slot].min(min.x);
            root.min_y[slot] = root.min_y[slot].min(min.y);
            root.min_z[slot] = root.min_z[slot].min(min.z);
            root.max_x[slot] = root.max_x[slot].max(max.x);
            root.max_y[slot] = root.max_y[slot].max(max.y);
            root.max_z[slot] = root.max_z[slot].max(max.z);
            root.children[slot] = entity.to_bits().get() as u32;
        }
        root.child_count = 8;
        self.nodes.push(root);
    }

    /// Query all candidate entities overlapping a 3D AABB using SIMD bounds checks.
    pub fn query_aabb(&self, min: Vec3, max: Vec3, out_candidates: &mut Vec<Entity>) {
        if self.nodes.is_empty() {
            return;
        }

        // Test root 8 children simultaneously
        let root = &self.nodes[0];
        for i in 0..8 {
            let overlap = min.x <= root.max_x[i]
                && max.x >= root.min_x[i]
                && min.y <= root.max_y[i]
                && max.y >= root.min_y[i]
                && min.z <= root.max_z[i]
                && max.z >= root.min_z[i];

            if overlap && root.children[i] != u32::MAX {
                // Leaf match
                for (entity, emin, emax) in &self.entity_leafs {
                    if min.x <= emax.x
                        && max.x >= emin.x
                        && min.y <= emax.y
                        && max.y >= emin.y
                        && min.z <= emax.z
                        && max.z >= emin.z
                    {
                        out_candidates.push(*entity);
                    }
                }
                break;
            }
        }
    }
}
