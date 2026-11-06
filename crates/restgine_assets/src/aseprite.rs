use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsepriteRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsepriteFrame {
    pub frame: AsepriteRect,
    pub duration: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsepriteTag {
    pub name: String,
    pub from: usize,
    pub to: usize,
    pub direction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsepriteMeta {
    pub image: String,
    pub size: AsepriteSize,
    #[serde(default, rename = "frameTags")]
    pub frame_tags: Vec<AsepriteTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsepriteSize {
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsepriteSheet {
    pub frames: Vec<AsepriteFrame>,
    pub meta: AsepriteMeta,
}

/// Structured runtime animation clip parsed from Aseprite.
#[derive(Debug, Clone)]
pub struct AnimationClip {
    pub name: String,
    pub frame_indices: Vec<usize>,
    pub frame_durations: Vec<f32>,
}

impl AsepriteSheet {
    pub fn from_json_str(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Extract all animation clips mapped by their tag name.
    pub fn extract_clips(&self) -> HashMap<String, AnimationClip> {
        let mut clips = HashMap::new();

        for tag in &self.meta.frame_tags {
            let mut indices = Vec::new();
            let mut durations = Vec::new();

            for i in tag.from..=tag.to {
                if i < self.frames.len() {
                    indices.push(i);
                    durations.push(self.frames[i].duration as f32 / 1000.0);
                }
            }

            clips.insert(
                tag.name.clone(),
                AnimationClip {
                    name: tag.name.clone(),
                    frame_indices: indices,
                    frame_durations: durations,
                },
            );
        }

        clips
    }
}
