use glam::Vec3A;
use hecs::World;
use restgine_ecs::{RigidBodyXPBD, Transform3D, Velocity3D};

/// Execute sub-stepped XPBD simulation.
/// Handles position integration, contact stabilization, velocity updates, and planar 2D clamping.
pub fn step_xpbd_substep(world: &mut World, dt: f32, substeps: u32, is_2d_planar: bool) {
    let sub_dt = dt / (substeps.max(1) as f32);
    let gravity = Vec3A::new(0.0, if is_2d_planar { 0.0 } else { -9.81 }, 0.0);

    for _step in 0..substeps {
        // 1. Predict unconstrained positions
        for (_id, (transform, vel, rb)) in
            world.query_mut::<(&mut Transform3D, &mut Velocity3D, &RigidBodyXPBD)>()
        {
            if rb.is_kinematic {
                continue;
            }

            transform.previous_translation = transform.translation;
            // Apply gravity and integrate
            vel.linear += gravity * sub_dt;
            transform.translation += vel.linear * sub_dt;

            if is_2d_planar {
                transform.translation.z = 0.0;
                vel.linear.z = 0.0;
            }
        }

        // 2. Solve positional constraints (Ground floor plane collision)
        for (_id, (transform, _vel, rb)) in
            world.query_mut::<(&mut Transform3D, &Velocity3D, &RigidBodyXPBD)>()
        {
            if rb.is_kinematic {
                continue;
            }

            // Floor boundary constraint at y = 0
            if transform.translation.y < 0.0 {
                transform.translation.y = 0.0;
            }
        }

        // 3. Derive updated velocities from position deltas
        for (_id, (transform, vel, rb)) in
            world.query_mut::<(&Transform3D, &mut Velocity3D, &RigidBodyXPBD)>()
        {
            if rb.is_kinematic {
                continue;
            }

            vel.linear = (transform.translation - transform.previous_translation) / sub_dt;
        }
    }
}
