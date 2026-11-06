pub mod components;

pub use components::{
    Footprint, FootprintShape, GroundShadow, RenderLayer, Sprite, Transform25D, Velocity25D,
};
pub use hecs::{Entity, World};

/// Helper to create and initialize a clean Restgine ECS world.
pub fn create_world() -> World {
    World::new()
}
