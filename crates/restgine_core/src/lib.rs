pub mod config;
pub mod input;
pub mod time;

pub use config::{GameConfig, ViewportConfig, WindowConfig};
pub use input::InputState;
pub use time::GameClock;

/// High-level lifecycle interface for a Restgine application.
pub trait Application {
    fn on_init(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn on_fixed_update(&mut self, dt: f32);
    fn on_update(&mut self, dt: f32);
    fn on_render(&mut self, alpha: f32);
    fn on_shutdown(&mut self);
}
