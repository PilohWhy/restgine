use glam::Vec2;
use restgine_ecs::{Footprint, FootprintShape, Transform3D};

/// Checks if two entities overlap in space (ground plane footprint AND vertical Z-interval).
pub fn check_overlap_25d(
    t1: &Transform3D,
    f1: &Footprint,
    t2: &Transform3D,
    f2: &Footprint,
) -> Option<Vec2> {
    // 1. Z-axis vertical interval overlap check
    let z1_min = t1.translation.z;
    let z1_max = z1_min + f1.z_height;
    let z2_min = t2.translation.z;
    let z2_max = z2_min + f2.z_height;

    let z_overlap = z1_min.max(z2_min) < z1_max.min(z2_max);
    if !z_overlap {
        return None;
    }

    // 2. Ground plane (x, y) footprint intersection
    let p1 = Vec2::new(t1.translation.x, t1.translation.y);
    let p2 = Vec2::new(t2.translation.x, t2.translation.y);

    match (&f1.shape, &f2.shape) {
        (FootprintShape::Circle { radius: r1 }, FootprintShape::Circle { radius: r2 }) => {
            let diff = p1 - p2;
            let dist_sq = diff.length_squared();
            let min_dist = r1 + r2;
            if dist_sq < min_dist * min_dist {
                let dist = dist_sq.sqrt();
                let normal = if dist > 0.0001 {
                    diff / dist
                } else {
                    Vec2::new(0.0, 1.0)
                };
                let penetration = min_dist - dist;
                Some(normal * penetration)
            } else {
                None
            }
        }
        (FootprintShape::Circle { radius }, FootprintShape::AABB { half_extents }) => {
            circle_aabb_overlap(p1, *radius, p2, *half_extents)
        }
        (FootprintShape::AABB { half_extents }, FootprintShape::Circle { radius }) => {
            circle_aabb_overlap(p2, *radius, p1, *half_extents).map(|v| -v)
        }
        (FootprintShape::AABB { half_extents: h1 }, FootprintShape::AABB { half_extents: h2 }) => {
            let diff = p1 - p2;
            let overlap_x = (h1.x + h2.x) - diff.x.abs();
            let overlap_y = (h1.y + h2.y) - diff.y.abs();

            if overlap_x > 0.0 && overlap_y > 0.0 {
                if overlap_x < overlap_y {
                    let sign = if diff.x >= 0.0 { 1.0 } else { -1.0 };
                    Some(Vec2::new(sign * overlap_x, 0.0))
                } else {
                    let sign = if diff.y >= 0.0 { 1.0 } else { -1.0 };
                    Some(Vec2::new(0.0, sign * overlap_y))
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn circle_aabb_overlap(
    circle_pos: Vec2,
    radius: f32,
    aabb_pos: Vec2,
    aabb_half: Vec2,
) -> Option<Vec2> {
    let diff = circle_pos - aabb_pos;
    let clamped = Vec2::new(
        diff.x.clamp(-aabb_half.x, aabb_half.x),
        diff.y.clamp(-aabb_half.y, aabb_half.y),
    );
    let closest = aabb_pos + clamped;
    let circle_to_closest = circle_pos - closest;
    let dist_sq = circle_to_closest.length_squared();

    if dist_sq < radius * radius {
        let dist = dist_sq.sqrt();
        let normal = if dist > 0.0001 {
            circle_to_closest / dist
        } else {
            Vec2::new(0.0, 1.0)
        };
        let penetration = radius - dist;
        Some(normal * penetration)
    } else {
        None
    }
}
