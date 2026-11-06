use glam::Vec2;
use image::{Rgba, RgbaImage};
use std::collections::HashMap;

/// A sub-region within a packed texture atlas.
#[derive(Debug, Clone)]
pub struct AtlasRegion {
    pub uv_rect: [f32; 4], // [u_min, v_min, u_max, v_max]
    pub pixel_size: Vec2,
}

/// Automatic sprite atlas packer with 1px border extrusion to eliminate
/// pixel art edge bleeding during camera subpixel movements.
pub struct TextureAtlasBuilder {
    images: HashMap<String, RgbaImage>,
    padding: u32,
}

impl Default for TextureAtlasBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TextureAtlasBuilder {
    pub fn new() -> Self {
        Self {
            images: HashMap::new(),
            padding: 2, // 1px extrusion + 1px spacing
        }
    }

    pub fn add_image(&mut self, id: String, img: RgbaImage) {
        self.images.insert(id, img);
    }

    /// Build a single combined texture atlas image and compute UV coordinates.
    pub fn build(&self) -> (RgbaImage, HashMap<String, AtlasRegion>) {
        if self.images.is_empty() {
            let empty = RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255]));
            let mut map = HashMap::new();
            map.insert(
                "default".to_string(),
                AtlasRegion {
                    uv_rect: [0.0, 0.0, 1.0, 1.0],
                    pixel_size: Vec2::new(1.0, 1.0),
                },
            );
            return (empty, map);
        }

        // Simple row packing algorithm
        let atlas_width = 1024u32;
        let mut current_x = self.padding;
        let mut current_y = self.padding;
        let mut row_max_h = 0u32;
        let mut placements = HashMap::new();

        for (id, img) in &self.images {
            let (w, h) = img.dimensions();
            if current_x + w + self.padding > atlas_width {
                current_x = self.padding;
                current_y += row_max_h + self.padding;
                row_max_h = 0;
            }

            placements.insert(id.clone(), (current_x, current_y, w, h));
            current_x += w + self.padding;
            row_max_h = row_max_h.max(h);
        }

        let atlas_height = (current_y + row_max_h + self.padding)
            .next_power_of_two()
            .max(128);
        let mut atlas_image = RgbaImage::from_pixel(atlas_width, atlas_height, Rgba([0, 0, 0, 0]));
        let mut regions = HashMap::new();

        for (id, (px, py, w, h)) in placements {
            let source_img = &self.images[&id];

            // Copy main sprite pixels
            for y in 0..h {
                for x in 0..w {
                    atlas_image.put_pixel(px + x, py + y, *source_img.get_pixel(x, y));
                }
            }

            // 1px Border Extrusion: copy edge pixels into adjacent 1px borders
            for x in 0..w {
                // Top border
                atlas_image.put_pixel(px + x, py - 1, *source_img.get_pixel(x, 0));
                // Bottom border
                atlas_image.put_pixel(px + x, py + h, *source_img.get_pixel(x, h - 1));
            }
            for y in 0..h {
                // Left border
                atlas_image.put_pixel(px - 1, py + y, *source_img.get_pixel(0, y));
                // Right border
                atlas_image.put_pixel(px + w, py + y, *source_img.get_pixel(w - 1, y));
            }

            let u_min = px as f32 / atlas_width as f32;
            let v_min = py as f32 / atlas_height as f32;
            let u_max = (px + w) as f32 / atlas_width as f32;
            let v_max = (py + h) as f32 / atlas_height as f32;

            regions.insert(
                id,
                AtlasRegion {
                    uv_rect: [u_min, v_min, u_max, v_max],
                    pixel_size: Vec2::new(w as f32, h as f32),
                },
            );
        }

        (atlas_image, regions)
    }
}
