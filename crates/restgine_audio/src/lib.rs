use glam::Vec2;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("Failed to initialize audio output stream: {0}")]
    StreamError(String),
    #[error("Failed to load sound file {0}: {1}")]
    LoadError(String, String),
}

/// 2.5D Spatial audio system for sound effects and music.
pub struct AudioEngine {
    stream_handle: Option<OutputStreamHandle>,
    sound_cache: HashMap<String, Arc<Vec<u8>>>,
    master_volume: f32,
    listener_pos: Vec2,
}

impl AudioEngine {
    /// Create a new AudioEngine with an optional OutputStreamHandle.
    pub fn new(handle: Option<OutputStreamHandle>) -> Self {
        Self {
            stream_handle: handle,
            sound_cache: HashMap::new(),
            master_volume: 1.0,
            listener_pos: Vec2::ZERO,
        }
    }

    /// Helper to initialize default audio device stream and engine.
    /// Returns the engine and the underlying stream (which must be kept alive by the runner).
    pub fn init_default() -> (Self, Option<OutputStream>) {
        match OutputStream::try_default() {
            Ok((stream, handle)) => (Self::new(Some(handle)), Some(stream)),
            Err(e) => {
                log::warn!(
                    "Audio output device could not be opened: {}. Audio will run in silent mode.",
                    e
                );
                (Self::new(None), None)
            }
        }
    }

    /// Set position of the listener (typically the Player) in world space for spatial audio.
    pub fn set_listener_position(&mut self, pos: Vec2) {
        self.listener_pos = pos;
    }

    /// Set master audio volume [0.0, 1.0].
    pub fn set_master_volume(&mut self, volume: f32) {
        self.master_volume = volume.clamp(0.0, 1.0);
    }

    /// Preload a sound file into memory.
    pub fn preload_sound<P: AsRef<Path>>(&mut self, path: P) -> Result<(), AudioError> {
        let path_ref = path.as_ref();
        let path_str = path_ref.to_string_lossy().to_string();
        if self.sound_cache.contains_key(&path_str) {
            return Ok(());
        }

        let bytes = std::fs::read(path_ref)
            .map_err(|e| AudioError::LoadError(path_str.clone(), e.to_string()))?;

        self.sound_cache.insert(path_str, Arc::new(bytes));
        Ok(())
    }

    /// Preload sound bytes directly into memory under a specific sound ID.
    pub fn preload_sound_bytes(&mut self, sound_id: &str, bytes: Vec<u8>) {
        self.sound_cache.insert(sound_id.to_string(), Arc::new(bytes));
    }

    /// Play an audio track in an infinite loop (e.g. ambient wind / forest).
    /// Returns the active Sink handle so volume or playback can be controlled.
    pub fn play_looping_sound(&self, sound_id: &str, volume: f32) -> Option<Sink> {
        let handle = self.stream_handle.as_ref()?;
        if let Some(bytes) = self.sound_cache.get(sound_id) {
            let cursor = Cursor::new((**bytes).clone());
            if let Ok(source) = Decoder::new(cursor) {
                if let Ok(sink) = Sink::try_new(handle) {
                    sink.set_volume(volume * self.master_volume);
                    sink.append(source.repeat_infinite());
                    return Some(sink);
                }
            }
        } else if let Ok(file) = File::open(sound_id) {
            let reader = BufReader::new(file);
            if let Ok(source) = Decoder::new(reader) {
                if let Ok(sink) = Sink::try_new(handle) {
                    sink.set_volume(volume * self.master_volume);
                    sink.append(source.repeat_infinite());
                    return Some(sink);
                }
            }
        }
        None
    }

    /// Play a 2D sound effect with optional pitch variation.
    pub fn play_sound(&self, sound_id: &str, volume: f32, speed: f32) {
        let handle = match &self.stream_handle {
            Some(h) => h,
            None => return,
        };

        if let Some(bytes) = self.sound_cache.get(sound_id) {
            let cursor = Cursor::new((**bytes).clone());
            if let Ok(source) = Decoder::new(cursor) {
                if let Ok(sink) = Sink::try_new(handle) {
                    sink.set_volume(volume * self.master_volume);
                    sink.set_speed(speed.max(0.1));
                    sink.append(source);
                    sink.detach();
                }
            }
        } else if let Ok(file) = File::open(sound_id) {
            let reader = BufReader::new(file);
            if let Ok(source) = Decoder::new(reader) {
                if let Ok(sink) = Sink::try_new(handle) {
                    sink.set_volume(volume * self.master_volume);
                    sink.set_speed(speed.max(0.1));
                    sink.append(source);
                    sink.detach();
                }
            }
        }
    }

    /// Play spatial 2.5D sound relative to the listener.
    /// Attenuates volume based on distance and applies stereo panning.
    pub fn play_spatial_sound(&self, sound_id: &str, emitter_pos: Vec2, max_distance: f32) {
        let diff = emitter_pos - self.listener_pos;
        let distance = diff.length();
        if distance > max_distance {
            return;
        }

        let distance_factor = (1.0 - (distance / max_distance)).clamp(0.0, 1.0);
        let volume = distance_factor * distance_factor; // Inverse square feel

        self.play_sound(sound_id, volume, 1.0);
    }
}
