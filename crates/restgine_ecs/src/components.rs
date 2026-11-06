use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// 2.5D position in space (inspired by The Binding of Isaac).
/// - `x, y`: Ground plane coordinates.
/// - `z`: Height above ground (for airborne units, jumping, or projectile arcs).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform25D {
    pub position: Vec3,
    pub previous_position: Vec3,
}

impl Transform25D {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        let pos = Vec3::new(x, y, z);
        Self {
            position: pos,
            previous_position: pos,
        }
    }

    /// Screen position projection: screen_x = x, screen_y = y - z.
    pub fn screen_coords(&self) -> Vec2 {
        Vec2::new(self.position.x, self.position.y - self.position.z)
    }

    /// Ground footprint coordinate used for Depth / Y-Sorting: y.
    pub fn ground_y(&self) -> f32 {
        self.position.y
    }
}

/// 2.5D velocity vector.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Velocity25D {
    pub velocity: Vec3,
}

impl Velocity25D {
    pub fn new(vx: f32, vy: f32, vz: f32) -> Self {
        Self {
            velocity: Vec3::new(vx, vy, vz),
        }
    }

    pub fn zero() -> Self {
        Self {
            velocity: Vec3::ZERO,
        }
    }
}

/// Physical footprint shape on the ground plane (x, y) with a vertical height boundary (z).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FootprintShape {
    Circle { radius: f32 },
    AABB { half_extents: Vec2 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Footprint {
    pub shape: FootprintShape,
    /// Vertical height of the entity along the Z axis.
    /// A projectile at z=10 can fly over an obstacle with height=8, but hits one with height=20.
    pub z_height: f32,
    pub is_solid: bool,
    pub is_trigger: bool,
}

impl Footprint {
    pub fn circle(radius: f32, z_height: f32) -> Self {
        Self {
            shape: FootprintShape::Circle { radius },
            z_height,
            is_solid: true,
            is_trigger: false,
        }
    }

    pub fn aabb(half_width: f32, half_height: f32, z_height: f32) -> Self {
        Self {
            shape: FootprintShape::AABB {
                half_extents: Vec2::new(half_width, half_height),
            },
            z_height,
            is_solid: true,
            is_trigger: false,
        }
    }
}

/// Discrete rendering layers matching The Binding of Isaac architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RenderLayer {
    /// Room floor tiles and static base walls.
    Floor = 0,
    /// Creep, stains, blood, and ground shadows.
    GroundDecals = 1,
    /// Dynamic entities sorted dynamically by Footprint Y (Player, Enemies, Rocks, Pickups).
    GroundEntities = 2,
    /// Airborne entities, flies, tears, and high jumping bosses.
    Airborne = 3,
    /// Overhanging door arches, roofs, and foreground canopies.
    Foreground = 4,
    /// Screen-space user interface (Hearts, Keys, Minimap).
    UserInterface = 5,
}

/// 2D Sprite visual representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sprite {
    pub texture_id: String,
    pub uv_rect: [f32; 4], // [u_min, v_min, u_max, v_max]
    pub size: Vec2,
    /// Anchor offset from ground pivot point to top-left of sprite.
    pub anchor_offset: Vec2,
    pub flip_x: bool,
    pub flip_y: bool,
    pub color_tint: [f32; 4],
    pub layer: RenderLayer,
}

impl Default for Sprite {
    fn default() -> Self {
        Self {
            texture_id: String::new(),
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            size: Vec2::new(16.0, 16.0),
            anchor_offset: Vec2::new(-8.0, -16.0), // Centered on ground, sprite extends upward
            flip_x: false,
            flip_y: false,
            color_tint: [1.0, 1.0, 1.0, 1.0],
            layer: RenderLayer::GroundEntities,
        }
    }
}

/// Ground shadow component that projects onto Layer 1 (GroundDecals).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GroundShadow {
    pub base_radius: f32,
    pub opacity: f32,
}

impl Default for GroundShadow {
    fn default() -> Self {
        Self {
            base_radius: 8.0,
            opacity: 0.5,
        }
    }
}
