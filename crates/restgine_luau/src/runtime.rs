use crate::bindings::{register_engine_api, EngineContext};
use mlua::Lua;
use std::path::{Path, PathBuf};

pub struct LuauRuntime {
    lua: Lua,
    main_script_path: PathBuf,
}

impl LuauRuntime {
    pub fn new<P: AsRef<Path>>(main_script: P, ctx: EngineContext) -> mlua::Result<Self> {
        let lua = Lua::new();
        register_engine_api(&lua, ctx)?;

        let path = main_script.as_ref().to_path_buf();
        let mut runtime = Self {
            lua,
            main_script_path: path,
        };

        runtime.reload()?;
        Ok(runtime)
    }

    /// Execute or reload the main script.
    pub fn reload(&mut self) -> mlua::Result<()> {
        if self.main_script_path.exists() {
            let code = std::fs::read_to_string(&self.main_script_path)
                .map_err(|e| mlua::Error::RuntimeError(format!("Failed to read script: {}", e)))?;
            self.lua.load(&code).set_name("main.luau").exec()?;
            log::info!(
                "Successfully loaded/reloaded Luau script: {:?}",
                self.main_script_path
            );
        } else {
            log::warn!(
                "Luau script does not exist yet at {:?}",
                self.main_script_path
            );
        }
        Ok(())
    }

    /// Invoke `init()` hook if defined in global Luau scope.
    pub fn call_init(&self) -> mlua::Result<()> {
        let globals = self.lua.globals();
        if let Ok(func) = globals.get::<_, mlua::Function>("init") {
            func.call::<_, ()>(())?;
        }
        Ok(())
    }

    /// Invoke `update(dt)` hook.
    pub fn call_update(&self, dt: f32) -> mlua::Result<()> {
        let globals = self.lua.globals();
        if let Ok(func) = globals.get::<_, mlua::Function>("update") {
            func.call::<_, ()>(dt)?;
        }
        Ok(())
    }

    /// Invoke `render(alpha)` hook.
    pub fn call_render(&self, alpha: f32) -> mlua::Result<()> {
        let globals = self.lua.globals();
        if let Ok(func) = globals.get::<_, mlua::Function>("render") {
            func.call::<_, ()>(alpha)?;
        }
        Ok(())
    }
}
