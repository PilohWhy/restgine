mod settings_ui;

use glam::{Mat4, Quat, Vec3, Vec3A};
use restgine_core::time::GameClock;
use restgine_core::ThreadScratchpad;
use restgine_render::mesh3d::CameraSettingsInput;
use restgine_render::{
    Camera3D, Mesh3DInstanceRaw, Mesh3DPipeline, SpriteInstanceRaw, SpritePipeline, WindowContext,
};
use settings_ui::{create_font_atlas_texture, GameSettings, SettingsUI};
use std::time::Instant;
use winit::event::{DeviceEvent, ElementState, Event, KeyEvent, MouseButton, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::CursorGrabMode;

// AABB for block collision
#[derive(Clone, Copy)]
struct BlockAABB {
    min: Vec3,
    max: Vec3,
    color: [f32; 4],
}

impl BlockAABB {
    fn new(center: Vec3, size: Vec3, color: [f32; 4]) -> Self {
        let half = size * 0.5;
        Self {
            min: center - half,
            max: center + half,
            color,
        }
    }

    fn to_instance(self) -> Mesh3DInstanceRaw {
        let center = (self.min + self.max) * 0.5;
        let size = self.max - self.min;
        let model = Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, center);
        Mesh3DInstanceRaw::new(model, self.color)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,restgine=info,game=info"),
    )
    .init();

    // 1. Initialize Window with uncapped GPU surface
    let mut window_ctx = WindowContext::new(
        "Restgine 3D — First-Person Walkaround | Vulkan Uncapped",
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

    // 2. Pipelines
    let mut mesh3d_pipeline =
        Mesh3DPipeline::new(device, config.format, config.width, config.height);
    let hud_pipeline = SpritePipeline::new(device, config.format);

    // Create 128x128 Font & Solid Rect Atlas for zero-alloc UI
    let (_ui_texture, ui_bind_group) = create_font_atlas_texture(device, queue, &hud_pipeline);

    // 3. World Setup: Plane and Static Blocks (Exact 0 background entities)
    let mut blocks: Vec<BlockAABB> = Vec::new();

    // Large Ground Plane (140x140 tiles)
    let ground = BlockAABB::new(
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(140.0, 1.0, 140.0),
        [0.08, 0.11, 0.16, 1.0], // Dark slate floor
    );

    // Stairs leading to a high platform
    blocks.push(BlockAABB::new(
        Vec3::new(0.0, 0.5, -8.0),
        Vec3::new(4.0, 1.0, 3.0),
        [0.15, 0.20, 0.28, 1.0],
    ));
    blocks.push(BlockAABB::new(
        Vec3::new(0.0, 1.0, -11.0),
        Vec3::new(4.0, 2.0, 3.0),
        [0.18, 0.24, 0.34, 1.0],
    ));
    blocks.push(BlockAABB::new(
        Vec3::new(0.0, 1.5, -14.0),
        Vec3::new(4.0, 3.0, 3.0),
        [0.22, 0.29, 0.40, 1.0],
    ));
    blocks.push(BlockAABB::new(
        Vec3::new(0.0, 2.0, -18.0),
        Vec3::new(8.0, 4.0, 5.0),
        [0.25, 0.34, 0.46, 1.0],
    )); // High platform

    // Colorful feature blocks around the arena
    blocks.push(BlockAABB::new(
        Vec3::new(-10.0, 1.5, -6.0),
        Vec3::new(3.0, 3.0, 3.0),
        [0.06, 0.85, 0.55, 1.0],
    )); // Emerald block
    blocks.push(BlockAABB::new(
        Vec3::new(12.0, 2.0, -4.0),
        Vec3::new(4.0, 4.0, 4.0),
        [0.96, 0.26, 0.42, 1.0],
    )); // Ruby block
    blocks.push(BlockAABB::new(
        Vec3::new(8.0, 1.0, 10.0),
        Vec3::new(2.5, 2.0, 2.5),
        [0.23, 0.51, 0.96, 1.0],
    )); // Sapphire block
    blocks.push(BlockAABB::new(
        Vec3::new(-12.0, 2.5, 8.0),
        Vec3::new(3.0, 5.0, 3.0),
        [0.96, 0.76, 0.20, 1.0],
    )); // Amber monolith
    blocks.push(BlockAABB::new(
        Vec3::new(-6.0, 1.0, 14.0),
        Vec3::new(3.0, 2.0, 3.0),
        [0.72, 0.38, 0.94, 1.0],
    )); // Amethyst block

    // Perimeter boundary walls
    blocks.push(BlockAABB::new(
        Vec3::new(0.0, 2.0, -70.0),
        Vec3::new(140.0, 4.0, 2.0),
        [0.06, 0.72, 0.50, 0.9],
    ));
    blocks.push(BlockAABB::new(
        Vec3::new(0.0, 2.0, 70.0),
        Vec3::new(140.0, 4.0, 2.0),
        [0.06, 0.72, 0.50, 0.9],
    ));
    blocks.push(BlockAABB::new(
        Vec3::new(-70.0, 2.0, 0.0),
        Vec3::new(2.0, 4.0, 140.0),
        [0.06, 0.72, 0.50, 0.9],
    ));
    blocks.push(BlockAABB::new(
        Vec3::new(70.0, 2.0, 0.0),
        Vec3::new(2.0, 4.0, 140.0),
        [0.06, 0.72, 0.50, 0.9],
    ));

    // 4. First-Person Player State with 60Hz Physics Accumulator
    let mut player_pos = Vec3::new(0.0, 1.7, 5.0); // Eye level at 1.7m
    let mut prev_player_pos = player_pos;
    let mut player_vel_y = 0.0f32;
    let mut is_grounded = true;
    let player_radius = 0.45f32;
    let player_height = 1.75f32;

    // First-person camera angles (radians)
    let mut cam_yaw = 0.0f32; // Horizontal rotation
    let mut cam_pitch = 0.0f32; // Vertical pitch

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

    // Cursor position in window coordinates
    let mut cursor_screen_x = 0.0f32;
    let mut cursor_screen_y = 0.0f32;

    // Camera setup with dynamic FOV from settings
    let mut camera = Camera3D::new(
        player_pos,
        player_pos + Vec3::new(0.0, 0.0, -1.0),
        config.width as f32 / config.height as f32,
    );
    camera.fov_y_rad = settings.fov.to_radians();

    // 60Hz Fixed Timestep Clock for deterministic physics & sub-frame lerp
    let mut clock = GameClock::new(60);

    let mut last_fps_update = Instant::now();
    let mut frame_count = 0u64;
    let mut current_fps = 0.0f32;
    let mut frame_time_ms = 0.0f32;

    let mut mesh_instances: Vec<Mesh3DInstanceRaw> = Vec::with_capacity(64);
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

                                // Collision step for X
                                let new_x = player_pos.x + horizontal_move.x;
                                if !check_player_collision(
                                    Vec3::new(new_x, player_pos.y, player_pos.z),
                                    player_radius,
                                    player_height,
                                    &blocks,
                                ) {
                                    player_pos.x = new_x;
                                }
                                // Collision step for Z
                                let new_z = player_pos.z + horizontal_move.z;
                                if !check_player_collision(
                                    Vec3::new(player_pos.x, player_pos.y, new_z),
                                    player_radius,
                                    player_height,
                                    &blocks,
                                ) {
                                    player_pos.z = new_z;
                                }
                            }

                            // Vertical Jump & Gravity
                            if key_space && is_grounded {
                                player_vel_y = jump_impulse;
                                is_grounded = false;
                            }

                            player_vel_y -= gravity * fixed_dt;
                            let new_y = player_pos.y + player_vel_y * fixed_dt;

                            // Floor collision & top of blocks
                            let ground_y = get_ground_height_under_player(
                                player_pos,
                                player_radius,
                                &blocks,
                            );
                            if new_y <= ground_y + player_height {
                                player_pos.y = ground_y + player_height;
                                player_vel_y = 0.0;
                                is_grounded = true;
                            } else {
                                player_pos.y = new_y;
                                is_grounded = false;
                            }

                            // Arena bounds clamping
                            player_pos.x = player_pos.x.clamp(-68.0, 68.0);
                            player_pos.z = player_pos.z.clamp(-68.0, 68.0);
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

                    // --- ASSEMBLE 3D MESHES TO RENDER ---
                    mesh_instances.clear();

                    // 1. Ground Plane
                    mesh_instances.push(ground.to_instance());

                    // 2. All 3D Blocks & Stairs
                    for block in &blocks {
                        mesh_instances.push(block.to_instance());
                    }

                    // Upload Camera & Instances to GPU
                    let view_proj = camera.view_proj_matrix();
                    let sun_dir = Vec3::new(0.5, 0.9, 0.35).normalize();

                    // Directional Shadow Projection Matrix
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

                    let camera_input = CameraSettingsInput {
                        view_proj,
                        light_view_proj,
                        cam_pos: render_player_pos,
                        light_dir: sun_dir,
                        gamma: settings.gamma,
                        motion_blur: settings.motion_blur,
                        fov_rad: settings.fov.to_radians(),
                        view_dist: settings.view_distance_world(),
                        quality_tiers: settings.quality_tiers_u32(),
                        quality_extra: settings.quality_extra_u32(),
                    };
                    mesh3d_pipeline.update_camera(queue, &camera_input);

                    queue.write_buffer(
                        &mesh3d_pipeline.instance_buffer,
                        0,
                        bytemuck::cast_slice(&mesh_instances),
                    );

                    // --- ASSEMBLE 2D ON-SCREEN HUD OVERLAY (FPS INDICATOR & SETTINGS MENU) ---
                    hud_instances.clear();
                    let screen_w = config.width as f32;
                    let screen_h = config.height as f32;

                    // 1. Top-Left FPS & Performance HUD Card
                    hud_instances.push(SpriteInstanceRaw {
                        position: [16.0, screen_h - 110.0],
                        size: [280.0, 94.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0 / 128.0, 1.0 / 128.0],
                        color_tint: [0.06, 0.09, 0.15, 0.88], // Dark slate translucent
                    });
                    hud_instances.push(SpriteInstanceRaw {
                        position: [16.0, screen_h - 18.0],
                        size: [280.0, 3.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0 / 128.0, 1.0 / 128.0],
                        color_tint: [0.06, 0.85, 0.55, 1.0], // Emerald accent
                    });

                    // Live FPS color badge
                    let fps_badge_color = if current_fps >= 1000.0 {
                        [0.06, 0.85, 0.55, 1.0] // Emerald (>1k FPS)
                    } else if current_fps >= 144.0 {
                        [0.23, 0.71, 0.96, 1.0] // Blue
                    } else {
                        [0.96, 0.76, 0.20, 1.0] // Amber
                    };

                    hud_instances.push(SpriteInstanceRaw {
                        position: [26.0, screen_h - 48.0],
                        size: [110.0, 20.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0 / 128.0, 1.0 / 128.0],
                        color_tint: fps_badge_color,
                    });

                    // Rolling FPS meter bar
                    let fps_meter = (current_fps / 5000.0).clamp(0.04, 1.0);
                    hud_instances.push(SpriteInstanceRaw {
                        position: [26.0, screen_h - 60.0],
                        size: [260.0 * fps_meter, 6.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0 / 128.0, 1.0 / 128.0],
                        color_tint: [0.06, 0.72, 0.50, 0.9],
                    });

                    // Latency bar (<1ms = emerald)
                    let latency_meter = (frame_time_ms / 16.66).clamp(0.02, 1.0);
                    hud_instances.push(SpriteInstanceRaw {
                        position: [26.0, screen_h - 72.0],
                        size: [260.0 * latency_meter, 4.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0 / 128.0, 1.0 / 128.0],
                        color_tint: if frame_time_ms < 1.0 {
                            [0.06, 0.85, 0.55, 0.95]
                        } else {
                            [0.23, 0.71, 0.96, 0.95]
                        },
                    });

                    // 2. Crosshair in the exact center of screen (only when menu is closed)
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
                            size: [580.0, 32.0],
                            uv_min: [0.0, 0.0],
                            uv_max: [1.0 / 128.0, 1.0 / 128.0],
                            color_tint: [0.06, 0.09, 0.15, 0.85],
                        });
                        hud_instances.push(SpriteInstanceRaw {
                            position: [16.0, 46.0],
                            size: [580.0, 2.0],
                            uv_min: [0.0, 0.0],
                            uv_max: [1.0 / 128.0, 1.0 / 128.0],
                            color_tint: [0.06, 0.72, 0.50, 0.8],
                        });
                        settings_ui::render_text(
                            "WASD: MOVE | SPACE: JUMP | SHIFT: SPRINT | ESC: SETTINGS MENU",
                            24.0,
                            24.0,
                            8.0,
                            8.0,
                            [0.85, 0.90, 0.96, 1.0],
                            &mut hud_instances,
                        );
                    }

                    // 3. Taffy-powered ESC Settings Menu Overlay (if open)
                    settings_ui.build_instances(
                        screen_w,
                        screen_h,
                        &settings,
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

                        // PASS 0: Directional Shadow Pass
                        {
                            let mut shadow_pass =
                                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("Directional Shadow Pass"),
                                    color_attachments: &[],
                                    depth_stencil_attachment: Some(
                                        wgpu::RenderPassDepthStencilAttachment {
                                            view: &mesh3d_pipeline.shadow_view,
                                            depth_ops: Some(wgpu::Operations {
                                                load: wgpu::LoadOp::Clear(1.0),
                                                store: wgpu::StoreOp::Store,
                                            }),
                                            stencil_ops: None,
                                        },
                                    ),
                                    occlusion_query_set: None,
                                    timestamp_writes: None,
                                });

                            shadow_pass.set_pipeline(&mesh3d_pipeline.shadow_pipeline);
                            shadow_pass
                                .set_bind_group(0, &mesh3d_pipeline.camera_bind_group, &[]);
                            shadow_pass.set_vertex_buffer(
                                0,
                                mesh3d_pipeline.cube_vertex_buffer.slice(..),
                            );
                            shadow_pass.set_vertex_buffer(
                                1,
                                mesh3d_pipeline.instance_buffer.slice(..),
                            );
                            shadow_pass.set_index_buffer(
                                mesh3d_pipeline.cube_index_buffer.slice(..),
                                wgpu::IndexFormat::Uint16,
                            );
                            shadow_pass.draw_indexed(
                                0..mesh3d_pipeline.cube_index_count,
                                0,
                                0..mesh_instances.len() as u32,
                            );
                        }

                        // PASS 1: 3D First-Person Scene with PBR and Dynamic Shadows
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
                                                    r: 0.04,
                                                    g: 0.06,
                                                    b: 0.11,
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

                            render_pass.set_pipeline(&mesh3d_pipeline.pipeline);
                            render_pass
                                .set_bind_group(0, &mesh3d_pipeline.camera_bind_group, &[]);
                            render_pass.set_bind_group(1, &mesh3d_pipeline.bone_bind_group, &[]);
                            render_pass.set_vertex_buffer(
                                0,
                                mesh3d_pipeline.cube_vertex_buffer.slice(..),
                            );
                            render_pass.set_vertex_buffer(
                                1,
                                mesh3d_pipeline.instance_buffer.slice(..),
                            );
                            render_pass.set_index_buffer(
                                mesh3d_pipeline.cube_index_buffer.slice(..),
                                wgpu::IndexFormat::Uint16,
                            );
                            render_pass.draw_indexed(
                                0..mesh3d_pipeline.cube_index_count,
                                0,
                                0..mesh_instances.len() as u32,
                            );
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
                            "Restgine 3D FPP | FPS: {:.1} ({:.2} ms) | Vulkan Uncapped | 60Hz Physics + Dynamic Shaders | Pure Rust",
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

