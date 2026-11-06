pub mod config;
pub mod input;
pub mod scratchpad;
pub mod time;
pub mod transform3d;

pub use config::{GameConfig, RenderDimension, ViewportConfig, WindowConfig};
pub use input::InputState;
pub use scratchpad::{Scratchpad, ThreadScratchpad};
pub use time::GameClock;
pub use transform3d::Transform3D;

/// High-level lifecycle interface for a Restgine application.
pub trait Application {
    fn on_init(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn on_fixed_update(&mut self, dt: f32);
    fn on_update(&mut self, dt: f32);
    fn on_render(&mut self, alpha: f32);
    fn on_shutdown(&mut self);
}
