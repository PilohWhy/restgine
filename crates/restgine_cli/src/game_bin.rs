mod settings_ui;

use glam::{Mat4, Vec3, Vec3A};
use restgine_core::time::GameClock;
use restgine_core::ThreadScratchpad;
use restgine_render::foliage::{scatter_chunk_grass, FoliagePipeline, GrassInstanceRaw};
use restgine_render::mesh3d::CameraSettingsInput;
use restgine_render::terrain::{terrain_height, TerrainChunkInstance, TerrainPipeline};
use restgine_render::{
    Camera3D, Mesh3DPipeline, SkyPipeline, SpriteInstanceRaw, SpritePipeline, WindowContext,
};
use settings_ui::{create_font_atlas_texture, GameSettings, QualityTier, SettingsUI};
use std::time::Instant;
use winit::event::{DeviceEvent, ElementState, Event, KeyEvent, MouseButton, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::CursorGrabMode;

// Embedded Ground Textures (Poliigon Patchy Grass 2K)
const GROUND_ALBEDO_BYTES: &[u8] =
    include_bytes!("../../../assets/textures/ground/Poliigon_GrassPatchyGround_4585_BaseColor.jpg");
const GROUND_NORMAL_BYTES: &[u8] =
    include_bytes!("../../../assets/textures/ground/Poliigon_GrassPatchyGround_4585_Normal.png");
const GROUND_AO_ROUGH_BYTES: &[u8] =
    include_bytes!("../../../assets/textures/ground/ground_ao_roughness.jpg");

// Embedded 3D Grass Mesh Variants and Atlases
const GRASS_VARIANTS_BIN: &[u8] = include_bytes!("../../../assets/models/grass/grass_variants.bin");
const GRASS_ATLAS_ALBEDO_BYTES: &[u8] =
    include_bytes!("../../../assets/models/grass/grass_atlas_albedo.png");
const GRASS_ATLAS_NORMAL_BYTES: &[u8] =
    include_bytes!("../../../assets/models/grass/grass_atlas_normal.png");

/// Dynamic first-person flashlight with organic inertia, breathing sway, and walking bobbing.
struct FlashlightState {
    pub is_on: bool,
    pub current_pos: Vec3,
    pub current_dir: Vec3,
    pub bob_time: f32,
    pub sway_time: f32,
}

impl FlashlightState {
    fn new(spawn_pos: Vec3, initial_dir: Vec3) -> Self {
        Self {
            is_on: true,
            current_pos: spawn_pos,
            current_dir: initial_dir,
            bob_time: 0.0,
            sway_time: 0.0,
        }
    }

    fn update(
        &mut self,
        dt: f32,
        camera_eye: Vec3,
        camera_look_dir: Vec3,
        is_moving: bool,
        is_sprinting: bool,
    ) {
        self.sway_time += dt;

        let world_up = Vec3::Y;
        let right = camera_look_dir.cross(world_up).normalize_or_zero();
        let up = right.cross(camera_look_dir).normalize_or_zero();

        // 1. Walking / Running Bobbing
        if is_moving {
            let bob_speed = if is_sprinting { 12.0 } else { 8.5 };
            self.bob_time += dt * bob_speed;
        } else {
            self.bob_time += dt * 1.5;
        }

        let bob_h = if is_moving {
            (self.bob_time * 0.5).cos() * if is_sprinting { 0.035 } else { 0.02 }
        } else {
            0.0
        };
        let bob_v = if is_moving {
            self.bob_time.sin() * if is_sprinting { 0.045 } else { 0.025 }
        } else {
            0.0
        };

        // 2. Micro Hand Tremor & Natural Breathing Sway
        let t = self.sway_time;
        let breath_x = (t * 1.4).sin() * 0.007 + (t * 3.8).cos() * 0.0028;
        let breath_y = (t * 2.1).cos() * 0.008 + (t * 5.2).sin() * 0.0028;

        // Socket offset: held to the right, lower, and forward
        let target_pos =
            camera_eye + right * (0.20 + bob_h) + up * (-0.16 + bob_v) + camera_look_dir * 0.12;

        let target_dir = (camera_look_dir + right * breath_x + up * breath_y).normalize();

        // 3. Cinematic Lag & Tweening (inertia)
        let pos_lerp_speed = (16.0 * dt).min(1.0);
        self.current_pos = self.current_pos.lerp(target_pos, pos_lerp_speed);

        let dir_lerp_speed = (10.5 * dt).min(1.0);
        self.current_dir = self
            .current_dir
            .lerp(target_dir, dir_lerp_speed)
            .normalize();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,restgine=info,game=info"),
    )
    .init();

    // 1. Initialize Window with uncapped GPU surface
    let mut window_ctx = WindowContext::new(
        "Restgine 3D — Procedural Infinite Grassland | Vulkan Uncapped",
        1280,
        720,
    )?;

    let event_loop = window_ctx
        .event_loop
        .take()
        .ok_or("EventLoop already taken")?;
    let device = &window_ctx.device;
    let queue = &window_ctx.queue;
    let surface = &window_ctx.surface;
    let window = window_ctx.window.clone();
    let mut config = window_ctx.config;

    // Load persistent game settings from settings.json
    let mut settings = GameSettings::load_or_default();
    let mut settings_ui = SettingsUI::new();

    // Grab cursor for seamless first-person mouse look initially
    let mut cursor_locked = true;
    let _ = window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
    window.set_cursor_visible(false);

    // 2. Rendering Pipelines
    let mut mesh3d_pipeline =
        Mesh3DPipeline::new(device, config.format, config.width, config.height);

    let terrain_pipeline = TerrainPipeline::new(
        device,
        queue,
        config.format,
        Mesh3DPipeline::DEPTH_FORMAT,
        &mesh3d_pipeline.camera_bind_group_layout,
        GROUND_ALBEDO_BYTES,
        GROUND_NORMAL_BYTES,
        GROUND_AO_ROUGH_BYTES,
    )?;

    let foliage_pipeline = FoliagePipeline::new(
        device,
        queue,
        config.format,
        Mesh3DPipeline::DEPTH_FORMAT,
        &mesh3d_pipeline.camera_bind_group_layout,
        GRASS_VARIANTS_BIN,
        GRASS_ATLAS_ALBEDO_BYTES,
        GRASS_ATLAS_NORMAL_BYTES,
    )?;

    let sky_pipeline = SkyPipeline::new(
        device,
        config.format,
        Mesh3DPipeline::DEPTH_FORMAT,
        &mesh3d_pipeline.camera_bind_group_layout,
    );

    let hud_pipeline = SpritePipeline::new(device, config.format);

    // Create 128x128 Font & Solid Rect Atlas for zero-alloc UI
    let (_ui_texture, ui_bind_group) = create_font_atlas_texture(device, queue, &hud_pipeline);

    // 3. Player State with 60Hz Physics Accumulator
    let player_height = 1.75f32;
    let spawn_ground_y = terrain_height(0.0, 0.0);
    let mut player_pos = Vec3::new(0.0, spawn_ground_y + player_height, 0.0);
    let mut prev_player_pos = player_pos;
    let mut player_vel_y = 0.0f32;
    let mut is_grounded = true;

    // First-person camera angles (radians)
    let mut cam_yaw = 0.0f32;
    let mut cam_pitch = 0.0f32;

    let mouse_sensitivity = 0.0022f32;
    let move_speed = 10.0f32; // Meters per second
    let jump_impulse = 9.5f32;
    let gravity = 26.0f32;

    // Controls
    let mut key_w = false;
    let mut key_s = false;
    let mut key_a = false;
    let mut key_d = false;
    let mut key_space = false;
    let mut key_shift = false;

    // Realistic Flashlight Controller
    let initial_look = Vec3::new(0.0, 0.0, -1.0);
    let mut flashlight = FlashlightState::new(player_pos, initial_look);

    // Cursor position in window coordinates
    let mut cursor_screen_x = 0.0f32;
    let mut cursor_screen_y = 0.0f32;

    // Camera setup with dynamic FOV from settings
    let mut camera = Camera3D::new(
        player_pos,
        player_pos + initial_look,
        config.width as f32 / config.height as f32,
    );
    camera.fov_y_rad = settings.fov.to_radians();

    // 60Hz Fixed Timestep Clock for deterministic physics & sub-frame lerp
    let mut clock = GameClock::new(60);
    let start_time = Instant::now();
    let mut last_frame_time = Instant::now();

    let mut last_fps_update = Instant::now();
    let mut frame_count = 0u64;
    let mut current_fps = 60.0f32;
    let mut frame_time_ms = 16.6f32;

    let mut terrain_instances: Vec<TerrainChunkInstance> = Vec::with_capacity(256);
    let num_grass_variants = foliage_pipeline.variant_ranges.len();
    let mut grass_instances_by_variant: Vec<Vec<GrassInstanceRaw>> = (0..num_grass_variants)
        .map(|_| Vec::with_capacity(FoliagePipeline::MAX_INSTANCES_PER_VARIANT))
        .collect();
    let mut hud_instances: Vec<SpriteInstanceRaw> = Vec::with_capacity(512);
    let scratchpad = ThreadScratchpad::new();

    event_loop.run(move |event, elwt| {
        match event {
            // Raw mouse motion for 1st person camera
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta: (dx, dy) },
                ..
            } if cursor_locked && !settings_ui.is_open => {
                cam_yaw += dx as f32 * mouse_sensitivity;
                cam_pitch -= dy as f32 * mouse_sensitivity;
                // Clamp pitch so player cannot flip upside down (-88° to +88°)
                cam_pitch = cam_pitch.clamp(-1.53, 1.53);
            }

            Event::WindowEvent { event, window_id } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                WindowEvent::Resized(new_size) => {
                    config.width = new_size.width.max(1);
                    config.height = new_size.height.max(1);
                    surface.configure(device, &config);
                    mesh3d_pipeline.resize(device, config.width, config.height);
                    camera.set_aspect_ratio(config.width, config.height);
                }
                WindowEvent::CursorMoved { position, .. } => {
                    cursor_screen_x = position.x as f32;
                    cursor_screen_y = config.height as f32 - position.y as f32; // Invert to HUD coords
                    if settings_ui.is_open {
                        settings_ui.handle_mouse_move(cursor_screen_x, &mut settings);
                    }
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => {
                    if state == ElementState::Pressed {
                        if settings_ui.is_open {
                            settings_ui.handle_mouse_down(
                                cursor_screen_x,
                                cursor_screen_y,
                                &mut settings,
                            );
                        } else if !cursor_locked {
                            cursor_locked = true;
                            let _ = window
                                .set_cursor_grab(CursorGrabMode::Locked)
                                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
                            window.set_cursor_visible(false);
                        }
                    } else if state == ElementState::Released {
                        settings_ui.handle_mouse_up();
                    }
                }
                WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            physical_key: PhysicalKey::Code(key_code),
                            state,
                            ..
                        },
                    ..
                } => {
                    let is_pressed = state == ElementState::Pressed;
                    match key_code {
                        KeyCode::Escape => {
                            if is_pressed {
                                let is_now_open = settings_ui.toggle();
                                if is_now_open {
                                    cursor_locked = false;
                                    let _ = window.set_cursor_grab(CursorGrabMode::None);
                                    window.set_cursor_visible(true);
                                } else {
                                    cursor_locked = true;
                                    let _ = window
                                        .set_cursor_grab(CursorGrabMode::Locked)
                                        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
                                    window.set_cursor_visible(false);
                                }
                            }
                        }
                        // Toggle Flashlight with 'F' key
                        KeyCode::KeyF => {
                            if is_pressed {
                                flashlight.is_on = !flashlight.is_on;
                            }
                        }
                        KeyCode::KeyW => key_w = is_pressed,
                        KeyCode::KeyS => key_s = is_pressed,
                        KeyCode::KeyA => key_a = is_pressed,
                        KeyCode::KeyD => key_d = is_pressed,
                        KeyCode::Space => key_space = is_pressed,
                        KeyCode::ShiftLeft | KeyCode::ShiftRight => key_shift = is_pressed,
                        _ => {}
                    }
                }
                WindowEvent::RedrawRequested => {
                    scratchpad.reset();

                    let now = Instant::now();
                    let dt = (now - last_frame_time).as_secs_f32().min(0.05);
                    last_frame_time = now;

                    // --- 60HZ DETERMINISTIC FIXED PHYSICS STEPPING ---
                    let _frame_dt = clock.advance_frame();

                    while clock.should_step() {
                        let fixed_dt = clock.fixed_dt(); // Exactly 1/60th of a second
                        prev_player_pos = player_pos;

                        // Only move player if settings menu is not open
                        if !settings_ui.is_open {
                            let forward = Vec3::new(cam_yaw.sin(), 0.0, -cam_yaw.cos());
                            let right = Vec3::new(cam_yaw.cos(), 0.0, cam_yaw.sin());

                            let mut wish_dir = Vec3::ZERO;
                            if key_w {
                                wish_dir += forward;
                            }
                            if key_s {
                                wish_dir -= forward;
                            }
                            if key_d {
                                wish_dir += right;
                            }
                            if key_a {
                                wish_dir -= right;
                            }

                            let speed = if key_shift { move_speed * 1.6 } else { move_speed };

                            if wish_dir.length_squared() > 0.0 {
                                wish_dir = wish_dir.normalize();
                                let horizontal_move = wish_dir * speed * fixed_dt;
                                player_pos.x += horizontal_move.x;
                                player_pos.z += horizontal_move.z;
                            }

                            // Vertical Jump & Gravity
                            if key_space && is_grounded {
                                player_vel_y = jump_impulse;
                                is_grounded = false;
                            }

                            player_vel_y -= gravity * fixed_dt;
                            let new_y = player_pos.y + player_vel_y * fixed_dt;

                            // Ground height under player
                            let ground_y = terrain_height(player_pos.x, player_pos.z);
                            if new_y <= ground_y + player_height {
                                player_pos.y = ground_y + player_height;
                                player_vel_y = 0.0;
                                is_grounded = true;
                            } else {
                                player_pos.y = new_y;
                                is_grounded = false;
                            }
                        }
                    }

                    // --- SUB-FRAME TRANSFORM INTERPOLATION ---
                    let alpha = clock.alpha().clamp(0.0, 1.0);
                    let render_player_pos = prev_player_pos.lerp(player_pos, alpha);

                    // --- FIRST-PERSON CAMERA UPDATE ---
                    let look_dir = Vec3::new(
                        cam_pitch.cos() * cam_yaw.sin(),
                        cam_pitch.sin(),
                        cam_pitch.cos() * -cam_yaw.cos(),
                    )
                    .normalize();

                    camera.eye = Vec3A::from(render_player_pos);
                    camera.target = Vec3A::from(render_player_pos + look_dir);
                    camera.fov_y_rad = settings.fov.to_radians();

                    // --- UPDATE REALISTIC DYNAMIC FLASHLIGHT ---
                    let is_moving = key_w || key_s || key_a || key_d;
                    flashlight.update(dt, render_player_pos, look_dir, is_moving, key_shift);

                    // --- STREAM VISIBLE TERRAIN CHUNKS ---
                    let chunk_size = TerrainPipeline::CHUNK_SIZE;
                    let center_cx = (render_player_pos.x / chunk_size).floor() as i32;
                    let center_cz = (render_player_pos.z / chunk_size).floor() as i32;

                    let chunk_radius = match settings.view_distance {
                        QualityTier::Low => 2,
                        QualityTier::Medium => 3,
                        QualityTier::High => 4,
                        QualityTier::Epic => 5,
                    };

                    terrain_instances.clear();
                    for cz in (center_cz - chunk_radius)..=(center_cz + chunk_radius) {
                        for cx in (center_cx - chunk_radius)..=(center_cx + chunk_radius) {
                            terrain_instances.push(TerrainChunkInstance {
                                chunk_offset: [cx as f32 * chunk_size, cz as f32 * chunk_size],
                                _padding: [0.0, 0.0],
                            });
                        }
                    }
                    queue.write_buffer(
                        &terrain_pipeline.instance_buffer,
                        0,
                        bytemuck::cast_slice(&terrain_instances),
                    );

                    // --- STREAM DENSE CARPET 3D FOLIAGE AROUND PLAYER ---
                    // Smart concentric optimization: lush dense carpet around player, tapering naturally outward
                    let base_foliage_density = match settings.foliage {
                        QualityTier::Low => 550,
                        QualityTier::Medium => 800,
                        QualityTier::High => 1100,
                        QualityTier::Epic => 1400,
                    };
                    let foliage_radius = match settings.foliage {
                        QualityTier::Low => 1,
                        QualityTier::Medium => 2,
                        QualityTier::High => 2,
                        QualityTier::Epic => 3,
                    };

                    for list in &mut grass_instances_by_variant {
                        list.clear();
                    }
                    for cz in (center_cz - foliage_radius)..=(center_cz + foliage_radius) {
                        for cx in (center_cx - foliage_radius)..=(center_cx + foliage_radius) {
                            let dist = (cx - center_cx).abs().max((cz - center_cz).abs());
                            let clumps = if dist <= 1 {
                                base_foliage_density
                            } else {
                                base_foliage_density / 3
                            };

                            scatter_chunk_grass(
                                cx,
                                cz,
                                clumps,
                                num_grass_variants,
                                &mut grass_instances_by_variant,
                            );
                        }
                    }
                    for (i, instances) in grass_instances_by_variant.iter().enumerate() {
                        if !instances.is_empty() && i < foliage_pipeline.instance_buffers.len() {
                            queue.write_buffer(
                                &foliage_pipeline.instance_buffers[i],
                                0,
                                bytemuck::cast_slice(instances),
                            );
                        }
                    }

                    // Upload Camera & Dynamic Settings Uniforms to GPU
                    let view_proj = camera.view_proj_matrix();
                    let inv_view_proj = view_proj.inverse();
                    let sun_dir = Vec3::new(0.5, 0.9, 0.35).normalize();

                    let sun_pos = render_player_pos + sun_dir * 80.0;
                    let light_view = Mat4::look_at_rh(sun_pos, render_player_pos, Vec3::Y);
                    let ortho_size = 65.0;
                    let light_proj = Mat4::orthographic_rh(
                        -ortho_size,
                        ortho_size,
                        -ortho_size,
                        ortho_size,
                        0.1,
                        200.0,
                    );
                    let light_view_proj = light_proj * light_view;

                    let elapsed_time = start_time.elapsed().as_secs_f32();
                    let camera_input = CameraSettingsInput {
                        view_proj,
                        inv_view_proj,
                        light_view_proj,
                        cam_pos: render_player_pos,
                        light_dir: sun_dir,
                        gamma: settings.gamma,
                        motion_blur: elapsed_time, // continuous time for live root-anchored wind sway & sky stars
                        fov_rad: settings.fov.to_radians(),
                        view_dist: settings.view_distance_world(),
                        quality_tiers: settings.quality_tiers_u32(),
                        quality_extra: settings.quality_extra_u32(),
                        flashlight_pos: flashlight.current_pos,
                        flashlight_dir: flashlight.current_dir,
                        flashlight_on: flashlight.is_on,
                        flashlight_intensity: 1.0,
                    };
                    mesh3d_pipeline.update_camera(queue, &camera_input);

                    // --- ASSEMBLE 2D ON-SCREEN HUD OVERLAY (FPS INDICATOR & SETTINGS MENU) ---
                    hud_instances.clear();
                    let screen_w = config.width as f32;
                    let screen_h = config.height as f32;

                    // 1. In-game Crosshair & Controls Helper (when menu is closed)
                    if !settings_ui.is_open {
                        let cx = screen_w * 0.5;
                        let cy = screen_h * 0.5;
                        let cross_color = [1.0, 1.0, 1.0, 0.85];
                        hud_instances.push(SpriteInstanceRaw {
                            position: [cx - 8.0, cy - 1.0],
                            size: [16.0, 2.0],
                            uv_min: [0.0, 0.0],
                            uv_max: [1.0 / 128.0, 1.0 / 128.0],
                            color_tint: cross_color,
                        });
                        hud_instances.push(SpriteInstanceRaw {
                            position: [cx - 1.0, cy - 8.0],
                            size: [2.0, 16.0],
                            uv_min: [0.0, 0.0],
                            uv_max: [1.0 / 128.0, 1.0 / 128.0],
                            color_tint: cross_color,
                        });

                        // Controls helper at bottom of screen
                        hud_instances.push(SpriteInstanceRaw {
                            position: [16.0, 16.0],
                            size: [640.0, 32.0],
                            uv_min: [0.0, 0.0],
                            uv_max: [1.0 / 128.0, 1.0 / 128.0],
                            color_tint: [0.06, 0.09, 0.15, 0.85],
                        });
                        hud_instances.push(SpriteInstanceRaw {
                            position: [16.0, 46.0],
                            size: [640.0, 2.0],
                            uv_min: [0.0, 0.0],
                            uv_max: [1.0 / 128.0, 1.0 / 128.0],
                            color_tint: [0.06, 0.72, 0.50, 0.8],
                        });
                        settings_ui::render_text(
                            "WASD: MOVE | SHIFT: SPRINT | SPACE: JUMP | F: FLASHLIGHT | ESC: SETTINGS",
                            24.0,
                            24.0,
                            8.0,
                            8.0,
                            [0.85, 0.90, 0.96, 1.0],
                            &mut hud_instances,
                        );
                    }

                    // 2. Taffy-powered ESC Settings Menu Overlay (if open)
                    settings_ui.build_instances(
                        screen_w,
                        screen_h,
                        &settings,
                        &mut hud_instances,
                    );

                    // 3. Top-Left FPS & Performance HUD Card (always rendered on top, never hidden)
                    let hud_w = 168.0;
                    let hud_h = 62.0;
                    let hud_x = 16.0;
                    let hud_y = screen_h - hud_h - 16.0;

                    // Dark background card
                    hud_instances.push(SpriteInstanceRaw {
                        position: [hud_x, hud_y],
                        size: [hud_w, hud_h],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0 / 128.0, 1.0 / 128.0],
                        color_tint: [0.05, 0.08, 0.13, 0.94],
                    });
                    // Emerald top stripe
                    hud_instances.push(SpriteInstanceRaw {
                        position: [hud_x, hud_y + hud_h - 2.0],
                        size: [hud_w, 2.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0 / 128.0, 1.0 / 128.0],
                        color_tint: [0.06, 0.85, 0.55, 1.0],
                    });

                    // Live FPS color calculation
                    let fps_color = if current_fps >= 1000.0 {
                        [0.06, 0.95, 0.60, 1.0] // Emerald (>1k FPS)
                    } else if current_fps >= 144.0 {
                        [0.25, 0.80, 1.0, 1.0] // Blue
                    } else {
                        [0.98, 0.80, 0.25, 1.0] // Amber
                    };

                    // Large live FPS readout
                    let fps_str = format!("FPS: {:.0}", current_fps);
                    settings_ui::render_text(
                        &fps_str,
                        hud_x + 10.0,
                        hud_y + 36.0,
                        9.0,
                        11.0,
                        fps_color,
                        &mut hud_instances,
                    );

                    // Frame latency in milliseconds
                    let ms_str = format!("{:.2} MS", frame_time_ms);
                    settings_ui::render_text(
                        &ms_str,
                        hud_x + 94.0,
                        hud_y + 37.0,
                        8.0,
                        9.0,
                        [0.72, 0.80, 0.92, 1.0],
                        &mut hud_instances,
                    );

                    // Rolling meter bar
                    let bar_w = hud_w - 20.0;
                    let meter_pct = (current_fps / 3000.0).clamp(0.04, 1.0);
                    hud_instances.push(SpriteInstanceRaw {
                        position: [hud_x + 10.0, hud_y + 22.0],
                        size: [bar_w, 4.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0 / 128.0, 1.0 / 128.0],
                        color_tint: [0.10, 0.15, 0.22, 1.0],
                    });
                    hud_instances.push(SpriteInstanceRaw {
                        position: [hud_x + 10.0, hud_y + 22.0],
                        size: [bar_w * meter_pct, 4.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0 / 128.0, 1.0 / 128.0],
                        color_tint: fps_color,
                    });

                    // Sub-line: engine frequency & graphics API
                    settings_ui::render_text(
                        "60HZ * VULKAN 3D",
                        hud_x + 10.0,
                        hud_y + 7.0,
                        7.0,
                        8.0,
                        [0.45, 0.55, 0.68, 1.0],
                        &mut hud_instances,
                    );

                    // Update HUD Projection
                    let hud_proj = Mat4::orthographic_rh(0.0, screen_w, 0.0, screen_h, -1.0, 1.0);
                    hud_pipeline.update_camera(queue, hud_proj);

                    queue.write_buffer(
                        &hud_pipeline.instance_buffer,
                        0,
                        bytemuck::cast_slice(&hud_instances),
                    );

                    // --- DRAW FRAME VIA WGPU ---
                    if let Ok(output) = surface.get_current_texture() {
                        let view = output
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());

                        let mut encoder =
                            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("3D First-Person Encoder"),
                            });

                        // PASS 1: 3D Scene (Infinite Procedural Terrain & Dense Dynamic Foliage & Sky)
                        {
                            let mut render_pass =
                                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("3D World Pass"),
                                    color_attachments: &[Some(
                                        wgpu::RenderPassColorAttachment {
                                            view: &view,
                                            resolve_target: None,
                                            ops: wgpu::Operations {
                                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                                    r: 0.03,
                                                    g: 0.06,
                                                    b: 0.14,
                                                    a: 1.0,
                                                }),
                                                store: wgpu::StoreOp::Store,
                                            },
                                        },
                                    )],
                                    depth_stencil_attachment: Some(
                                        wgpu::RenderPassDepthStencilAttachment {
                                            view: &mesh3d_pipeline.depth_view,
                                            depth_ops: Some(wgpu::Operations {
                                                load: wgpu::LoadOp::Clear(0.0), // Reversed-Z
                                                store: wgpu::StoreOp::Store,
                                            }),
                                            stencil_ops: None,
                                        },
                                    ),
                                    occlusion_query_set: None,
                                    timestamp_writes: None,
                                });

                            // 1A: Terrain Chunks with PBR and Normal Mapping
                            render_pass.set_pipeline(&terrain_pipeline.pipeline);
                            render_pass.set_bind_group(0, &mesh3d_pipeline.camera_bind_group, &[]);
                            render_pass.set_bind_group(1, &terrain_pipeline.material_bind_group, &[]);
                            render_pass.set_vertex_buffer(
                                0,
                                terrain_pipeline.chunk_mesh_vertex_buffer.slice(..),
                            );
                            render_pass.set_vertex_buffer(
                                1,
                                terrain_pipeline.instance_buffer.slice(..),
                            );
                            render_pass.set_index_buffer(
                                terrain_pipeline.chunk_mesh_index_buffer.slice(..),
                                wgpu::IndexFormat::Uint16,
                            );
                            render_pass.draw_indexed(
                                0..terrain_pipeline.chunk_mesh_index_count,
                                0,
                                0..terrain_instances.len() as u32,
                            );

                            // 1B: Dense 3D Foliage with Wind Sway & Alpha Cutout
                            render_pass.set_pipeline(&foliage_pipeline.pipeline);
                            render_pass.set_bind_group(0, &mesh3d_pipeline.camera_bind_group, &[]);
                            render_pass.set_bind_group(1, &foliage_pipeline.material_bind_group, &[]);
                            render_pass.set_vertex_buffer(0, foliage_pipeline.vertex_buffer.slice(..));
                            render_pass.set_index_buffer(
                                foliage_pipeline.index_buffer.slice(..),
                                wgpu::IndexFormat::Uint16,
                            );

                            for (variant_idx, range) in foliage_pipeline.variant_ranges.iter().enumerate() {
                                let count = grass_instances_by_variant[variant_idx].len() as u32;
                                if count > 0 && variant_idx < foliage_pipeline.instance_buffers.len() {
                                    render_pass.set_vertex_buffer(
                                        1,
                                        foliage_pipeline.instance_buffers[variant_idx].slice(..),
                                    );
                                    render_pass.draw_indexed(
                                        range.index_start..(range.index_start + range.index_count),
                                        0,
                                        0..count,
                                    );
                                }
                            }

                            // 1C: Atmospheric Twilight Sky Dome & Starfield (drawn on background)
                            render_pass.set_pipeline(&sky_pipeline.pipeline);
                            render_pass.set_bind_group(0, &mesh3d_pipeline.camera_bind_group, &[]);
                            render_pass.draw(0..3, 0..1);
                        }

                        // PASS 2: 2D On-Screen HUD Overlay, Font Atlas, and Settings Menu
                        {
                            let mut hud_pass =
                                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("HUD & UI Pass"),
                                    color_attachments: &[Some(
                                        wgpu::RenderPassColorAttachment {
                                            view: &view,
                                            resolve_target: None,
                                            ops: wgpu::Operations {
                                                load: wgpu::LoadOp::Load,
                                                store: wgpu::StoreOp::Store,
                                            },
                                        },
                                    )],
                                    depth_stencil_attachment: None,
                                    occlusion_query_set: None,
                                    timestamp_writes: None,
                                });

                            hud_pass.set_pipeline(&hud_pipeline.pipeline);
                            hud_pass.set_bind_group(0, &hud_pipeline.camera_bind_group, &[]);
                            hud_pass.set_bind_group(1, &ui_bind_group, &[]);
                            hud_pass.set_vertex_buffer(
                                0,
                                hud_pipeline.quad_vertex_buffer.slice(..),
                            );
                            hud_pass.set_vertex_buffer(
                                1,
                                hud_pipeline.instance_buffer.slice(..),
                            );
                            hud_pass.draw(0..6, 0..hud_instances.len() as u32);
                        }

                        queue.submit(std::iter::once(encoder.finish()));
                        output.present();
                    }

                    frame_count += 1;

                    // Update FPS Metrics & Window Title every 0.25 seconds
                    if last_fps_update.elapsed().as_secs_f32() >= 0.25 {
                        let elapsed_sec = last_fps_update.elapsed().as_secs_f32();
                        current_fps = frame_count as f32 / elapsed_sec;
                        frame_time_ms = 1000.0 / current_fps.max(1.0);
                        let title = format!(
                            "Restgine 3D FPP | FPS: {:.1} ({:.2} ms) | Vulkan Uncapped | 60Hz Physics + Procedural Grassland | Pure Rust",
                            current_fps, frame_time_ms
                        );
                        window.set_title(&title);
                        frame_count = 0;
                        last_fps_update = Instant::now();
                    }

                    window.request_redraw();
                }
                _ => {}
            },
            Event::AboutToWait => {
                window.request_redraw();
            }
            _ => {}
        }
    })?;

    Ok(())
}
