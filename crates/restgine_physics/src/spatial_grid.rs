use glam::Vec2;
use hecs::Entity;
use std::collections::{HashMap, HashSet};

/// 2D Spatial Hash Grid for high-performance $O(1)$ spatial queries (monsters, tears, obstacles).
#[derive(Debug, Clone)]
pub struct SpatialHashGrid {
    cell_size: f32,
    grid: HashMap<(i32, i32), Vec<Entity>>,
}

impl SpatialHashGrid {
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell_size: cell_size.max(8.0),
            grid: HashMap::new(),
        }
    }

    /// Clear all stored entities from the grid.
    pub fn clear(&mut self) {
        self.grid.clear();
    }

    fn to_cell(&self, x: f32, y: f32) -> (i32, i32) {
        (
            (x / self.cell_size).floor() as i32,
            (y / self.cell_size).floor() as i32,
        )
    }

    /// Insert an entity with an AABB bounding footprint into overlapping grid cells.
    pub fn insert_aabb(&mut self, entity: Entity, min: Vec2, max: Vec2) {
        let (min_cx, min_cy) = self.to_cell(min.x, min.y);
        let (max_cx, max_cy) = self.to_cell(max.x, max.y);

        for cx in min_cx..=max_cx {
            for cy in min_cy..=max_cy {
                self.grid.entry((cx, cy)).or_default().push(entity);
            }
        }
    }

    /// Query all unique entities located in cells overlapping the given bounding box.
    pub fn query_range(&self, min: Vec2, max: Vec2) -> HashSet<Entity> {
        let mut results = HashSet::new();
        let (min_cx, min_cy) = self.to_cell(min.x, min.y);
        let (max_cx, max_cy) = self.to_cell(max.x, max.y);

        for cx in min_cx..=max_cx {
            for cy in min_cy..=max_cy {
                if let Some(entities) = self.grid.get(&(cx, cy)) {
                    for &entity in entities {
                        results.insert(entity);
                    }
                }
            }
        }

        results
    }
}
