use glam::Vec2;
use hecs::{Entity, World};
use mlua::{FromLua, Lua, UserData, UserDataMethods, Value};
use restgine_audio::AudioEngine;
use restgine_core::InputState;
use restgine_ecs::{Footprint, RenderLayer, Sprite, Transform25D, Velocity25D};
use restgine_render::PixelCamera;
use restgine_steam::SteamContext;
use std::sync::{Arc, Mutex};

/// Luau userdata wrapper around an ECS Entity.
#[derive(Clone, Copy)]
pub struct LuaEntity {
    pub entity: Entity,
}

impl UserData for LuaEntity {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(_methods: &mut M) {}
}

impl<'lua> FromLua<'lua> for LuaEntity {
    fn from_lua(value: Value<'lua>, _lua: &'lua Lua) -> mlua::Result<Self> {
        match value {
            Value::UserData(ud) => Ok(*ud.borrow::<Self>()?),
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "LuaEntity",
                message: Some("expected entity userdata".into()),
            }),
        }
    }
}

/// Shared engine context exposed to Luau scripts.
pub struct EngineContext {
    pub world: Arc<Mutex<World>>,
    pub input: Arc<Mutex<InputState>>,
    pub audio: Arc<Mutex<AudioEngine>>,
    pub camera: Arc<Mutex<PixelCamera>>,
    pub steam: Arc<Mutex<SteamContext>>,
}

