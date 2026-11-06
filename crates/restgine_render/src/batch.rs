use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use restgine_ecs::{RenderLayer, Sprite, Transform25D};

/// GPU instance data layout for instanced sprite rendering.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SpriteInstanceRaw {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub color_tint: [f32; 4],
}

/// An item pending render in the 2.5D sorting queue.
#[derive(Debug, Clone)]
pub struct RenderQueueItem {
    pub layer: RenderLayer,
    /// Footprint ground Y coordinate used for depth sorting within the layer.
    pub footprint_y: f32,
    pub instance: SpriteInstanceRaw,
}

/// Render queue that collects 2.5D sprites and sorts them deterministically
/// according to discrete layers and Isaac-style Y-sorting.
#[derive(Debug, Default)]
pub struct IsaacRenderBatch {
    items: Vec<RenderQueueItem>,
}

impl IsaacRenderBatch {
    pub fn new() -> Self {
        Self {
            items: Vec::with_capacity(4096),
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Submit a 2.5D sprite to the batch.
    pub fn submit_sprite(&mut self, transform: &Transform25D, sprite: &Sprite) {
        // In 2.5D projection:
        // screen_x = x
        // screen_y = y - z
        let screen_pos = transform.screen_coords() + sprite.anchor_offset;

        let uv_min = if sprite.flip_x {
            [sprite.uv_rect[2], sprite.uv_rect[1]]
        } else {
            [sprite.uv_rect[0], sprite.uv_rect[1]]
        };

        let uv_max = if sprite.flip_x {
            [sprite.uv_rect[0], sprite.uv_rect[3]]
        } else {
            [sprite.uv_rect[2], sprite.uv_rect[3]]
        };

        let instance = SpriteInstanceRaw {
            position: [screen_pos.x.round(), screen_pos.y.round()],
            size: [sprite.size.x, sprite.size.y],
            uv_min,
            uv_max,
            color_tint: sprite.color_tint,
        };

        self.items.push(RenderQueueItem {
            layer: sprite.layer,
            footprint_y: transform.ground_y(),
            instance,
        });
    }

    /// Submit a shadow decal projected directly onto the ground.
    pub fn submit_shadow(&mut self, transform: &Transform25D, radius: f32, opacity: f32) {
        // Shadow is on ground plane at (x, y), fading as height z increases
        let z_height = transform.position.z.max(0.0);
        let scale_factor = (1.0 - (z_height / 120.0)).clamp(0.2, 1.0);
        let effective_radius = radius * scale_factor;
        let effective_opacity = opacity * scale_factor;

        let ground_pos = Vec2::new(transform.position.x, transform.position.y);
        let screen_pos = ground_pos - Vec2::new(effective_radius, effective_radius * 0.5);

        let instance = SpriteInstanceRaw {
            position: [screen_pos.x.round(), screen_pos.y.round()],
            size: [effective_radius * 2.0, effective_radius],
            uv_min: [0.0, 0.0],
            uv_max: [1.0, 1.0],
            color_tint: [0.0, 0.0, 0.0, effective_opacity],
        };

        self.items.push(RenderQueueItem {
            layer: RenderLayer::GroundDecals,
            footprint_y: transform.ground_y(),
            instance,
        });
    }

    /// Sort all queued sprites deterministically:
    /// Primary key: RenderLayer (Floor -> Decals -> GroundEntities -> Airborne -> Foreground -> UI)
    /// Secondary key: Footprint Y coordinate (back-to-front ground Y sorting)
    pub fn build_sorted_instances(&mut self) -> Vec<SpriteInstanceRaw> {
        self.items.sort_by(|a, b| {
            a.layer.cmp(&b.layer).then_with(|| {
                a.footprint_y
                    .partial_cmp(&b.footprint_y)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });

        self.items.iter().map(|item| item.instance).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use restgine_ecs::Transform25D;

    #[test]
    fn test_isaac_y_sorting() {
        let mut batch = IsaacRenderBatch::new();

        let t_front = Transform25D::new(50.0, 100.0, 0.0); // Footprint y = 100
        let t_back = Transform25D::new(50.0, 20.0, 0.0); // Footprint y = 20

        let s1 = Sprite {
            layer: RenderLayer::GroundEntities,
            ..Default::default()
        };

        batch.submit_sprite(&t_front, &s1);
        batch.submit_sprite(&t_back, &s1);

        let instances = batch.build_sorted_instances();
        // The one with smaller y (back) must come first so front renders on top!
        assert_eq!(instances[0].position[1], 4.0); // 20 - 16 = 4
        assert_eq!(instances[1].position[1], 84.0); // 100 - 16 = 84
    }
}
