use glam::{Mat4, Quat, Vec3, Vec3A};
use hecs::World;
use restgine_core::{ThreadScratchpad, Transform3D};
use restgine_ecs::{Footprint, RigidBodyXPBD, Velocity3D};
use restgine_physics::step_xpbd_substep;
use restgine_render::{
    Camera3D, Mesh3DInstanceRaw, Mesh3DPipeline, SpriteInstanceRaw, SpritePipeline, WindowContext,
};
use std::time::Instant;
use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,restgine=info,game=info"),
    )
    .init();

    // 1. Initialize Native Window with uncapped GPU surface
    let mut window_ctx = WindowContext::new(
        "Restgine 3D Game | Pure Rust | Vulkan / DirectX 12",
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

    // 2. Initialize 3D Mesh pipeline with Reversed-Z depth testing
    let mut mesh3d_pipeline =
        Mesh3DPipeline::new(device, config.format, config.width, config.height);

    // 3. Initialize 2D Sprite pipeline for On-Screen HUD Overlay
    let hud_pipeline = SpritePipeline::new(device, config.format);

    // 4. Populate 3D Simulation World
    let mut world = World::new();

    // Spawn 3D Player entity
    let player = world.spawn((
        Transform3D {
            translation: Vec3A::new(0.0, 1.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3A::new(2.0, 2.0, 2.0),
            previous_translation: Vec3A::new(0.0, 1.0, 0.0),
        },
        Velocity3D {
            linear: Vec3A::ZERO,
            angular: Vec3::ZERO,
        },
        RigidBodyXPBD::dynamic(5.0),
        Footprint::sphere(1.5),
    ));

    // Spawn 40+ 3D Bouncing Cubes with diverse candy colors and angular velocities
    let palette: [[f32; 4]; 7] = [
        [0.96, 0.26, 0.42, 1.0], // Ruby red
        [0.23, 0.51, 0.96, 1.0], // Sapphire blue
        [0.10, 0.82, 0.61, 1.0], // Emerald green
        [0.96, 0.76, 0.20, 1.0], // Amber gold
        [0.72, 0.38, 0.94, 1.0], // Amethyst purple
        [0.06, 0.85, 0.91, 1.0], // Cyan
        [0.98, 0.55, 0.24, 1.0], // Coral orange
    ];

    let mut cube_colors: Vec<[f32; 4]> = Vec::new();

    for i in 0..42 {
        let fi = i as f32;
        let ring = (i / 7) as f32 + 1.0;
        let angle = (i % 7) as f32 * (std::f32::consts::TAU / 7.0) + ring * 0.4;
        let dist = ring * 18.0;

        let x = angle.cos() * dist;
        let y = 8.0 + (i % 6) as f32 * 4.0;
        let z = angle.sin() * dist;

        let scale = 1.2 + (i % 4) as f32 * 0.4;
        let color = palette[i % palette.len()];
        cube_colors.push(color);

        world.spawn((
            Transform3D {
                translation: Vec3A::new(x, y, z),
                rotation: Quat::from_rotation_y(angle),
                scale: Vec3A::splat(scale),
                previous_translation: Vec3A::new(x, y, z),
            },
            Velocity3D {
                linear: Vec3A::new(
                    (fi - 21.0) * 0.8,
                    12.0 + (i % 5) as f32 * 3.0,
                    (i as f32 % 5.0) * 2.0,
                ),
                angular: Vec3::new(
                    1.2 + (i % 3) as f32 * 0.5,
                    0.8 + (i % 4) as f32 * 0.4,
                    1.5 - (i % 2) as f32 * 0.8,
                ),
            },
            RigidBodyXPBD::dynamic(1.5),
            Footprint::sphere(scale),
        ));
    }

    // 5. 3D Camera with 3rd-person follow and orbit
    let mut camera = Camera3D::new(
        Vec3::new(0.0, 35.0, 55.0),
        Vec3::new(0.0, 2.0, 0.0),
        config.width as f32 / config.height as f32,
    );
    let mut cam_yaw = 0.0f32;
    let cam_distance = 65.0f32;

    // Controls state
    let mut key_w = false;
    let mut key_s = false;
    let mut key_a = false;
    let mut key_d = false;
    let mut key_space = false;
    let mut key_q = false;
    let mut key_e = false;

    let dt = 1.0 / 120.0;
    let mut frame_count = 0u64;
    let mut last_fps_update = Instant::now();
    let mut current_fps = 0.0f32;
    let mut frame_time_ms = 0.0f32;

    let mut mesh_instances: Vec<Mesh3DInstanceRaw> = Vec::with_capacity(128);
    let mut hud_instances: Vec<SpriteInstanceRaw> = Vec::with_capacity(64);

    let scratchpad = ThreadScratchpad::new();

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event, window_id } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                WindowEvent::Resized(new_size) => {
                    config.width = new_size.width.max(1);
                    config.height = new_size.height.max(1);
                    surface.configure(device, &config);
                    mesh3d_pipeline.resize(device, config.width, config.height);
                    camera.set_aspect_ratio(config.width, config.height);
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
                        KeyCode::KeyW | KeyCode::ArrowUp => key_w = is_pressed,
                        KeyCode::KeyS | KeyCode::ArrowDown => key_s = is_pressed,
                        KeyCode::KeyA | KeyCode::ArrowLeft => key_a = is_pressed,
                        KeyCode::KeyD | KeyCode::ArrowRight => key_d = is_pressed,
                        KeyCode::Space => key_space = is_pressed,
                        KeyCode::KeyQ => key_q = is_pressed,
                        KeyCode::KeyE => key_e = is_pressed,
                        _ => {}
                    }
                }
                WindowEvent::RedrawRequested => {
                    scratchpad.reset();

                    // --- CAMERA ROTATION ---
                    if key_q {
                        cam_yaw += 2.0 * dt;
                    }
                    if key_e {
                        cam_yaw -= 2.0 * dt;
                    }

                    // --- PLAYER INPUT & 3D MOVEMENT ---
                    let forward = Vec3A::new(-cam_yaw.sin(), 0.0, -cam_yaw.cos());
                    let right = Vec3A::new(cam_yaw.cos(), 0.0, -cam_yaw.sin());
                    let mut move_dir = Vec3A::ZERO;

                    if key_w {
                        move_dir += forward;
                    }
                    if key_s {
                        move_dir -= forward;
                    }
                    if key_a {
                        move_dir -= right;
                    }
                    if key_d {
                        move_dir += right;
                    }

                    let mut player_pos = Vec3A::ZERO;

                    if let Ok(mut trans) = world.get::<&mut Transform3D>(player) {
                        if let Ok(mut vel) = world.get::<&mut Velocity3D>(player) {
                            let speed = 40.0;
                            if move_dir.length_squared() > 0.0 {
                                let norm = move_dir.normalize();
                                vel.linear.x = norm.x * speed;
                                vel.linear.z = norm.z * speed;
                            } else {
                                vel.linear.x *= 0.85;
                                vel.linear.z *= 0.85;
                            }

                            // Jump impulse
                            if key_space && trans.translation.y <= 1.05 {
                                vel.linear.y = 22.0;
                            }

                            // Apply gravity to player
                            vel.linear.y -= 45.0 * dt;
                            trans.translation += vel.linear * dt;

                            // Floor collision
                            if trans.translation.y <= 1.0 {
                                trans.translation.y = 1.0;
                                vel.linear.y = 0.0;
                            }

                            // Arena boundaries
                            trans.translation.x = trans.translation.x.clamp(-75.0, 75.0);
                            trans.translation.z = trans.translation.z.clamp(-75.0, 75.0);

                            player_pos = trans.translation;
                        }
                    }

                    // --- CAMERA FOLLOW ---
                    let cam_offset = Vec3A::new(
                        cam_yaw.sin() * cam_distance,
                        32.0,
                        cam_yaw.cos() * cam_distance,
                    );
                    camera.eye = player_pos + cam_offset;
                    camera.target = player_pos + Vec3A::new(0.0, 2.0, 0.0);

                    // --- XPBD PHYSICS SIMULATION FOR BOUNCING 3D CUBES ---
                    step_xpbd_substep(&mut world, dt, 4, false);

                    // Ground bounces and 3D rotational spins
                    for (_, (trans, vel)) in
                        world.query_mut::<(&mut Transform3D, &mut Velocity3D)>()
                    {
                        // Gravity
                        vel.linear.y -= 38.0 * dt;
                        trans.translation += vel.linear * dt;

                        // 3D rotation update
                        let ang_speed = vel.angular.length();
                        if ang_speed > 0.001 {
                            let rot_axis = vel.angular / ang_speed;
                            let delta_rot = Quat::from_axis_angle(rot_axis, ang_speed * dt);
                            trans.rotation = (delta_rot * trans.rotation).normalize();
                        }

                        // Floor bounce with high elasticity
                        let half_h = trans.scale.y * 0.5;
                        if trans.translation.y <= half_h {
                            trans.translation.y = half_h;
                            vel.linear.y = -vel.linear.y * 0.88; // Bouncy
                            vel.linear.x += (rand_simple(trans.translation.x) - 0.5) * 2.0;
                            vel.linear.z += (rand_simple(trans.translation.z) - 0.5) * 2.0;
                        }

                        // Wall bounces
                        if trans.translation.x.abs() > 75.0 {
                            vel.linear.x = -vel.linear.x * 0.9;
                            trans.translation.x = trans.translation.x.clamp(-75.0, 75.0);
                        }
                        if trans.translation.z.abs() > 75.0 {
                            vel.linear.z = -vel.linear.z * 0.9;
                            trans.translation.z = trans.translation.z.clamp(-75.0, 75.0);
                        }
                    }

                    // --- ASSEMBLE 3D INSTANCES ---
                    mesh_instances.clear();

                    // 1. 3D Ground Floor (wide dark slate tiles)
                    let floor_mat = Mat4::from_scale_rotation_translation(
                        Vec3::new(160.0, 0.5, 160.0),
                        Quat::IDENTITY,
                        Vec3::new(0.0, -0.25, 0.0),
                    );
                    mesh_instances.push(Mesh3DInstanceRaw::new(
                        floor_mat,
                        [0.08, 0.11, 0.16, 1.0], // Dark slate
                    ));

                    // 2. Perimeter Pillars
                    for px in &[-78.0, 78.0] {
                        for pz in &[-78.0, 78.0] {
                            let pillar_mat = Mat4::from_scale_rotation_translation(
                                Vec3::new(3.0, 16.0, 3.0),
                                Quat::IDENTITY,
                                Vec3::new(*px, 8.0, *pz),
                            );
                            mesh_instances.push(Mesh3DInstanceRaw::new(
                                pillar_mat,
                                [0.06, 0.72, 0.50, 1.0], // Emerald accents
                            ));
                        }
                    }

                    // 3. Player 3D Mesh (bright emerald cube)
                    if let Ok(trans) = world.get::<&Transform3D>(player) {
                        mesh_instances.push(Mesh3DInstanceRaw::new(
                            trans.to_mat4(),
                            [0.06, 0.85, 0.55, 1.0], // Emerald player
                        ));
                    }

                    // 4. 40+ Bouncing Candy Cubes
                    let mut color_idx = 0;
                    for (entity, trans) in world.query::<&Transform3D>().iter() {
                        if entity != player {
                            let color = if color_idx < cube_colors.len() {
                                cube_colors[color_idx]
                            } else {
                                [0.9, 0.5, 0.2, 1.0]
                            };
                            mesh_instances.push(Mesh3DInstanceRaw::new(trans.to_mat4(), color));
                            color_idx += 1;
                        }
                    }

                    // Update 3D Camera Uniforms
                    let view_proj = camera.view_proj_matrix();
                    let light_dir = Vec3::new(0.6, 1.0, 0.45).normalize();
                    mesh3d_pipeline.update_camera(
                        queue,
                        view_proj,
                        Vec3::from(camera.eye),
                        light_dir,
                    );

                    queue.write_buffer(
                        &mesh3d_pipeline.instance_buffer,
                        0,
                        bytemuck::cast_slice(&mesh_instances),
                    );

                    // --- ASSEMBLE 2D ON-SCREEN HUD OVERLAY (FPS INDICATOR) ---
                    hud_instances.clear();
                    let screen_w = config.width as f32;
                    let screen_h = config.height as f32;

                    // HUD Container Card (Top-Left corner)
                    hud_instances.push(SpriteInstanceRaw {
                        position: [16.0, screen_h - 130.0],
                        size: [280.0, 114.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: [0.06, 0.09, 0.15, 0.88], // Translucent slate-900
                    });

                    // HUD Card Top Border Accent (Emerald)
                    hud_instances.push(SpriteInstanceRaw {
                        position: [16.0, screen_h - 18.0],
                        size: [280.0, 3.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: [0.06, 0.72, 0.50, 1.0],
                    });

                    // Live FPS Pill / Badge
                    let fps_color = if current_fps >= 2000.0 {
                        [0.06, 0.85, 0.55, 1.0] // Emerald green
                    } else if current_fps >= 144.0 {
                        [0.23, 0.71, 0.96, 1.0] // Bright blue
                    } else {
                        [0.96, 0.76, 0.20, 1.0] // Amber
                    };

                    hud_instances.push(SpriteInstanceRaw {
                        position: [26.0, screen_h - 48.0],
                        size: [120.0, 22.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: fps_color,
                    });

                    // FPS bar visualizer (relative performance meter)
                    let fps_ratio = (current_fps / 5000.0).clamp(0.05, 1.0);
                    hud_instances.push(SpriteInstanceRaw {
                        position: [26.0, screen_h - 60.0],
                        size: [260.0 * fps_ratio, 6.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: [0.06, 0.72, 0.50, 0.9],
                    });

                    // Frametime latency indicator bar (< 1ms = emerald green)
                    let latency_ratio = (frame_time_ms / 16.66).clamp(0.02, 1.0);
                    hud_instances.push(SpriteInstanceRaw {
                        position: [26.0, screen_h - 74.0],
                        size: [260.0 * latency_ratio, 4.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: if frame_time_ms < 1.0 {
                            [0.06, 0.85, 0.55, 0.95]
                        } else {
                            [0.23, 0.71, 0.96, 0.95]
                        },
                    });

                    // Controls helper bar at screen bottom
                    hud_instances.push(SpriteInstanceRaw {
                        position: [16.0, 16.0],
                        size: [520.0, 32.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: [0.06, 0.09, 0.15, 0.82],
                    });
                    hud_instances.push(SpriteInstanceRaw {
                        position: [16.0, 46.0],
                        size: [520.0, 2.0],
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        color_tint: [0.06, 0.72, 0.50, 0.8],
                    });

                    // Update HUD Orthographic Projection (Screen Space 0,0 at bottom-left)
                    let hud_proj = Mat4::orthographic_rh(0.0, screen_w, 0.0, screen_h, -1.0, 1.0);
                    hud_pipeline.update_camera(queue, hud_proj);

                    queue.write_buffer(
                        &hud_pipeline.instance_buffer,
                        0,
                        bytemuck::cast_slice(&hud_instances),
                    );

                    // --- RENDER PASSES VIA WGPU ---
                    if let Ok(output) = surface.get_current_texture() {
                        let view = output
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());

                        let mut encoder =
                            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("Game 3D Encoder"),
                            });

                        // PASS 1: 3D Scene Rasterization with Depth Buffer
                        {
                            let mut render_pass =
                                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("3D Game Render Pass"),
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
                                                load: wgpu::LoadOp::Clear(0.0), // Reversed-Z far plane
                                                store: wgpu::StoreOp::Store,
                                            }),
                                            stencil_ops: None,
                                        },
                                    ),
                                    occlusion_query_set: None,
                                    timestamp_writes: None,
                                });

                            render_pass.set_pipeline(&mesh3d_pipeline.pipeline);
                            render_pass.set_bind_group(0, &mesh3d_pipeline.camera_bind_group, &[]);
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

                        // PASS 2: 2D On-Screen HUD Overlay (FPS Indicator)
                        {
                            let mut hud_pass =
                                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("HUD Overlay Pass"),
                                    color_attachments: &[Some(
                                        wgpu::RenderPassColorAttachment {
                                            view: &view,
                                            resolve_target: None,
                                            ops: wgpu::Operations {
                                                load: wgpu::LoadOp::Load, // Preserve 3D scene
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
                            hud_pass.set_bind_group(
                                1,
                                &hud_pipeline.default_texture_bind_group,
                                &[],
                            );
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
                            "Restgine 3D Game | FPS: {:.1} ({:.2} ms) | 43 Entities | XPBD 3D Physics | Vulkan Uncapped",
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

fn rand_simple(seed: f32) -> f32 {
    let x = (seed * 12.9898 + 78.233).sin() * 43_758.547_f32;
    x - x.floor()
}
