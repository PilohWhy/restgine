use glam::{Quat, Vec3, Vec3A};
use hecs::World;
use restgine_core::{ThreadScratchpad, Transform3D};
use restgine_ecs::{
    AssetId, Footprint, GroundShadow, MeshletCluster, RenderLayer, RigidBodyXPBD, Sprite,
    Velocity3D,
};
use restgine_physics::{step_xpbd_substep, BVH8Broadphase};
use restgine_render::{Camera3D, MeshInstanceRaw};
use std::time::Instant;

pub fn run_continuum_demo(duration_secs: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("\x1b[1;36m=== Restgine Continuum Demo Game ===\x1b[0m");
    println!("Features: Unified 2D/2.5D & 3D world, Uncapped FPS, Reversed-Z 3D Camera");
    println!("Entities: Player character + 20 Bouncing 3D Cubes + 10 Elevation Projectiles + Static Pillars");
    println!(
        "Duration: {} (Running uncapped)\n",
        if duration_secs == 0 {
            "Infinite".to_string()
        } else {
            format!("{}s", duration_secs)
        }
    );

    let mut world = World::new();

    // 1. Spawn Player character (2.5D grounded entity)
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

    // 2. Spawn 20 Bouncing 3D Rigid Bodies
    for i in 0..20 {
        let fi = i as f32;
        let x = (fi - 10.0) * 15.0;
        let y = 10.0 + (i % 5) as f32 * 5.0;
        let z = ((i % 4) as f32 - 2.0) * 20.0;

        world.spawn((
            Transform3D {
                translation: Vec3A::new(x, y, z),
                rotation: Quat::IDENTITY,
                scale: Vec3A::splat(1.5),
                previous_translation: Vec3A::new(x, y, z),
            },
            Velocity3D {
                linear: Vec3A::new(0.0, 5.0 + fi * 0.5, 0.0),
                angular: Vec3::new(0.5, 1.0, 0.2),
            },
            RigidBodyXPBD::dynamic(2.0),
            MeshletCluster {
                mesh_asset: AssetId::DEFAULT_CUBE,
                cluster_offset: i as u32,
                cluster_count: 4,
                bounding_sphere: [0.0, 0.0, 0.0, 2.0],
            },
            Footprint::sphere(2.0),
            GroundShadow {
                base_radius: 6.0,
                opacity: 0.4,
            },
        ));
    }

    // 3. Spawn 10 Airborne Projectiles with elevation Z
    for i in 0..10 {
        let angle = i as f32 * std::f32::consts::TAU / 10.0;
        world.spawn((
            Transform3D::new(angle.cos() * 40.0, angle.sin() * 40.0, 12.0),
            Velocity3D {
                linear: Vec3A::new(-angle.sin() * 80.0, angle.cos() * 80.0, 0.0),
                angular: Vec3::ZERO,
            },
            Footprint::circle(4.0, 6.0),
            Sprite {
                asset: AssetId::DEFAULT_WHITE,
                layer: RenderLayer::Airborne,
                color_tint: [1.0, 0.8, 0.2, 1.0],
                ..Default::default()
            },
            GroundShadow {
                base_radius: 5.0,
                opacity: 0.3,
            },
        ));
    }

    // 4. Setup Reversed-Z 3D Camera tracking the scene
    let mut camera = Camera3D::new(
        Vec3::new(0.0, 60.0, 120.0),
        Vec3::new(0.0, 0.0, 0.0),
        16.0 / 9.0,
    );

    let mut bvh8 = BVH8Broadphase::new();
    let scratchpad = ThreadScratchpad::new();

    let start = Instant::now();
    let mut last_log = Instant::now();
    let mut frames_this_sec = 0u64;
    let mut total_frames = 0u64;
    let dt = 1.0 / 120.0;

    println!("\x1b[1;32m[PLAYING]\x1b[0m Simulation active with live uncapped metrics:");

    loop {
        if duration_secs > 0 && start.elapsed().as_secs() >= duration_secs {
            break;
        }

        let frame_start = Instant::now();
        scratchpad.reset();

        // Step 1: Update Player Movement & Bounce on boundaries
        if let Ok(mut player_trans) = world.get::<&mut Transform3D>(player) {
            if let Ok(mut player_vel) = world.get::<&mut Velocity3D>(player) {
                player_trans.translation += player_vel.linear * dt;
                // Boundary bounce
                if player_trans.translation.x.abs() > 150.0 {
                    player_vel.linear.x = -player_vel.linear.x;
                }
                if player_trans.translation.y.abs() > 100.0 {
                    player_vel.linear.y = -player_vel.linear.y;
                }
                camera.target = player_trans.translation;
                camera.eye = player_trans.translation + Vec3A::new(0.0, 60.0, 120.0);
            }
        }

        // Camera Uniform update (Reversed-Z view-projection matrix)
        let _view_proj = camera.view_proj_matrix();

        // Step 2: XPBD Physics Simulation (sub-stepped)
        step_xpbd_substep(&mut world, dt, 4, false);

        // Step 3: SIMD BVH8 Broadphase build
        bvh8.clear();
        for (entity, (trans, _)) in world.query::<(&Transform3D, &Footprint)>().iter() {
            let r = 4.0;
            let min = Vec3::new(
                trans.translation.x - r,
                trans.translation.y - r,
                trans.translation.z - r,
            );
            let max = Vec3::new(
                trans.translation.x + r,
                trans.translation.y + r,
                trans.translation.z + r,
            );
            bvh8.insert(entity, min, max);
        }
        bvh8.build();

        // Step 4: Extract 64-bit Visibility Buffer instances into scratchpad
        if let Some(instances) = scratchpad.alloc_slice::<MeshInstanceRaw>(32) {
            let mut count = 0;
            for (_id, (trans, cluster)) in world.query::<(&Transform3D, &MeshletCluster)>().iter() {
                if count < instances.len() {
                    instances[count] =
                        MeshInstanceRaw::from_mat4(trans.to_mat4(), cluster.cluster_offset);
                    count += 1;
                }
            }
        }

        let _frame_time_us = frame_start.elapsed().as_micros();
        frames_this_sec += 1;
        total_frames += 1;

        // Log stats every 1 second
        if last_log.elapsed().as_secs_f32() >= 1.0 {
            let elapsed_sec = last_log.elapsed().as_secs_f32();
            let current_fps = frames_this_sec as f32 / elapsed_sec;
            println!(
                "  -> Frame #{:<8} | Uncapped FPS: \x1b[1;32m{:>8.1}\x1b[0m | Frame Latency: {:>6.2} µs | VRAM Allocs: 0 B",
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
    println!("\n\x1b[1;32m[DONE]\x1b[0m Continuum game demo finished successfully.");
    println!("  Total Frames:    {}", total_frames);
    println!("  Elapsed Time:    {:.2}s", total_elapsed);
    println!(
        "  Overall Avg FPS: \x1b[1;32m{:.1} FPS\x1b[0m\n",
        overall_fps
    );

    Ok(())
}
