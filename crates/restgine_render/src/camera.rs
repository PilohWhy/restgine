use glam::{Mat4, Vec2};

/// Responsive pixel art camera supporting integer scaling and dynamic aspect ratios
/// (Steam Deck 16:10, standard 16:9, Ultrawide 21:9) without black letterbox bars.
#[derive(Debug, Clone)]
pub struct PixelCamera {
    pub position: Vec2,
    pub native_height: u32,
    pub scale: u32,
    pub virtual_width: f32,
    pub virtual_height: f32,
}

impl PixelCamera {
    pub fn new(native_height: u32) -> Self {
        Self {
            position: Vec2::ZERO,
            native_height: native_height.max(180),
            scale: 1,
            virtual_width: 480.0,
            virtual_height: 270.0,
        }
    }

    /// Update screen resolution and recalculate integer scale factor and responsive viewport.
    pub fn update_window_size(&mut self, window_width: u32, window_height: u32) {
        let calculated_scale = (window_height / self.native_height).max(1);
        self.scale = calculated_scale;

        // Dynamic virtual resolution that fits the exact aspect ratio
        self.virtual_width = (window_width as f32 / calculated_scale as f32).ceil();
        self.virtual_height = (window_height as f32 / calculated_scale as f32).ceil();
    }

    /// Compute orthographic projection matrix.
    pub fn projection_matrix(&self) -> Mat4 {
        let half_w = self.virtual_width * 0.5;
        let half_h = self.virtual_height * 0.5;

        // Pixel-snapped camera position to eliminate subpixel jitter
        let snapped_cam_x = self.position.x.round();
        let snapped_cam_y = self.position.y.round();

        let left = snapped_cam_x - half_w;
        let right = snapped_cam_x + half_w;
        let bottom = snapped_cam_y + half_h;
        let top = snapped_cam_y - half_h;

        Mat4::orthographic_rh(left, right, bottom, top, -1000.0, 1000.0)
    }

    /// Convert screen pixel coordinates to world coordinates.
    pub fn screen_to_world(&self, screen_x: f32, screen_y: f32) -> Vec2 {
        let half_w = self.virtual_width * 0.5;
        let half_h = self.virtual_height * 0.5;
        let vx = screen_x / self.scale as f32;
        let vy = screen_y / self.scale as f32;

        Vec2::new(self.position.x - half_w + vx, self.position.y - half_h + vy)
    }
}
