use restgine_render::SpriteInstanceRaw;
use serde::{Deserialize, Serialize};
use taffy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityTier {
    Low = 0,
    Medium = 1,
    High = 2,
    Epic = 3,
}

impl QualityTier {
    pub const ALL: [QualityTier; 4] = [
        QualityTier::Low,
        QualityTier::Medium,
        QualityTier::High,
        QualityTier::Epic,
    ];

    pub fn name(self) -> &'static str {
        match self {
            QualityTier::Low => "LOW",
            QualityTier::Medium => "MED",
            QualityTier::High => "HIGH",
            QualityTier::Epic => "EPIC",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSettings {
    pub gamma: f32,       // 0.5 to 1.5 (50% to 150%)
    pub fov: f32,         // 70.0 to 110.0 degrees
    pub motion_blur: f32, // 0.0 to 1.0 (0% to 100%)
    pub global_illumination: QualityTier,
    pub shadows: QualityTier,
    pub textures: QualityTier,
    pub post_processing: QualityTier,
    pub effects: QualityTier,
    pub view_distance: QualityTier,
    pub foliage: QualityTier,
    pub shading: QualityTier,
    pub antialiasing: QualityTier,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            gamma: 1.0,
            fov: 90.0,
            motion_blur: 0.5,
            global_illumination: QualityTier::High,
            shadows: QualityTier::High,
            textures: QualityTier::High,
            post_processing: QualityTier::High,
            effects: QualityTier::High,
            view_distance: QualityTier::High,
            foliage: QualityTier::High,
            shading: QualityTier::High,
            antialiasing: QualityTier::High,
        }
    }
}

impl GameSettings {
    pub fn load_or_default() -> Self {
        if let Ok(contents) = std::fs::read_to_string("settings.json") {
            if let Ok(settings) = serde_json::from_str::<GameSettings>(&contents) {
                return settings;
            }
        }
        let default_settings = Self::default();
        let _ = default_settings.save();
        default_settings
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write("settings.json", json)
    }

    pub fn view_distance_world(&self) -> f32 {
        match self.view_distance {
            QualityTier::Low => 80.0,
            QualityTier::Medium => 140.0,
            QualityTier::High => 220.0,
            QualityTier::Epic => 350.0,
        }
    }

    pub fn quality_tiers_u32(&self) -> [u32; 4] {
        [
            self.shadows as u32,
            self.global_illumination as u32,
            self.shading as u32,
            self.post_processing as u32,
        ]
    }

