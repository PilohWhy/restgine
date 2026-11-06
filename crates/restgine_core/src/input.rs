use glam::Vec2;
use std::collections::{HashMap, HashSet};

/// Input action state tracker supporting digital buttons and 2D analog axes.
#[derive(Debug, Default, Clone)]
pub struct InputState {
    keys_down: HashSet<String>,
    keys_just_pressed: HashSet<String>,
    keys_just_released: HashSet<String>,
    axes: HashMap<String, f32>,
    mouse_position: Vec2,
    mouse_buttons_down: HashSet<u8>,
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset frame-specific triggers (just_pressed / just_released).
    pub fn clear_frame_state(&mut self) {
        self.keys_just_pressed.clear();
        self.keys_just_released.clear();
    }

    pub fn set_key_down(&mut self, key: &str, down: bool) {
        let key_str = key.to_lowercase();
        if down {
            if !self.keys_down.contains(&key_str) {
                self.keys_just_pressed.insert(key_str.clone());
            }
            self.keys_down.insert(key_str);
        } else {
            if self.keys_down.contains(&key_str) {
                self.keys_just_released.insert(key_str.clone());
            }
            self.keys_down.remove(&key_str);
        }
    }

    pub fn is_key_down(&self, key: &str) -> bool {
        self.keys_down.contains(&key.to_lowercase())
    }

    pub fn is_key_just_pressed(&self, key: &str) -> bool {
        self.keys_just_pressed.contains(&key.to_lowercase())
    }

    pub fn is_key_just_released(&self, key: &str) -> bool {
        self.keys_just_released.contains(&key.to_lowercase())
    }

    pub fn set_axis(&mut self, axis_name: &str, value: f32) {
        self.axes
            .insert(axis_name.to_lowercase(), value.clamp(-1.0, 1.0));
    }

    pub fn get_axis(&self, axis_name: &str) -> f32 {
        self.axes
            .get(&axis_name.to_lowercase())
            .copied()
            .unwrap_or(0.0)
    }

    /// Get normalized 2D movement vector from four digital or analog inputs.
    pub fn get_vector(
        &self,
        negative_x: &str,
        positive_x: &str,
        negative_y: &str,
        positive_y: &str,
    ) -> Vec2 {
        let mut x = 0.0;
        let mut y = 0.0;

        if self.is_key_down(negative_x) {
            x -= 1.0;
        }
        if self.is_key_down(positive_x) {
            x += 1.0;
        }
        if self.is_key_down(negative_y) {
            y -= 1.0;
        }
        if self.is_key_down(positive_y) {
            y += 1.0;
        }

        // Add analog axes if present
        x += self.get_axis("gamepad_left_x");
        y += self.get_axis("gamepad_left_y");

        let vec = Vec2::new(x, y);
        if vec.length_squared() > 1.0 {
            vec.normalize()
        } else {
            vec
        }
    }

    pub fn set_mouse_button_down(&mut self, button: u8, down: bool) {
        if down {
            self.mouse_buttons_down.insert(button);
        } else {
            self.mouse_buttons_down.remove(&button);
        }
    }

    pub fn is_mouse_button_down(&self, button: u8) -> bool {
        self.mouse_buttons_down.contains(&button)
    }

    pub fn set_mouse_position(&mut self, pos: Vec2) {
        self.mouse_position = pos;
    }

    pub fn mouse_position(&self) -> Vec2 {
        self.mouse_position
    }
}
