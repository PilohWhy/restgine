pub mod components;

pub use components::{
    AssetId, Footprint, FootprintShape, GroundShadow, MeshletCluster, RenderLayer, RigidBodyXPBD,
    Sprite, Transform25D, Transform3D, Velocity25D, Velocity3D,
};
pub use hecs::{Entity, World};

/// Helper to create and initialize a clean Restgine ECS world.
pub fn create_world() -> World {
    World::new()
}
