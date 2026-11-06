use glam::{Vec2, Vec3, Vec3A};
pub use restgine_core::Transform3D;
use serde::{Deserialize, Serialize};

/// Interned 32-bit generational asset identifier eliminating heap String allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId(pub u32);

impl AssetId {
    pub const INVALID: Self = Self(0);
    pub const DEFAULT_WHITE: Self = Self(1);
    pub const DEFAULT_CUBE: Self = Self(2);
}

impl Default for AssetId {
    fn default() -> Self {
        Self::DEFAULT_WHITE
    }
}

/// Backward compatibility alias for 2.5D projects.
pub type Transform25D = Transform3D;

/// 3D/2.5D velocity vector.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Velocity3D {
    pub linear: Vec3A,
    pub angular: Vec3,
}

impl Velocity3D {
    pub fn new(vx: f32, vy: f32, vz: f32) -> Self {
        Self {
            linear: Vec3A::new(vx, vy, vz),
            angular: Vec3::ZERO,
        }
    }

    pub fn zero() -> Self {
        Self {
            linear: Vec3A::ZERO,
            angular: Vec3::ZERO,
        }
    }
}

pub type Velocity25D = Velocity3D;

/// Physical footprint shape on the ground plane (x, y) with a vertical height boundary (z).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FootprintShape {
    Circle { radius: f32 },
    AABB { half_extents: Vec2 },
    Sphere3D { radius: f32 },
    Box3D { half_extents: Vec3 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Footprint {
    pub shape: FootprintShape,
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

    pub fn sphere(radius: f32) -> Self {
        Self {
            shape: FootprintShape::Sphere3D { radius },
            z_height: radius * 2.0,
            is_solid: true,
            is_trigger: false,
        }
    }

    pub fn box3d(hx: f32, hy: f32, hz: f32) -> Self {
        Self {
            shape: FootprintShape::Box3D {
                half_extents: Vec3::new(hx, hy, hz),
            },
            z_height: hz * 2.0,
            is_solid: true,
            is_trigger: false,
        }
    }
}

/// Discrete rendering layers matching The Binding of Isaac architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RenderLayer {
    Floor = 0,
    GroundDecals = 1,
    GroundEntities = 2,
    Airborne = 3,
    Foreground = 4,
    UserInterface = 5,
}

/// 2D Sprite visual representation using compact 32-bit AssetId and packed colors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sprite {
    pub asset: AssetId,
    pub uv_rect: [f32; 4],
    pub size: Vec2,
    pub anchor_offset: Vec2,
    pub flip_x: bool,
    pub flip_y: bool,
    pub color_tint: [f32; 4],
    pub layer: RenderLayer,
}

impl Default for Sprite {
    fn default() -> Self {
        Self {
            asset: AssetId::DEFAULT_WHITE,
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            size: Vec2::new(16.0, 16.0),
            anchor_offset: Vec2::new(-8.0, -16.0),
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

/// 3D Meshlet cluster component for hardware GPU culling and Visibility Buffer rendering.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MeshletCluster {
    pub mesh_asset: AssetId,
    pub cluster_offset: u32,
    pub cluster_count: u32,
    pub bounding_sphere: [f32; 4], // [center_x, center_y, center_z, radius]
}

impl Default for MeshletCluster {
    fn default() -> Self {
        Self {
            mesh_asset: AssetId::DEFAULT_CUBE,
            cluster_offset: 0,
            cluster_count: 1,
            bounding_sphere: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

/// XPBD (Extended Position-Based Dynamics) rigid body component.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RigidBodyXPBD {
    pub mass: f32,
    pub inv_mass: f32,
    pub restitution: f32,
    pub friction: f32,
    pub is_kinematic: bool,
}

impl RigidBodyXPBD {
    pub fn dynamic(mass: f32) -> Self {
        let m = mass.max(0.001);
        Self {
            mass: m,
            inv_mass: 1.0 / m,
            restitution: 0.3,
            friction: 0.5,
            is_kinematic: false,
        }
    }

    pub fn kinematic() -> Self {
        Self {
            mass: 0.0,
            inv_mass: 0.0,
            restitution: 0.0,
            friction: 0.5,
            is_kinematic: true,
        }
    }
}
