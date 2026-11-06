# Restgine Developer Guide

This guide walks you through building games with Restgine, from setting up your first project to deploying on Steam.

---

## 1. Project Anatomy

When you create a project via `restgine init my_game`, the CLI generates the following structure:

```
my_game/
├── game.toml           # Engine configuration (window, target fps, Steam AppID)
├── assets/
│   ├── sprites/        # PNG sprite sheets and Aseprite JSON metadata
│   ├── models/         # 3D meshes (OBJ/GLTF) and precompiled clusters
│   └── audio/          # WAV and OGG sound effects and music
└── scripts/
    ├── main.luau       # Main game loop entry point
    └── restgine.d.luau # Luau LSP type definitions for your IDE
```

---

## 2. Configuration (`game.toml`)

Configure your game window, dimension continuum mode, and Steam settings:

```toml
title = "My Dungeon Crawler"
version = "0.1.0"
steam_app_id = 480
dimension = "Full3D" # Options: "Dimensional2D", "Isometric25D", "Full3D"

[window]
width = 1280
height = 720
fullscreen = false
resizable = true
vsync = false # Set true for capped 60Hz, false for uncapped frame delivery

[viewport]
native_height = 270 # Virtual pixel height for responsive integer scaling
min_scale = 1
target_physics_hz = 60
uncapped_framerate = true
```

---

## 3. Writing Game Logic in Luau

Restgine embeds a sandboxed Luau runtime. Your `main.luau` script drives game initialization, fixed timestep updates, and render interpolation.

```luau
--!strict
local player = nil
local shoot_timer = 0

function init()
    -- Spawn player entity on ground plane (x=0, y=0, z=0)
    player = restgine:spawn({
        x = 0,
        y = 0,
        z = 0,
        radius = 8,
        z_height = 16,
        is_solid = true,
        layer = "ground_entities"
    })

    -- Unlock introductory achievement if running under Steam
    restgine:unlock_achievement("START_GAME")
end

function update(dt: number)
    if not player then return end

    -- 4-directional movement using WASD or Gamepad
    local move_x, move_y = restgine:get_vector("a", "d", "w", "s")
    local speed = 140
    restgine:set_velocity(player, move_x * speed, move_y * speed, 0)

    -- Center camera on player
    local px, py, pz = restgine:get_position(player)
    restgine:set_camera_position(px, py)

    -- Projectile firing with arrow keys
    shoot_timer -= dt
    local shoot_x, shoot_y = restgine:get_vector("left", "right", "up", "down")
    if (shoot_x ~= 0 or shoot_y ~= 0) and shoot_timer <= 0 then
        shoot_timer = 0.2
        restgine:play_sound("assets/audio/shoot.wav", 0.8, 1.0)

        -- Spawn projectile at chest height (z = 8)
        local tear = restgine:spawn({
            x = px,
            y = py,
            z = 8,
            radius = 4,
            z_height = 6,
            is_solid = false,
            layer = "airborne"
        })
        restgine:set_velocity(tear, shoot_x * 240, shoot_y * 240, 10)
    end
end
```

---

## 4. Understanding the Continuum Pipeline

### 2D vs 2.5D vs Full 3D
Restgine uses a single unified math kernel. You do not need separate engine modes for 2D and 3D:
- **2D Pixel Art**: Operates at $z=0$. Rendered with integer pixel-snapping to prevent subpixel edge shimmering.
- **2.5D Isometric (The Binding of Isaac style)**: Entities move along a ground plane $(x, y)$ with an elevation coordinate $z$. Sorting occurs automatically along the ground footprint $y$.
- **Full 3D**: Rigid bodies utilize the sub-stepped XPBD solver with continuous gravity and restitution. Rendered through the 64-bit Visibility Buffer pipeline.

### The 64-bit Visibility Buffer
Traditional deferred rendering writes large G-Buffers (normals, roughness, albedo, depth) costing 32-64 bytes per pixel. Restgine writes an 8-byte target (`u32 instance_id`, `u32 primitive_id`). Shading is evaluated in a deferred compute pass only once for visible pixels, completely eliminating quad overdraw.

---

## 5. Steamworks Integration

Restgine includes out-of-the-box Steamworks support:
- **Steam Input**: Automatically maps gamepad axes and buttons.
- **Steam Cloud Saves**: Save and read player save files synchronously through `ISteamRemoteStorage`.
- **Steam Deck**: Detects Steam Deck hardware at startup to optimize power draw and integer scaling.

---

## 6. Building for Distribution

To package your game into a release bundle:

```bash
restgine build --release
```

This compiles your project with release optimizations, packages scripts and textures into `dist/game.pak`, and copies `restgine.exe` and `steam_api64.dll` into the output folder, ready for Steam deployment.
