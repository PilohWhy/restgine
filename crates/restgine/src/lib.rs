//! # Restgine
//! High-performance universal 2D/3D continuum game engine for Steam.
//!
//! Restgine provides a modular architecture:
//! - [`core`]: High-frequency fixed timestep clock (60Hz default), input state, config
//! - [`ecs`]: Cache-friendly archetypal Entity Component System (via `hecs`)
//! - [`physics`]: Deterministic 2.5D/3D collision detection and spatial hash grids
//! - [`render`]: Next-gen Vulkan/WGPU rendering (3D terrain, dense foliage, forest, sky, 2D batching)
//! - [`audio`]: Zero-latency spatial 2.5D/3D and ambient audio engine (via `rodio`)
//! - [`assets`]: Virtual File System (VFS) with packed `.pak` archives and encryption
//! - [`luau`]: Sandboxed hot-reloading scripting with Luau
//! - [`steam`]: Native Steamworks integration with achievements, stats, cloud saves

pub use restgine_assets as assets;
pub use restgine_audio as audio;
pub use restgine_core as core;
pub use restgine_ecs as ecs;
pub use restgine_luau as luau;
pub use restgine_physics as physics;
pub use restgine_render as render;
pub use restgine_steam as steam;

// Re-exports of foundational math & runtime crates
pub use bytemuck;
pub use glam;
pub use hecs;
pub use image;
pub use log;
pub use wgpu;
pub use winit;

/// Ergonomic prelude re-exporting the most common engine types and traits.
pub mod prelude {
    pub use restgine_assets::VfsPacker;
    pub use restgine_audio::{AudioEngine, AudioError};
    pub use restgine_core::time::GameClock;
    pub use restgine_core::{GameConfig, InputState, RenderDimension, ThreadScratchpad};
    pub use restgine_ecs::{create_world, Entity, World};
    pub use hecs;
    pub use restgine_luau::{EngineContext, LuauRuntime};
    pub use restgine_physics::{step_physics_25d, SpatialHashGrid};
    pub use restgine_render::camera::PixelCamera;
    pub use restgine_render::camera3d::Camera3D;
    pub use restgine_render::culling::{BoundingSphere, Frustum, Plane, AABB};
    pub use restgine_render::foliage::{
        scatter_chunk_grass, FoliagePipeline, GrassInstanceRaw, GrassVertex,
    };
    pub use restgine_render::forest::{
        scatter_chunk_forest, ForestInstanceRaw, ForestPipeline, ForestVariant, ForestVertex,
        TreeCollider,
    };
    pub use restgine_render::mesh3d::{
        CameraSettingsInput, Mesh3DInstanceRaw, Mesh3DPipeline, Mesh3DVertex,
    };
    pub use restgine_render::pipeline::{CameraUniform, SpritePipeline};
    pub use restgine_render::sky::SkyPipeline;
    pub use restgine_render::terrain::{
        terrain_height, terrain_normal, TerrainChunkInstance, TerrainPipeline, TerrainVertex,
    };
    pub use restgine_render::window::WindowContext;
    pub use restgine_render::{IsaacRenderBatch, RenderQueueItem, SpriteInstanceRaw};
    pub use restgine_steam::SteamContext;

    pub use glam::{vec2, vec3, Mat4, Quat, Vec2, Vec3, Vec3A, Vec4};
}
