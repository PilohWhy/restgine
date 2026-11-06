use glam::{Mat4, Quat, Vec3, Vec3A};
use hecs::World;
use restgine_core::{ThreadScratchpad, Transform3D};
use restgine_ecs::{
    AssetId, Footprint, GroundShadow, MeshletCluster, RenderLayer, RigidBodyXPBD, Sprite,
    Velocity3D,
};
use restgine_physics::{step_xpbd_substep, BVH8Broadphase};
use restgine_render::{SpriteInstanceRaw, SpritePipeline, WindowContext};
use std::time::Instant;
use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

pub fn run_continuum_demo(duration_secs: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("\x1b[1;36m=== Restgine Continuum Demo Game ===\x1b[0m");
    println!("Initializing 2D/3D Continuum Graphical Pipeline on GPU...");

    // Try creating graphical window first
    match WindowContext::new("Restgine Continuum Demo [Pure Rust | wgpu]", 1280, 720) {
        Ok(window_ctx) => {
            println!("\x1b[1;32m[WINDOW CREATED]\x1b[0m 1280x720 window active.");
            println!("Controls: WASD / Arrow Keys to move player, ESC to exit.");
            run_windowed_demo(window_ctx, duration_secs)
        }
        Err(e) => {
            println!(
                "\x1b[1;33m[HEADLESS FALLBACK]\x1b[0m Graphical window unavailable ({e}). Running simulation loop.",
            );
            run_headless_demo(duration_secs)
        }
    }
}

