use serde::{Deserialize, Serialize};

/// Global configuration for a Restgine game, usually loaded from `game.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    pub title: String,
    pub version: String,
    pub steam_app_id: u32,
    pub window: WindowConfig,
    pub viewport: ViewportConfig,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            title: "Restgine Game".to_string(),
            version: "0.1.0".to_string(),
            steam_app_id: 480, // Default to Spacewar for testing
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
            vsync: true,
        }
    }
}

/// Responsive pixel art viewport settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportConfig {
    /// Baseline target vertical resolution for pixel art integer scaling (e.g., 270p or 360p).
    pub native_height: u32,
    /// Minimum allowed integer scale factor.
    pub min_scale: u32,
    /// Fixed physics update frequency in Hz (typically 60 Hz).
    pub target_fps: u32,
}

impl Default for ViewportConfig {
    fn default() -> Self {
        Self {
            native_height: 270,
            min_scale: 1,
            target_fps: 60,
        }
    }
}

impl GameConfig {
    /// Parse configuration from a TOML string.
    pub fn from_toml(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }

    /// Serialize configuration to a TOML string.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}
