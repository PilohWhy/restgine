use glam::{Affine3A, Mat4, Quat, Vec2, Vec3, Vec3A};
use serde::{Deserialize, Serialize};

/// 64-byte cache-line aligned 3D Affine Transform representing both 3D entities
/// and planar 2D/2.5D projections without engine branching.
#[repr(align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform3D {
    pub translation: Vec3A,
    pub rotation: Quat,
    pub scale: Vec3A,
    pub previous_translation: Vec3A,
}

impl Transform3D {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        let t = Vec3A::new(x, y, z);
        Self {
            translation: t,
            rotation: Quat::IDENTITY,
            scale: Vec3A::ONE,
            previous_translation: t,
        }
    }

    pub fn from_translation_rotation(t: Vec3, r: Quat) -> Self {
        let ta = Vec3A::from(t);
        Self {
            translation: ta,
            rotation: r,
            scale: Vec3A::ONE,
            previous_translation: ta,
        }
    }

    /// Construct 4x4 matrix representation.
    pub fn to_mat4(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::from(self.scale),
            self.rotation,
            Vec3::from(self.translation),
        )
    }

    /// Construct Affine3A fast representation for SIMD vector transformations.
    pub fn to_affine(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(
            Vec3::from(self.scale),
            self.rotation,
            Vec3::from(self.translation),
        )
    }

    /// Interpolated translation between previous physics tick and current physics tick
    /// based on sub-frame alpha factor [0.0, 1.0].
    pub fn interpolated_translation(&self, alpha: f32) -> Vec3A {
        self.previous_translation
            .lerp(self.translation, alpha.clamp(0.0, 1.0))
    }

    /// Interpolated 4x4 model matrix representation for smooth uncapped rendering.
    pub fn interpolated_mat4(&self, alpha: f32) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::from(self.scale),
            self.rotation,
            Vec3::from(self.interpolated_translation(alpha)),
        )
    }

    /// 2D / 2.5D Screen coordinates projection for isometric or top-down games:
    /// screen_x = x, screen_y = y - z.
    pub fn screen_coords_25d(&self) -> Vec2 {
        Vec2::new(self.translation.x, self.translation.y - self.translation.z)
    }

    /// Ground Y footprint coordinate for Y-sorting in 2.5D.
    pub fn ground_y(&self) -> f32 {
        self.translation.y
    }
}

impl Default for Transform3D {
    fn default() -> Self {
        Self {
            translation: Vec3A::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3A::ONE,
            previous_translation: Vec3A::ZERO,
        }
    }
}