    pub fn quality_extra_u32(&self) -> [u32; 4] {
        [
            self.effects as u32,
            self.foliage as u32,
            self.antialiasing as u32,
            self.textures as u32,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderId {
    Gamma,
    Fov,
    MotionBlur,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingId {
    GlobalIllumination,
    Shadows,
    Textures,
    PostProcessing,
    Effects,
    ViewDistance,
    Foliage,
    Shading,
    AntiAliasing,
}

impl SettingId {
    pub const ALL: [SettingId; 9] = [
        SettingId::GlobalIllumination,
        SettingId::Shadows,
        SettingId::Textures,
        SettingId::PostProcessing,
        SettingId::Effects,
        SettingId::ViewDistance,
        SettingId::Foliage,
        SettingId::Shading,
        SettingId::AntiAliasing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingId::GlobalIllumination => "GLOBAL ILLUMINATION",
            SettingId::Shadows => "SHADOWS",
            SettingId::Textures => "TEXTURES",
            SettingId::PostProcessing => "POST PROCESSING",
            SettingId::Effects => "EFFECTS",
            SettingId::ViewDistance => "VIEW DISTANCE",
            SettingId::Foliage => "FOLIAGE",
            SettingId::Shading => "SHADING",
            SettingId::AntiAliasing => "ANTIALIASING",
        }
    }

    pub fn get_tier(self, s: &GameSettings) -> QualityTier {
        match self {
            SettingId::GlobalIllumination => s.global_illumination,
            SettingId::Shadows => s.shadows,
            SettingId::Textures => s.textures,
            SettingId::PostProcessing => s.post_processing,
            SettingId::Effects => s.effects,
            SettingId::ViewDistance => s.view_distance,
            SettingId::Foliage => s.foliage,
            SettingId::Shading => s.shading,
            SettingId::AntiAliasing => s.antialiasing,
        }
    }

    pub fn set_tier(self, s: &mut GameSettings, tier: QualityTier) {
        match self {
            SettingId::GlobalIllumination => s.global_illumination = tier,
            SettingId::Shadows => s.shadows = tier,
            SettingId::Textures => s.textures = tier,
            SettingId::PostProcessing => s.post_processing = tier,
            SettingId::Effects => s.effects = tier,
            SettingId::ViewDistance => s.view_distance = tier,
            SettingId::Foliage => s.foliage = tier,
            SettingId::Shading => s.shading = tier,
            SettingId::AntiAliasing => s.antialiasing = tier,
        }
    }
}

pub struct SliderHitArea {
    pub id: SliderId,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub struct TierButtonHitArea {
    pub setting: SettingId,
    pub tier: QualityTier,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub struct TabHitArea {
    pub tab_index: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub struct SettingsUI {
    pub is_open: bool,
    pub active_tab: usize, // 0 = Graphics, 1 = Controls, 2 = Gameplay
    pub dragging_slider: Option<SliderId>,
    pub slider_hit_areas: Vec<SliderHitArea>,
    pub tier_button_hit_areas: Vec<TierButtonHitArea>,
    pub tab_hit_areas: Vec<TabHitArea>,
}

impl Default for SettingsUI {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsUI {
    pub fn new() -> Self {
        Self {
            is_open: false,
            active_tab: 0,
            dragging_slider: None,
            slider_hit_areas: Vec::new(),
            tier_button_hit_areas: Vec::new(),
            tab_hit_areas: Vec::new(),
        }
    }

    pub fn toggle(&mut self) -> bool {
        self.is_open = !self.is_open;
        if !self.is_open {
            self.dragging_slider = None;
        }
        self.is_open
    }

    pub fn handle_mouse_down(&mut self, x: f32, y: f32, settings: &mut GameSettings) -> bool {
        if !self.is_open {
            return false;
        }

        // Check tabs
        for tab in &self.tab_hit_areas {
            if x >= tab.x && x <= tab.x + tab.width && y >= tab.y && y <= tab.y + tab.height {
                self.active_tab = tab.tab_index;
                return true;
            }
        }

        if self.active_tab == 0 {
            // Check sliders
            for slider in &self.slider_hit_areas {
                if x >= slider.x - 10.0
                    && x <= slider.x + slider.width + 10.0
                    && y >= slider.y - 8.0
                    && y <= slider.y + slider.height + 8.0
                {
                    self.dragging_slider = Some(slider.id);
                    self.update_slider_value(slider.id, x, slider.x, slider.width, settings);
                    let _ = settings.save();
                    return true;
                }
            }

            // Check tier buttons
            for btn in &self.tier_button_hit_areas {
                if x >= btn.x && x <= btn.x + btn.width && y >= btn.y && y <= btn.y + btn.height {
                    btn.setting.set_tier(settings, btn.tier);
                    let _ = settings.save();
                    return true;
                }
            }
        }

        false
    }

    pub fn handle_mouse_move(&mut self, x: f32, settings: &mut GameSettings) -> bool {
        if !self.is_open {
            return false;
        }

        if let Some(slider_id) = self.dragging_slider {
            if let Some(slider) = self.slider_hit_areas.iter().find(|s| s.id == slider_id) {
                self.update_slider_value(slider.id, x, slider.x, slider.width, settings);
                let _ = settings.save();
                return true;
            }
        }

        false
    }

    pub fn handle_mouse_up(&mut self) {
        self.dragging_slider = None;
    }

    fn update_slider_value(
        &self,
        id: SliderId,
        mouse_x: f32,
        slider_x: f32,
        slider_w: f32,
        settings: &mut GameSettings,
    ) {
        let t = ((mouse_x - slider_x) / slider_w).clamp(0.0, 1.0);
        match id {
            SliderId::Gamma => {
                // 50% to 150% (0.5 to 1.5)
                settings.gamma = (0.5 + t * 1.0).clamp(0.5, 1.5);
            }
            SliderId::Fov => {
                // 70 to 110 degrees
                settings.fov = (70.0 + t * 40.0).clamp(70.0, 110.0);
            }
            SliderId::MotionBlur => {
                // 0% to 100% (0.0 to 1.0)
                settings.motion_blur = t.clamp(0.0, 1.0);
            }
        }
    }

    /// Build layout using Taffy and generate sprite quads.
    pub fn build_instances(
        &mut self,
        screen_w: f32,
        screen_h: f32,
        settings: &GameSettings,
        instances: &mut Vec<SpriteInstanceRaw>,
    ) {
        if !self.is_open {
            return;
        }

        self.slider_hit_areas.clear();
        self.tier_button_hit_areas.clear();
        self.tab_hit_areas.clear();

        // 1. Fullscreen dark backdrop overlay
        instances.push(SpriteInstanceRaw {
            position: [0.0, 0.0],
            size: [screen_w, screen_h],
            uv_min: [0.0, 0.0],
            uv_max: [1.0 / 128.0, 1.0 / 128.0],
            color_tint: [0.02, 0.03, 0.06, 0.75], // Translucent backdrop
        });

        // 2. Compute Taffy CSS Flexbox layout for centered settings card
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let card_w = 900.0_f32.min(screen_w - 40.0);
        let card_h = 640.0_f32.min(screen_h - 40.0);

        let card_node = taffy
            .new_leaf(Style {
                size: Size {
                    width: length(card_w),
                    height: length(card_h),
                },
                ..Default::default()
            })
            .unwrap();

        taffy.compute_layout(card_node, Size::MAX_CONTENT).unwrap();

        let card_x = (screen_w - card_w) * 0.5;
        let card_y = (screen_h - card_h) * 0.5;

        // Draw Card Background and Border
        instances.push(SpriteInstanceRaw {
            position: [card_x, card_y],
            size: [card_w, card_h],
            uv_min: [0.0, 0.0],
            uv_max: [1.0 / 128.0, 1.0 / 128.0],
            color_tint: [0.05, 0.08, 0.13, 0.96], // Slate glass
        });

        // Header Top Emerald Strip
        instances.push(SpriteInstanceRaw {
            position: [card_x, card_y + card_h - 4.0],
            size: [card_w, 4.0],
            uv_min: [0.0, 0.0],
            uv_max: [1.0 / 128.0, 1.0 / 128.0],
            color_tint: [0.06, 0.85, 0.55, 1.0], // Emerald accent
        });

        // Card Outer Border Lines
        let border_color = [0.12, 0.18, 0.28, 1.0];
        // Bottom border
        instances.push(SpriteInstanceRaw {
            position: [card_x, card_y],
            size: [card_w, 2.0],
            uv_min: [0.0, 0.0],
            uv_max: [1.0 / 128.0, 1.0 / 128.0],
            color_tint: border_color,
        });
        // Left border
        instances.push(SpriteInstanceRaw {
            position: [card_x, card_y],
            size: [2.0, card_h],
            uv_min: [0.0, 0.0],
            uv_max: [1.0 / 128.0, 1.0 / 128.0],
            color_tint: border_color,
        });
        // Right border
        instances.push(SpriteInstanceRaw {
            position: [card_x + card_w - 2.0, card_y],
            size: [2.0, card_h],
            uv_min: [0.0, 0.0],
            uv_max: [1.0 / 128.0, 1.0 / 128.0],
            color_tint: border_color,
        });

        // Title and Subtitle Text
        render_text(
            "RESTGINE ENGINE SETTINGS",
            card_x + 24.0,
            card_y + card_h - 32.0,
            12.0,
            12.0,
            [0.95, 0.98, 1.0, 1.0],
            instances,
        );
        render_text(
            "PRESS [ESC] TO RESUME - LIVE SHADER MODES",
            card_x + card_w - 380.0,
            card_y + card_h - 30.0,
            8.0,
            8.0,
            [0.55, 0.65, 0.78, 1.0],
            instances,
        );

        // Divider below header
        instances.push(SpriteInstanceRaw {
            position: [card_x + 20.0, card_y + card_h - 44.0],
            size: [card_w - 40.0, 1.0],
            uv_min: [0.0, 0.0],
            uv_max: [1.0 / 128.0, 1.0 / 128.0],
            color_tint: [0.15, 0.22, 0.32, 1.0],
        });

        // 3. Tab Buttons Row
        let tabs = ["GRAPHICS", "CONTROLS", "GAMEPLAY"];
        let tab_w = 120.0;
        let tab_h = 28.0;
        let tabs_start_y = card_y + card_h - 80.0;

        for (i, tab_name) in tabs.iter().enumerate() {
            let tab_x = card_x + 24.0 + (i as f32) * (tab_w + 12.0);
            let is_active = self.active_tab == i;

            self.tab_hit_areas.push(TabHitArea {
                tab_index: i,
                x: tab_x,
                y: tabs_start_y,
                width: tab_w,
                height: tab_h,
            });

            let bg_color = if is_active {
                [0.06, 0.72, 0.50, 0.30]
            } else {
                [0.08, 0.12, 0.18, 0.80]
            };
            instances.push(SpriteInstanceRaw {
                position: [tab_x, tabs_start_y],
                size: [tab_w, tab_h],
                uv_min: [0.0, 0.0],
                uv_max: [1.0 / 128.0, 1.0 / 128.0],
                color_tint: bg_color,
            });

            if is_active {
                instances.push(SpriteInstanceRaw {
                    position: [tab_x, tabs_start_y],
                    size: [tab_w, 2.0],
                    uv_min: [0.0, 0.0],
                    uv_max: [1.0 / 128.0, 1.0 / 128.0],
                    color_tint: [0.06, 0.85, 0.55, 1.0],
                });
            }

            let text_color = if is_active {
                [0.06, 0.85, 0.55, 1.0]
            } else {
                [0.60, 0.70, 0.82, 1.0]
            };
            let text_x = tab_x + (tab_w - (tab_name.len() as f32 * 8.0)) * 0.5;
            render_text(
                tab_name,
                text_x,
                tabs_start_y + 8.0,
                8.0,
                8.0,
                text_color,
                instances,
            );
        }

        // 4. Tab Body Content
        match self.active_tab {
            0 => {
                // --- GRAPHICS TAB ---
                let mut current_y = tabs_start_y - 36.0;

                // Section 1: Sliders
                let slider_track_w = 260.0;
                let slider_track_h = 8.0;

                // Slider 1: GAMMA
                self.render_slider(
                    SliderId::Gamma,
                    "GAMMA",
                    &format!("{:.0}%", settings.gamma * 100.0),
                    (settings.gamma - 0.5) / 1.0,
                    card_x + 24.0,
                    current_y,
                    slider_track_w,
                    slider_track_h,
                    instances,
                );
                current_y -= 32.0;

                // Slider 2: FOV
                self.render_slider(
                    SliderId::Fov,
                    "FOV",
                    &format!("{:.0}", settings.fov),
                    (settings.fov - 70.0) / 40.0,
                    card_x + 24.0,
                    current_y,
                    slider_track_w,
                    slider_track_h,
                    instances,
                );
                current_y -= 32.0;

                // Slider 3: MOTION BLUR
                self.render_slider(
                    SliderId::MotionBlur,
                    "MOTION BLUR",
                    &format!("{:.0}%", settings.motion_blur * 100.0),
                    settings.motion_blur,
                    card_x + 24.0,
                    current_y,
                    slider_track_w,
                    slider_track_h,
                    instances,
                );
                current_y -= 36.0;

                // Sub-divider
                instances.push(SpriteInstanceRaw {
                    position: [card_x + 24.0, current_y + 12.0],
                    size: [card_w - 48.0, 1.0],
                    uv_min: [0.0, 0.0],
                    uv_max: [1.0 / 128.0, 1.0 / 128.0],
                    color_tint: [0.12, 0.17, 0.25, 1.0],
                });

                // Section 2: 9 Quality Tier Selectors
                for setting in SettingId::ALL {
                    self.render_tier_row(
                        setting,
                        setting.label(),
                        setting.get_tier(settings),
                        card_x + 24.0,
                        current_y,
                        card_w - 48.0,
                        instances,
                    );
                    current_y -= 28.0;
                }
            }
            1 => {
                // --- CONTROLS TAB ---
                let mut current_y = tabs_start_y - 40.0;
                let bindings = [
                    ("W / A / S / D", "FIRST-PERSON WALKAROUND"),
                    ("MOUSE LOOK", "FREE 360 DEGREE MOUSE CAMERA"),
                    ("SPACE", "JUMP (DETERMINISTIC 60HZ IMPULSE)"),
                    ("LEFT SHIFT", "SPRINT (1.6X SPEED MULTIPLIER)"),
                    ("ESCAPE", "TOGGLE SETTINGS / UNLOCK MOUSE"),
                ];

                for (key, desc) in bindings {
                    // Key badge
                    instances.push(SpriteInstanceRaw {
                        position: [card_x + 28.0, current_y - 4.0],
                        size: [160.0, 24.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0 / 128.0, 1.0 / 128.0],
                        color_tint: [0.10, 0.15, 0.22, 1.0],
                    });
                    render_text(
                        key,
                        card_x + 36.0,
                        current_y + 4.0,
                        8.0,
                        8.0,
                        [0.06, 0.85, 0.55, 1.0],
                        instances,
                    );
                    render_text(
                        desc,
                        card_x + 210.0,
                        current_y + 4.0,
                        8.0,
                        8.0,
                        [0.85, 0.90, 0.96, 1.0],
                        instances,
                    );
                    current_y -= 36.0;
                }
            }
            _ => {
                // --- GAMEPLAY TAB ---
                let mut current_y = tabs_start_y - 40.0;
                let info = [
                    ("COORDINATE SPACE", "2D/3D CONTINUUM WITH REVERSED-Z DEPTH"),
                    (
                        "PHYSICS FREQUENCY",
                        "60HZ FIXED TIMESTEP WITH SUBFRAME LERP",
                    ),
                    ("ENTITY COUNT", "0 DYNAMIC BACKGROUND ENTITIES (ZERO GC)"),
                    ("GPU ARCHITECTURE", "WGPU VULKAN/DX12 BRANCHLESS SIMD"),
                    ("PERSISTENCE", "SAVED AUTOMATICALLY TO SETTINGS.JSON"),
                ];

                for (label, val) in info {
                    render_text(
                        label,
                        card_x + 28.0,
                        current_y,
                        8.0,
                        8.0,
                        [0.06, 0.85, 0.55, 1.0],
                        instances,
                    );
                    render_text(
                        val,
                        card_x + 28.0,
                        current_y - 14.0,
                        8.0,
                        8.0,
                        [0.82, 0.88, 0.95, 1.0],
                        instances,
                    );
                    current_y -= 42.0;
                }
            }
        }

        // 5. Footer status
        instances.push(SpriteInstanceRaw {
            position: [card_x + 20.0, card_y + 36.0],
            size: [card_w - 40.0, 1.0],
            uv_min: [0.0, 0.0],
            uv_max: [1.0 / 128.0, 1.0 / 128.0],
            color_tint: [0.12, 0.17, 0.25, 1.0],
        });
        render_text(
            "RESTGINE 3D - REAL-TIME CONFIGURATION READY",
            card_x + 24.0,
            card_y + 16.0,
            8.0,
            8.0,
            [0.45, 0.55, 0.68, 1.0],
            instances,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn render_slider(
        &mut self,
        id: SliderId,
        label: &str,
        value_str: &str,
        fraction: f32,
        x: f32,
        y: f32,
        track_w: f32,
        track_h: f32,
        instances: &mut Vec<SpriteInstanceRaw>,
    ) {
        // Label on left
        render_text(label, x, y, 8.0, 8.0, [0.85, 0.90, 0.96, 1.0], instances);

        let track_x = x + 240.0;
        let track_y = y - 2.0;

        self.slider_hit_areas.push(SliderHitArea {
            id,
            x: track_x,
            y: track_y,
            width: track_w,
            height: track_h,
        });

        // Track background
        instances.push(SpriteInstanceRaw {
            position: [track_x, track_y],
            size: [track_w, track_h],
            uv_min: [0.0, 0.0],
            uv_max: [1.0 / 128.0, 1.0 / 128.0],
            color_tint: [0.10, 0.14, 0.20, 1.0],
        });

        // Track fill
        let fill_w = track_w * fraction.clamp(0.0, 1.0);
        if fill_w > 0.0 {
            instances.push(SpriteInstanceRaw {
                position: [track_x, track_y],
                size: [fill_w, track_h],
                uv_min: [0.0, 0.0],
                uv_max: [1.0 / 128.0, 1.0 / 128.0],
                color_tint: [0.06, 0.85, 0.55, 0.9],
            });
        }

        // Thumb handle
        let thumb_x = (track_x + fill_w - 6.0).clamp(track_x - 4.0, track_x + track_w - 8.0);
        instances.push(SpriteInstanceRaw {
            position: [thumb_x, track_y - 4.0],
            size: [12.0, track_h + 8.0],
            uv_min: [0.0, 0.0],
            uv_max: [1.0 / 128.0, 1.0 / 128.0],
            color_tint: [1.0, 1.0, 1.0, 1.0],
        });

        // Value on right
        render_text(
            value_str,
            track_x + track_w + 20.0,
            y,
            8.0,
            8.0,
            [0.06, 0.85, 0.55, 1.0],
            instances,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn render_tier_row(
        &mut self,
        setting: SettingId,
        label: &str,
        active_tier: QualityTier,
        x: f32,
        y: f32,
        total_w: f32,
        instances: &mut Vec<SpriteInstanceRaw>,
    ) {
        // Label
        render_text(label, x, y, 8.0, 8.0, [0.82, 0.88, 0.95, 1.0], instances);

        // 4 Radio Buttons on the right
        let btn_w = 64.0;
        let btn_h = 20.0;
        let gap = 8.0;
        let start_x = x + total_w - (4.0 * btn_w + 3.0 * gap) - 10.0;

        for (i, &tier) in QualityTier::ALL.iter().enumerate() {
            let btn_x = start_x + (i as f32) * (btn_w + gap);
            let is_selected = tier == active_tier;

            self.tier_button_hit_areas.push(TierButtonHitArea {
                setting,
                tier,
                x: btn_x,
                y: y - 4.0,
                width: btn_w,
                height: btn_h,
            });

            let bg_color = if is_selected {
                [0.06, 0.85, 0.55, 0.9] // Selected Emerald
            } else {
                [0.10, 0.14, 0.20, 0.8] // Inactive Dark Slate
            };

            instances.push(SpriteInstanceRaw {
                position: [btn_x, y - 4.0],
                size: [btn_w, btn_h],
                uv_min: [0.0, 0.0],
                uv_max: [1.0 / 128.0, 1.0 / 128.0],
                color_tint: bg_color,
            });

            // Button label
            let text_color = if is_selected {
                [0.02, 0.05, 0.04, 1.0] // Dark text on bright emerald
            } else {
                [0.60, 0.68, 0.78, 1.0]
            };
            let text = tier.name();
            let text_x = btn_x + (btn_w - (text.len() as f32 * 8.0)) * 0.5;
            render_text(text, text_x, y + 2.0, 8.0, 8.0, text_color, instances);
        }
    }
}

// ----------------------------------------------------------------------------
// Crisp Bitmap Font Renderer (5x7 glyphs in 8x8 cells)
// ----------------------------------------------------------------------------

pub fn create_font_atlas_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &restgine_render::SpritePipeline,
) -> (wgpu::Texture, wgpu::BindGroup) {
    let mut data = vec![0u8; 128 * 128 * 4];

    // Pixel (0,0) is solid white so rectangles can sample it
    data[0] = 255;
    data[1] = 255;
    data[2] = 255;
    data[3] = 255;

    // Populate glyphs into 16x16 grid of 8x8 cells
    for code in 32u8..=126u8 {
        let glyph_idx = code - 32;
        let cell_x = (glyph_idx % 16) as usize * 8;
        let cell_y = (glyph_idx / 16) as usize * 8;

        let pattern = get_glyph_pattern(code);
        for (row, &row_bits) in pattern.iter().enumerate() {
            for col in 0..5 {
                if (row_bits & (1 << (4 - col))) != 0 {
                    let px = cell_x + col + 1;
                    let py = cell_y + row;
                    if px < 128 && py < 128 {
                        let idx = (py * 128 + px) * 4;
                        data[idx] = 255;
                        data[idx + 1] = 255;
                        data[idx + 2] = 255;
                        data[idx + 3] = 255;
                    }
                }
            }
        }
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("UI Font & Rect Atlas Texture"),
        size: wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &data,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(128 * 4),
            rows_per_image: Some(128),
        },
        wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
    );

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("UI Font Atlas Bind Group"),
        layout: &pipeline.texture_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    (texture, bind_group)
}

pub fn render_text(
    text: &str,
    start_x: f32,
    start_y: f32,
    char_w: f32,
    char_h: f32,
    color: [f32; 4],
    instances: &mut Vec<SpriteInstanceRaw>,
) {
    let mut cur_x = start_x;
    for ch in text.chars() {
        let code = (ch as u32).clamp(32, 126) as u8;
        if code > 32 {
            let glyph_idx = code - 32;
            let col = (glyph_idx % 16) as f32;
            let row = (glyph_idx / 16) as f32;

            // Flip Y so the top of the glyph texture (V=row/16.0) maps to the top of the quad
            let uv_min = [col / 16.0, (row + 1.0) / 16.0];
            let uv_max = [(col + 1.0) / 16.0, row / 16.0];

            instances.push(SpriteInstanceRaw {
                position: [cur_x, start_y],
                size: [char_w, char_h],
                uv_min,
                uv_max,
                color_tint: color,
            });
        }
        cur_x += char_w;
    }
}

fn get_glyph_pattern(code: u8) -> [u8; 7] {
    match code.to_ascii_uppercase() {
        b'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        b'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        b'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        b'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        b'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        b'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        b'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        b'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        b'M' => [0x11, 0x1B, 0x15, 0x11, 0x11, 0x11, 0x11],
        b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        b'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        b'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b'S' => [0x0E, 0x11, 0x10, 0x0E, 0x01, 0x11, 0x0E],
        b'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        b'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        b'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        b'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        b'0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        b'1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'2' => [0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        b'3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        b'4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        b'5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        b'6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        b'7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        b'9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        b'%' => [0x19, 0x19, 0x02, 0x04, 0x08, 0x13, 0x13],
        b':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        b'-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        b'.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        b'[' => [0x0E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0E],
        b']' => [0x0E, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0E],
        b'/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        b'<' => [0x02, 0x04, 0x08, 0x10, 0x08, 0x04, 0x02],
        b'>' => [0x08, 0x04, 0x02, 0x01, 0x02, 0x04, 0x08],
        b'(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        b')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        b'|' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'*' => [0x00, 0x04, 0x15, 0x0E, 0x15, 0x04, 0x00],
        b',' => [0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x08],
        b'_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        _ => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    }
}
