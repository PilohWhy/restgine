use crate::collision::check_overlap_25d;
use crate::spatial_grid::SpatialHashGrid;
use glam::Vec2;
use hecs::World;
use restgine_ecs::{Footprint, FootprintShape, Transform3D, Velocity3D};

/// Run a deterministic physics tick:
/// 1. Integrate position from velocity.
/// 2. Build spatial hash grid of solid obstacles.
/// 3. Resolve dynamic vs static wall collisions with smooth sliding.
pub fn step_physics_25d(world: &mut World, grid: &mut SpatialHashGrid, dt: f32) {
    grid.clear();

    // 1. Store previous position and update current
    for (_id, (transform, velocity)) in world.query_mut::<(&mut Transform3D, &Velocity3D)>() {
        transform.previous_translation = transform.translation;
        transform.translation += velocity.linear * dt;
    }

    // 2. Populate spatial grid with solid static entities
    for (id, (transform, footprint)) in world.query_mut::<(&Transform3D, &Footprint)>() {
        if footprint.is_solid {
            let p = Vec2::new(transform.translation.x, transform.translation.y);
            let (min, max) = match footprint.shape {
                FootprintShape::Circle { radius } | FootprintShape::Sphere3D { radius } => {
                    (p - Vec2::splat(radius), p + Vec2::splat(radius))
                }
                FootprintShape::AABB { half_extents } => (p - half_extents, p + half_extents),
                FootprintShape::Box3D { half_extents } => {
                    let ext = Vec2::new(half_extents.x, half_extents.y);
                    (p - ext, p + ext)
                }
            };
            grid.insert_aabb(id, min, max);
        }
    }

    // 3. Resolve overlaps for dynamic moving entities
    let moving_entities: Vec<_> = world
        .query::<(&Transform3D, &Velocity3D, &Footprint)>()
        .iter()
        .map(|(id, _)| id)
        .collect();

    for entity in moving_entities {
        let (mut current_transform, current_footprint) =
            match world.query_one::<(&Transform3D, &Footprint)>(entity) {
                Ok(mut q) => {
                    if let Some((t, f)) = q.get() {
                        (*t, *f)
                    } else {
                        continue;
                    }
                }
                Err(_) => continue,
            };

        let pos2 = Vec2::new(
            current_transform.translation.x,
            current_transform.translation.y,
        );
        let query_box_min = pos2 - Vec2::splat(32.0);
        let query_box_max = pos2 + Vec2::splat(32.0);
        let potential_colliders = grid.query_range(query_box_min, query_box_max);

        for other_entity in potential_colliders {
            if other_entity == entity {
                continue;
            }

            if let Ok(mut other_q) = world.query_one::<(&Transform3D, &Footprint)>(other_entity) {
                if let Some((other_transform, other_footprint)) = other_q.get() {
                    if let Some(push_vector) = check_overlap_25d(
                        &current_transform,
                        &current_footprint,
                        other_transform,
                        other_footprint,
                    ) {
                        // Push back along ground plane
                        current_transform.translation.x += push_vector.x;
                        current_transform.translation.y += push_vector.y;
                    }
                }
            }
        }

        // Apply updated transform back
        if let Ok(mut trans_ref) = world.get::<&mut Transform3D>(entity) {
            trans_ref.translation = current_transform.translation;
        }
    }
}
