use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RenderDimension {
    Dimensional2D,
    Isometric25D,
    #[default]
    Full3D,
}

/// Global configuration for a Restgine game, loaded from `game.toml` or constructed in code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    pub title: String,
    pub version: String,
    pub steam_app_id: u32,
    pub dimension: RenderDimension,
    pub window: WindowConfig,
    pub viewport: ViewportConfig,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            title: "Restgine Game".to_string(),
            version: "0.2.0".to_string(),
            steam_app_id: 480,
            dimension: RenderDimension::Full3D,
            window: WindowConfig::default(),
            viewport: ViewportConfig::default(),
        }
    }
}

/// Window display settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub resizable: bool,
    pub vsync: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fullscreen: false,
            resizable: true,
            vsync: false, // Default to uncapped for extreme benchmark throughput
        }
    }
}

/// Viewport and benchmark frame pacing settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportConfig {
    pub native_height: u32,
    pub min_scale: u32,
    /// Fixed physics update frequency in Hz (typically 60 Hz).
    pub target_physics_hz: u32,
    /// Whether rendering runs uncapped for maximum benchmark framerates.
    pub uncapped_framerate: bool,
}

impl Default for ViewportConfig {
    fn default() -> Self {
        Self {
            native_height: 720,
            min_scale: 1,
            target_physics_hz: 60,
            uncapped_framerate: true,
        }
    }
}

impl GameConfig {
    pub fn from_toml(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }

    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}