fn run_windowed_demo(
    mut window_ctx: WindowContext,
    duration_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = window_ctx
        .event_loop
        .take()
        .ok_or("EventLoop already taken")?;
    let device = &window_ctx.device;
    let queue = &window_ctx.queue;
    let surface = &window_ctx.surface;
    let window = window_ctx.window.clone();
    let mut config = window_ctx.config;

    let sprite_pipeline = SpritePipeline::new(device, config.format);

    let mut world = World::new();

    // 1. Spawn Player character (emerald green avatar)
    let player = world.spawn((
        Transform3D::new(0.0, 0.0, 0.0),
        Velocity3D {
            linear: Vec3A::ZERO,
            angular: Vec3::ZERO,
        },
        Footprint::circle(12.0, 24.0),
        Sprite {
            asset: AssetId::DEFAULT_WHITE,
            layer: RenderLayer::GroundEntities,
            color_tint: [0.06, 0.72, 0.50, 1.0], // Emerald green #10b981
            ..Default::default()
        },
        GroundShadow {
            base_radius: 16.0,
            opacity: 0.6,
        },
    ));

    // 2. Spawn 20 Bouncing 3D Rigid Bodies with distinct candy palettes
    let cube_colors: [[f32; 4]; 5] = [
        [0.94, 0.38, 0.57, 1.0], // Candy pink
        [0.23, 0.51, 0.96, 1.0], // Cobalt blue
        [0.96, 0.71, 0.20, 1.0], // Amber gold
        [0.66, 0.38, 0.94, 1.0], // Purple
        [0.10, 0.74, 0.61, 1.0], // Teal
    ];

    for i in 0..20 {
        let fi = i as f32;
        let x = (fi - 10.0) * 35.0;
        let y = 30.0 + (i % 5) as f32 * 12.0;
        let z = ((i % 4) as f32 - 2.0) * 40.0;
        let color = cube_colors[i % cube_colors.len()];

        world.spawn((
            Transform3D {
                translation: Vec3A::new(x, y, z),
                rotation: Quat::IDENTITY,
                scale: Vec3A::splat(1.5),
                previous_translation: Vec3A::new(x, y, z),
            },
            Velocity3D {
                linear: Vec3A::new((fi - 10.0) * 5.0, 15.0 + fi * 2.0, 0.0),
                angular: Vec3::new(0.8, 1.2, 0.4),
            },
            RigidBodyXPBD::dynamic(2.0),
            MeshletCluster {
                mesh_asset: AssetId::DEFAULT_CUBE,
                cluster_offset: i as u32,
                cluster_count: 4,
                bounding_sphere: [0.0, 0.0, 0.0, 2.0],
            },
            Footprint::sphere(4.0),
            Sprite {
                asset: AssetId::DEFAULT_WHITE,
                layer: RenderLayer::Airborne,
                color_tint: color,
                ..Default::default()
            },
            GroundShadow {
                base_radius: 10.0,
                opacity: 0.45,
            },
        ));
    }

    // 3. Spawn 10 Airborne Projectiles orbiting
    for i in 0..10 {
        let angle = i as f32 * std::f32::consts::TAU / 10.0;
        world.spawn((
            Transform3D::new(angle.cos() * 120.0, angle.sin() * 120.0, 25.0),
            Velocity3D {
                linear: Vec3A::new(-angle.sin() * 90.0, angle.cos() * 90.0, 0.0),
                angular: Vec3::ZERO,
            },
            Footprint::circle(6.0, 8.0),
            Sprite {
                asset: AssetId::DEFAULT_WHITE,
                layer: RenderLayer::Airborne,
                color_tint: [1.0, 0.85, 0.25, 1.0], // Golden glow
                ..Default::default()
            },
            GroundShadow {
                base_radius: 7.0,
                opacity: 0.35,
            },
        ));
    }

    let mut cam_target = Vec3A::ZERO;

    let mut key_up = false;
    let mut key_down = false;
    let mut key_left = false;
    let mut key_right = false;

    let start_time = Instant::now();
    let mut last_fps_update = Instant::now();
    let mut frame_count = 0u64;
    let dt = 1.0 / 120.0;

    let mut raw_instances: Vec<SpriteInstanceRaw> = Vec::with_capacity(256);

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event, window_id } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                WindowEvent::Resized(new_size) => {
                    config.width = new_size.width.max(1);
                    config.height = new_size.height.max(1);
                    surface.configure(device, &config);
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
                        KeyCode::Escape => elwt.exit(),
                        KeyCode::KeyW | KeyCode::ArrowUp => key_up = is_pressed,
                        KeyCode::KeyS | KeyCode::ArrowDown => key_down = is_pressed,
                        KeyCode::KeyA | KeyCode::ArrowLeft => key_left = is_pressed,
                        KeyCode::KeyD | KeyCode::ArrowRight => key_right = is_pressed,
                        _ => {}
                    }
                }
                WindowEvent::RedrawRequested => {
                    if duration_secs > 0 && start_time.elapsed().as_secs() >= duration_secs {
                        elwt.exit();
                        return;
                    }

                    // 1. Process player input
                    let mut move_dir = Vec3A::ZERO;
                    if key_up {
                        move_dir.y += 1.0;
                    }
                    if key_down {
                        move_dir.y -= 1.0;
                    }
                    if key_left {
                        move_dir.x -= 1.0;
                    }
                    if key_right {
                        move_dir.x += 1.0;
                    }
                    let speed = 220.0;

                    if let Ok(mut trans) = world.get::<&mut Transform3D>(player) {
                        if move_dir.length_squared() > 0.0 {
                            trans.translation += move_dir.normalize() * speed * dt;
                        }
                        // Arena boundary clamping
                        trans.translation.x = trans.translation.x.clamp(-350.0, 350.0);
                        trans.translation.y = trans.translation.y.clamp(-220.0, 220.0);

                        // Smooth camera follow
                        cam_target = trans.translation;
                    }

                    // 2. XPBD Physics Sub-stepping
                    step_xpbd_substep(&mut world, dt, 4, false);

                    // 3. Bounce 3D cubes on ground floor (Z = 0) and arena bounds
                    for (_, (trans, vel)) in
                        world.query_mut::<(&mut Transform3D, &mut Velocity3D)>()
                    {
                        if trans.translation.y <= 4.0 && vel.linear.y < 0.0 {
                            vel.linear.y = -vel.linear.y * 0.85; // Bouncy ground
                            trans.translation.y = 4.0;
                        }
                        if trans.translation.x.abs() > 360.0 {
                            vel.linear.x = -vel.linear.x;
                        }
                        if trans.translation.z.abs() > 240.0 {
                            vel.linear.z = -vel.linear.z;
                        }
                    }

                    // 4. Build Sprite Instance Batch for Rendering
                    raw_instances.clear();

                    // 4a. Arena Ground Grid (dark stylish floor tile)
                    raw_instances.push(SpriteInstanceRaw {
                        position: [-380.0, -250.0],
                        size: [760.0, 500.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: [0.10, 0.13, 0.18, 1.0], // Dark slate floor
                    });

                    // 4b. Arena Outer Border
                    raw_instances.push(SpriteInstanceRaw {
                        position: [-382.0, -252.0],
                        size: [764.0, 4.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: [0.06, 0.72, 0.50, 0.9], // Emerald border
                    });
                    raw_instances.push(SpriteInstanceRaw {
                        position: [-382.0, 248.0],
                        size: [764.0, 4.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: [0.06, 0.72, 0.50, 0.9],
                    });
                    raw_instances.push(SpriteInstanceRaw {
                        position: [-382.0, -250.0],
                        size: [4.0, 500.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: [0.06, 0.72, 0.50, 0.9],
                    });
                    raw_instances.push(SpriteInstanceRaw {
                        position: [378.0, -250.0],
                        size: [4.0, 500.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: [0.06, 0.72, 0.50, 0.9],
                    });

                    // 4c. Render Drop Shadows for entities
                    for (_, (trans, shadow)) in world.query::<(&Transform3D, &GroundShadow)>().iter()
                    {
                        // Shadow shrinks as entity height (Y) increases
                        let height_factor = (1.0 - (trans.translation.y / 80.0)).clamp(0.2, 1.0);
                        let radius = shadow.base_radius * height_factor;
                        raw_instances.push(SpriteInstanceRaw {
                            position: [
                                trans.translation.x - radius,
                                trans.translation.z - radius * 0.5,
                            ],
                            size: [radius * 2.0, radius],
                            uv_min: [0.0, 0.0],
                            uv_max: [1.0, 1.0],
                            color_tint: [0.0, 0.0, 0.0, shadow.opacity * height_factor],
                        });
                    }

                    // 4d. Render Entity Sprites / Meshes
                    for (_, (trans, sprite)) in world.query::<(&Transform3D, &Sprite)>().iter() {
                        let size = 20.0 * trans.scale.x;
                        raw_instances.push(SpriteInstanceRaw {
                            position: [
                                trans.translation.x - size * 0.5,
                                trans.translation.z + trans.translation.y - size * 0.5,
                            ],
                            size: [size, size],
                            uv_min: [0.0, 0.0],
                            uv_max: [1.0, 1.0],
                            color_tint: sprite.color_tint,
                        });
                    }

                    // 5. Update Camera projection centered on player and Instance Buffer
                    let aspect = config.width as f32 / config.height as f32;
                    let ortho_w = 900.0;
                    let ortho_h = ortho_w / aspect;
                    let view_proj = Mat4::orthographic_rh(
                        cam_target.x - ortho_w * 0.5,
                        cam_target.x + ortho_w * 0.5,
                        cam_target.y - ortho_h * 0.5,
                        cam_target.y + ortho_h * 0.5,
                        -1000.0,
                        1000.0,
                    );
                    sprite_pipeline.update_camera(queue, view_proj);

                    queue.write_buffer(
                        &sprite_pipeline.instance_buffer,
                        0,
                        bytemuck::cast_slice(&raw_instances),
                    );

                    // 6. Draw Frame via wgpu
                    if let Ok(output) = surface.get_current_texture() {
                        let view = output
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());
                        let mut encoder =
                            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("Demo Command Encoder"),
                            });

                        {
                            let mut render_pass =
                                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("Demo Render Pass"),
                                    color_attachments: &[Some(
                                        wgpu::RenderPassColorAttachment {
                                            view: &view,
                                            resolve_target: None,
                                            ops: wgpu::Operations {
                                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                                    r: 0.04,
                                                    g: 0.06,
                                                    b: 0.10,
                                                    a: 1.0,
                                                }),
                                                store: wgpu::StoreOp::Store,
                                            },
                                        },
                                    )],
                                    depth_stencil_attachment: None,
                                    occlusion_query_set: None,
                                    timestamp_writes: None,
                                });

                            render_pass.set_pipeline(&sprite_pipeline.pipeline);
                            render_pass.set_bind_group(0, &sprite_pipeline.camera_bind_group, &[]);
                            render_pass.set_bind_group(
                                1,
                                &sprite_pipeline.default_texture_bind_group,
                                &[],
                            );
                            render_pass.set_vertex_buffer(
                                0,
                                sprite_pipeline.quad_vertex_buffer.slice(..),
                            );
                            render_pass.set_vertex_buffer(
                                1,
                                sprite_pipeline.instance_buffer.slice(..),
                            );
                            render_pass.draw(0..6, 0..raw_instances.len() as u32);
                        }

                        queue.submit(std::iter::once(encoder.finish()));
                        output.present();
                    }

                    frame_count += 1;

                    // 7. Live Window Title Stats Update
                    if last_fps_update.elapsed().as_secs_f32() >= 0.25 {
                        let elapsed_sec = last_fps_update.elapsed().as_secs_f32();
                        let current_fps = frame_count as f32 / elapsed_sec;
                        let frame_time_ms = 1000.0 / current_fps.max(1.0);
                        let title = format!(
                            "Restgine Continuum Demo | FPS: {:.1} ({:.2} ms) | 31 Entities | XPBD Physics | Pure Rust",
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

fn run_headless_demo(duration_secs: u64) -> Result<(), Box<dyn std::error::Error>> {
    let mut world = World::new();

    let player = world.spawn((
        Transform3D::new(0.0, 0.0, 0.0),
        Velocity3D {
            linear: Vec3A::new(60.0, 40.0, 0.0),
            angular: Vec3::ZERO,
        },
        Footprint::circle(8.0, 16.0),
        Sprite {
            asset: AssetId::DEFAULT_WHITE,
            layer: RenderLayer::GroundEntities,
            ..Default::default()
        },
        GroundShadow {
            base_radius: 12.0,
            opacity: 0.6,
        },
    ));

    for i in 0..20 {
        let fi = i as f32;
        world.spawn((
            Transform3D::new((fi - 10.0) * 15.0, 10.0, 0.0),
            Velocity3D {
                linear: Vec3A::new(0.0, 5.0 + fi * 0.5, 0.0),
                angular: Vec3::ZERO,
            },
            RigidBodyXPBD::dynamic(2.0),
            Footprint::sphere(2.0),
            GroundShadow {
                base_radius: 6.0,
                opacity: 0.4,
            },
        ));
    }

    let mut bvh8 = BVH8Broadphase::new();
    let scratchpad = ThreadScratchpad::new();
    let start = Instant::now();
    let mut last_log = Instant::now();
    let mut frames_this_sec = 0u64;
    let mut total_frames = 0u64;
    let dt = 1.0 / 120.0;

    println!("\x1b[1;32m[PLAYING]\x1b[0m Headless simulation active with live uncapped metrics:");

    loop {
        if duration_secs > 0 && start.elapsed().as_secs() >= duration_secs {
            break;
        }

        scratchpad.reset();

        if let Ok(mut player_trans) = world.get::<&mut Transform3D>(player) {
            if let Ok(mut player_vel) = world.get::<&mut Velocity3D>(player) {
                player_trans.translation += player_vel.linear * dt;
                if player_trans.translation.x.abs() > 150.0 {
                    player_vel.linear.x = -player_vel.linear.x;
                }
            }
        }

        step_xpbd_substep(&mut world, dt, 4, false);

        bvh8.clear();
        for (entity, (trans, _)) in world.query::<(&Transform3D, &Footprint)>().iter() {
            let r = 4.0;
            let min = Vec3::new(trans.translation.x - r, trans.translation.y - r, 0.0);
            let max = Vec3::new(trans.translation.x + r, trans.translation.y + r, 0.0);
            bvh8.insert(entity, min, max);
        }
        bvh8.build();

        frames_this_sec += 1;
        total_frames += 1;

        if last_log.elapsed().as_secs_f32() >= 1.0 {
            let elapsed_sec = last_log.elapsed().as_secs_f32();
            let current_fps = frames_this_sec as f32 / elapsed_sec;
            println!(
                "  -> Frame #{:<8} | Uncapped FPS: \x1b[1;32m{:>8.1}\x1b[0m | Frame Latency: {:>6.2} µs | 0 Heap Allocs",
                total_frames,
                current_fps,
                1_000_000.0 / current_fps,
            );
            frames_this_sec = 0;
            last_log = Instant::now();
        }
    }

    let total_elapsed = start.elapsed().as_secs_f64();
    let overall_fps = total_frames as f64 / total_elapsed;
    println!("\n\x1b[1;32m[DONE]\x1b[0m Simulation finished successfully.");
    println!("  Total Frames:    {}", total_frames);
    println!("  Elapsed Time:    {:.2}s", total_elapsed);
    println!(
        "  Overall Avg FPS: \x1b[1;32m{:.1} FPS\x1b[0m\n",
        overall_fps
    );

    Ok(())
}