pub fn register_engine_api(lua: &Lua, ctx: EngineContext) -> mlua::Result<()> {
    let globals = lua.globals();
    let restgine_table = lua.create_table()?;

    // Helper macro / function to handle both dot (.) and colon (:) method calls
    // When called as obj:method(arg), args tuple is (self, arg)

    // --- Entity Spawning ---
    let world_arc = ctx.world.clone();
    restgine_table.set(
        "spawn",
        lua.create_function(move |_, (_self, params): (Value, mlua::Table)| {
            let x: f32 = params.get("x").unwrap_or(0.0);
            let y: f32 = params.get("y").unwrap_or(0.0);
            let z: f32 = params.get("z").unwrap_or(0.0);
            let radius: f32 = params.get("radius").unwrap_or(8.0);
            let z_height: f32 = params.get("z_height").unwrap_or(16.0);
            let is_solid: bool = params.get("is_solid").unwrap_or(true);
            let texture: String = params.get("texture").unwrap_or_default();

            let layer_str: String = params.get("layer").unwrap_or_else(|_| "ground_entities".to_string());
            let layer = match layer_str.as_str() {
                "floor" => RenderLayer::Floor,
                "decals" => RenderLayer::GroundDecals,
                "airborne" => RenderLayer::Airborne,
                "foreground" => RenderLayer::Foreground,
                "ui" => RenderLayer::UserInterface,
                _ => RenderLayer::GroundEntities,
            };

            let mut world = world_arc.lock().unwrap();
            let entity = world.spawn((
                Transform25D::new(x, y, z),
                Velocity25D::zero(),
                Footprint::circle(radius, z_height).with_solid(is_solid),
                Sprite {
                    texture_id: texture,
                    layer,
                    ..Default::default()
                },
            ));

            Ok(LuaEntity { entity })
        })?,
    )?;

    // --- Entity Manipulation ---
    let world_arc = ctx.world.clone();
    restgine_table.set(
        "set_velocity",
        lua.create_function(move |_, (_self, handle, vx, vy, vz): (Value, LuaEntity, f32, f32, Option<f32>)| {
            let world = world_arc.lock().unwrap();
            if let Ok(mut vel) = world.get::<&mut Velocity25D>(handle.entity) {
                vel.velocity.x = vx;
                vel.velocity.y = vy;
                if let Some(z) = vz {
                    vel.velocity.z = z;
                }
            }
            Ok(())
        })?,
    )?;

    let world_arc = ctx.world.clone();
    restgine_table.set(
        "get_position",
        lua.create_function(move |_, (_self, handle): (Value, LuaEntity)| {
            let world = world_arc.lock().unwrap();
            let mut pos = (0.0, 0.0, 0.0);
            if let Ok(transform) = world.get::<&Transform25D>(handle.entity) {
                pos = (transform.position.x, transform.position.y, transform.position.z);
            }
            Ok(pos)
        })?,
    )?;

    let world_arc = ctx.world.clone();
    restgine_table.set(
        "destroy_entity",
        lua.create_function(move |_, (_self, handle): (Value, LuaEntity)| {
            let mut world = world_arc.lock().unwrap();
            let _ = world.despawn(handle.entity);
            Ok(())
        })?,
    )?;

    // --- Input System ---
    let input_arc = ctx.input.clone();
    restgine_table.set(
        "is_key_down",
        lua.create_function(move |_, (_self, key): (Value, String)| {
            let input = input_arc.lock().unwrap();
            Ok(input.is_key_down(&key))
        })?,
    )?;

    let input_arc = ctx.input.clone();
    restgine_table.set(
        "get_vector",
        lua.create_function(move |_, (_self, neg_x, pos_x, neg_y, pos_y): (Value, String, String, String, String)| {
            let input = input_arc.lock().unwrap();
            let vec = input.get_vector(&neg_x, &pos_x, &neg_y, &pos_y);
            Ok((vec.x, vec.y))
        })?,
    )?;

    // --- Audio System ---
    let audio_arc = ctx.audio.clone();
    restgine_table.set(
        "play_sound",
        lua.create_function(move |_, (_self, path, volume, speed): (Value, String, Option<f32>, Option<f32>)| {
            let audio = audio_arc.lock().unwrap();
            audio.play_sound(&path, volume.unwrap_or(1.0), speed.unwrap_or(1.0));
            Ok(())
        })?,
    )?;

    let audio_arc = ctx.audio.clone();
    restgine_table.set(
        "play_spatial_sound",
        lua.create_function(move |_, (_self, path, x, y, max_dist): (Value, String, f32, f32, Option<f32>)| {
            let audio = audio_arc.lock().unwrap();
            audio.play_spatial_sound(&path, Vec2::new(x, y), max_dist.unwrap_or(300.0));
            Ok(())
        })?,
    )?;

    let audio_arc = ctx.audio.clone();
    restgine_table.set(
        "set_listener_position",
        lua.create_function(move |_, (_self, x, y): (Value, f32, f32)| {
            let mut audio = audio_arc.lock().unwrap();
            audio.set_listener_position(Vec2::new(x, y));
            Ok(())
        })?,
    )?;

    // --- Camera System ---
    let camera_arc = ctx.camera.clone();
    restgine_table.set(
        "set_camera_position",
        lua.create_function(move |_, (_self, x, y): (Value, f32, f32)| {
            let mut cam = camera_arc.lock().unwrap();
            cam.position = Vec2::new(x, y);
            Ok(())
        })?,
    )?;

    // --- Steamworks System ---
    let steam_arc = ctx.steam.clone();
    restgine_table.set(
        "unlock_achievement",
        lua.create_function(move |_, (_self, name): (Value, String)| {
            let steam = steam_arc.lock().unwrap();
            Ok(steam.unlock_achievement(&name))
        })?,
    )?;

    let steam_arc = ctx.steam.clone();
    restgine_table.set(
        "is_steam_deck",
        lua.create_function(move |_, _self: Value| {
            let steam = steam_arc.lock().unwrap();
            Ok(steam.is_steam_deck())
        })?,
    )?;

    // --- Logging ---
    restgine_table.set(
        "log_info",
        lua.create_function(|_, (_self, msg): (Value, String)| {
            log::info!("[Luau] {}", msg);
            Ok(())
        })?,
    )?;

    globals.set("restgine", restgine_table)?;
    Ok(())
}

trait FootprintExt {
    fn with_solid(self, solid: bool) -> Self;
}

impl FootprintExt for Footprint {
    fn with_solid(mut self, solid: bool) -> Self {
        self.is_solid = solid;
        self
    }
}
