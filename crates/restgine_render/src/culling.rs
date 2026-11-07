use glam::{Mat4, Vec3, Vec4};

/// Plane in 3D space: Ax + By + Cz + D = 0, normalized.
#[derive(Debug, Clone, Copy)]
pub struct Plane {
    pub normal: Vec3,
    pub d: f32,
}

impl Plane {
    #[inline]
    pub fn new(x: f32, y: f32, z: f32, d: f32) -> Self {
        let len = (x * x + y * y + z * z).sqrt();
        if len > 1e-6 {
            Self {
                normal: Vec3::new(x / len, y / len, z / len),
                d: d / len,
            }
        } else {
            Self {
                normal: Vec3::ZERO,
                d,
            }
        }
    }

    #[inline]
    pub fn distance_to_point(&self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.d
    }
}

/// Axis-Aligned Bounding Box (AABB) in world space.
#[derive(Debug, Clone, Copy)]
pub struct AABB {
    pub min: Vec3,
    pub max: Vec3,
}

impl AABB {
    #[inline]
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    #[inline]
    pub fn from_center_half_extents(center: Vec3, half_extents: Vec3) -> Self {
        Self {
            min: center - half_extents,
            max: center + half_extents,
        }
    }

    #[inline]
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    #[inline]
    pub fn half_extents(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }
}

/// Bounding Sphere in world space.
#[derive(Debug, Clone, Copy)]
pub struct BoundingSphere {
    pub center: Vec3,
    pub radius: f32,
}

impl BoundingSphere {
    #[inline]
    pub fn new(center: Vec3, radius: f32) -> Self {
        Self { center, radius }
    }
}

/// High-performance 3D Camera Viewing Frustum extracted from View-Projection matrix.
/// Supports both standard/orthographic and Reversed-Z infinite perspective projections.
#[derive(Debug, Clone, Copy)]
pub struct Frustum {
    /// Planes: 0=Left, 1=Right, 2=Bottom, 3=Top, 4=Near(RevZ)/Far(Std), 5=Far(RevZ)/Near(Std)
    pub planes: [Plane; 6],
}

impl Frustum {
    /// Extract frustum planes from combined View-Projection matrix using Gribb-Hartmann method.
    pub fn from_view_proj(m: Mat4) -> Self {
        // Rows of the matrix
        let row0 = Vec4::new(m.x_axis.x, m.y_axis.x, m.z_axis.x, m.w_axis.x);
        let row1 = Vec4::new(m.x_axis.y, m.y_axis.y, m.z_axis.y, m.w_axis.y);
        let row2 = Vec4::new(m.x_axis.z, m.y_axis.z, m.z_axis.z, m.w_axis.z);
        let row3 = Vec4::new(m.x_axis.w, m.y_axis.w, m.z_axis.w, m.w_axis.w);

        // Left plane: row3 + row0
        let p_left = row3 + row0;
        // Right plane: row3 - row0
        let p_right = row3 - row0;
        // Bottom plane: row3 + row1
        let p_bottom = row3 + row1;
        // Top plane: row3 - row1
        let p_top = row3 - row1;
        // Clip Z <= W plane (Near plane in Reversed-Z, Far plane in Standard-Z / Ortho): row3 - row2
        let p_z_max = row3 - row2;
        // Clip Z >= 0 plane (Infinite Far in Reversed-Z, Near plane in Standard-Z / Ortho): row2
        let p_z_min = row2;

        Self {
            planes: [
                Plane::new(p_left.x, p_left.y, p_left.z, p_left.w),
                Plane::new(p_right.x, p_right.y, p_right.z, p_right.w),
                Plane::new(p_bottom.x, p_bottom.y, p_bottom.z, p_bottom.w),
                Plane::new(p_top.x, p_top.y, p_top.z, p_top.w),
                Plane::new(p_z_max.x, p_z_max.y, p_z_max.z, p_z_max.w),
                Plane::new(p_z_min.x, p_z_min.y, p_z_min.z, p_z_min.w),
            ],
        }
    }

    /// Fast test if a Bounding Sphere is inside or intersecting the frustum.
    #[inline]
    pub fn intersects_sphere(&self, sphere: &BoundingSphere) -> bool {
        for plane in &self.planes {
            if plane.distance_to_point(sphere.center) < -sphere.radius {
                return false;
            }
        }
        true
    }

    /// Fast test if an AABB is inside or intersecting the frustum.
    #[inline]
    pub fn intersects_aabb(&self, aabb: &AABB) -> bool {
        for plane in &self.planes {
            // Find positive vertex in direction of normal
            let px = if plane.normal.x >= 0.0 {
                aabb.max.x
            } else {
                aabb.min.x
            };
            let py = if plane.normal.y >= 0.0 {
                aabb.max.y
            } else {
                aabb.min.y
            };
            let pz = if plane.normal.z >= 0.0 {
                aabb.max.z
            } else {
                aabb.min.z
            };

            if plane.distance_to_point(Vec3::new(px, py, pz)) < 0.0 {
                return false;
            }
        }
        true
    }

    /// Fast test for a terrain chunk with given horizontal 2D bounds and Y range.
    #[inline]
    pub fn intersects_chunk(
        &self,
        min_x: f32,
        min_z: f32,
        size: f32,
        min_y: f32,
        max_y: f32,
    ) -> bool {
        let aabb = AABB {
            min: Vec3::new(min_x, min_y, min_z),
            max: Vec3::new(min_x + size, max_y, min_z + size),
        };
        self.intersects_aabb(&aabb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera3d::Camera3D;

    #[test]
    fn test_reversed_z_frustum_near_plane_and_negative_y() {
        // Camera at Y = -10.0 looking along -Z
        let cam = Camera3D::new(
            Vec3::new(0.0, -10.0, 0.0),
            Vec3::new(0.0, -10.0, -1.0),
            16.0 / 9.0,
        );
        let frustum = cam.frustum();

        // Geometry at Y = -10.0 directly in front of camera (Z = -5.0) must be visible
        assert!(frustum.intersects_sphere(&BoundingSphere::new(Vec3::new(0.0, -10.0, -5.0), 1.0)));
        assert!(frustum.intersects_chunk(-2.0, -7.0, 4.0, -12.0, -8.0));

        // Geometry behind camera (Z = +5.0) must be culled
        assert!(!frustum.intersects_sphere(&BoundingSphere::new(Vec3::new(0.0, -10.0, 5.0), 1.0)));
        assert!(!frustum.intersects_chunk(-2.0, 3.0, 4.0, -12.0, -8.0));
    }
}
