use std::io::{Read, Write};
use steamworks::{AppId, Client};

#[derive(Debug, thiserror::Error)]
pub enum SteamError {
    #[error("Failed to initialize Steamworks SDK: {0}")]
    InitFailed(String),
}

/// Native Steam integration managing Steamworks callbacks, Steam Cloud, and Achievements.
pub struct SteamContext {
    client: Option<Client>,
    app_id: u32,
    is_steam_deck: bool,
}

impl SteamContext {
    /// Initialize connection to the Steam client.
    /// If Steam is not running (e.g. during offline development or headless CI),
    /// enters graceful mock mode to keep developer flow unblocked.
    pub fn init(app_id: u32) -> Self {
        // Write steam_appid.txt if not present
        if !std::path::Path::new("steam_appid.txt").exists() {
            let _ = std::fs::write("steam_appid.txt", app_id.to_string());
        }

        match Client::init_app(AppId(app_id)) {
            Ok(client) => {
                let is_deck = client.utils().is_steam_running_on_steam_deck();
                log::info!(
                    "Steamworks SDK initialized successfully for AppID {} (Steam Deck: {})",
                    app_id,
                    is_deck
                );

                Self {
                    client: Some(client),
                    app_id,
                    is_steam_deck: is_deck,
                }
            }
            Err(e) => {
                log::warn!(
                    "Steam client not detected or initialization failed: {}. Running in local mock mode.",
                    e
                );
                Self {
                    client: None,
                    app_id,
                    is_steam_deck: false,
                }
            }
        }
    }

    /// Pump Steam callbacks. Call this once per frame in the main game loop.
    pub fn run_callbacks(&self) {
        if let Some(client) = &self.client {
            client.run_callbacks();
        }
    }

    /// Check if the game is actively running on a Steam Deck console.
    pub fn is_steam_deck(&self) -> bool {
        self.is_steam_deck
    }

    /// Unlock a Steam achievement and trigger the in-game Steam notification overlay.
    pub fn unlock_achievement(&self, name: &str) -> bool {
        if let Some(client) = &self.client {
            let stats = client.user_stats();
            let ach = stats.achievement(name);
            let _ = ach.set();
            let success = stats.store_stats();
            log::info!("Steam achievement '{}' unlock requested (result: {:?})", name, success);
            true
        } else {
            log::info!("[Mock Steam] Achievement unlocked: '{}'", name);
            true
        }
    }

    /// Save game state to Steam Cloud (ISteamRemoteStorage).
    pub fn save_cloud_file(&self, file_name: &str, data: &[u8]) -> Result<(), String> {
        if let Some(client) = &self.client {
            let storage = client.remote_storage();
            let file = storage.file(file_name);
            let mut writer = file.write();
            writer
                .write_all(data)
                .map_err(|e| format!("Steam Cloud write error: {:?}", e))?;
            log::info!("Successfully synced {} bytes to Steam Cloud: '{}'", data.len(), file_name);
            Ok(())
        } else {
            // Local fallback
            let _ = std::fs::create_dir_all("saves");
            let path = format!("saves/{}", file_name);
            std::fs::write(&path, data).map_err(|e| e.to_string())?;
            log::info!("[Mock Steam] Saved {} bytes to local storage: '{}'", data.len(), path);
            Ok(())
        }
    }

    /// Load game state from Steam Cloud.
    pub fn read_cloud_file(&self, file_name: &str) -> Result<Vec<u8>, String> {
        if let Some(client) = &self.client {
            let storage = client.remote_storage();
            let file = storage.file(file_name);
            if !file.exists() {
                return Err(format!("File '{}' does not exist in Steam Cloud", file_name));
            }
            let mut reader = file.read();
            let mut buf = Vec::new();
            reader
                .read_to_end(&mut buf)
                .map_err(|e| format!("Steam Cloud read error: {:?}", e))?;
            Ok(buf)
        } else {
            let path = format!("saves/{}", file_name);
            std::fs::read(&path).map_err(|e| e.to_string())
        }
    }

    pub fn app_id(&self) -> u32 {
        self.app_id
    }
}
