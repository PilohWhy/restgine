use glam::{Mat4, Vec3, Vec3A};

/// Full 3D 6-DOF camera supporting reversed-Z infinite perspective projection.
/// Reversed-Z maps near plane to Z=1.0 and far plane to Z=0.0 in clip space,
/// granting uniform floating-point depth distribution and eliminating Z-fighting.
#[derive(Debug, Clone)]
pub struct Camera3D {
    pub eye: Vec3A,
    pub target: Vec3A,
    pub up: Vec3A,
    pub fov_y_rad: f32,
    pub aspect_ratio: f32,
    pub z_near: f32,
}

impl Camera3D {
    pub fn new(eye: Vec3, target: Vec3, aspect_ratio: f32) -> Self {
        Self {
            eye: Vec3A::from(eye),
            target: Vec3A::from(target),
            up: Vec3A::new(0.0, 1.0, 0.0),
            fov_y_rad: 60.0_f32.to_radians(),
            aspect_ratio: aspect_ratio.max(0.01),
            z_near: 0.1,
        }
    }

    pub fn set_aspect_ratio(&mut self, width: u32, height: u32) {
        if height > 0 {
            self.aspect_ratio = width as f32 / height as f32;
        }
    }

    /// Compute right-handed LookAt view matrix.
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(
            Vec3::from(self.eye),
            Vec3::from(self.target),
            Vec3::from(self.up),
        )
    }

    /// Compute Reversed-Z infinite perspective projection matrix.
    /// In Reversed-Z:
    /// Clip Z = 1.0 at z_near, Clip Z = 0.0 at infinity.
    pub fn projection_matrix(&self) -> Mat4 {
        let f = 1.0 / (self.fov_y_rad * 0.5).tan();
        let ar = self.aspect_ratio;
        let n = self.z_near;

        // Reversed-Z infinite perspective RH:
        // [ f / ar, 0,  0,  0 ]
        // [ 0,      f,  0,  0 ]
        // [ 0,      0,  0, -1 ]
        // [ 0,      0,  n,  0 ]
        Mat4::from_cols_array(&[
            f / ar,
            0.0,
            0.0,
            0.0,
            0.0,
            f,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -1.0,
            0.0,
            0.0,
            n,
            0.0,
        ])
    }

    /// Combined View-Projection matrix for GPU uniforms.
    pub fn view_proj_matrix(&self) -> Mat4 {
        self.projection_matrix() * self.view_matrix()
    }
}