fn check_player_collision(pos: Vec3, radius: f32, height: f32, blocks: &[BlockAABB]) -> bool {
    let p_min = Vec3::new(pos.x - radius, pos.y - height + 0.1, pos.z - radius);
    let p_max = Vec3::new(pos.x + radius, pos.y, pos.z + radius);

    for b in blocks {
        if p_min.x < b.max.x
            && p_max.x > b.min.x
            && p_min.y < b.max.y
            && p_max.y > b.min.y
            && p_min.z < b.max.z
            && p_max.z > b.min.z
        {
            return true;
        }
    }
    false
}

fn get_ground_height_under_player(pos: Vec3, radius: f32, blocks: &[BlockAABB]) -> f32 {
    let mut max_y = 0.0f32; // Floor plane is at Y=0
    let p_min_x = pos.x - radius;
    let p_max_x = pos.x + radius;
    let p_min_z = pos.z - radius;
    let p_max_z = pos.z + radius;

    for b in blocks {
        if p_min_x < b.max.x
            && p_max_x > b.min.x
            && p_min_z < b.max.z
            && p_max_z > b.min.z
            && b.max.y <= pos.y + 0.2
        {
            // Under or near feet
            if b.max.y > max_y {
                max_y = b.max.y;
            }
        }
    }
    max_y
}
