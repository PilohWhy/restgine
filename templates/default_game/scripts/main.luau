--!strict
-- Restgine Demonstration Game: Isaac Odyssey
-- 2.5D Isometric Movement, Y-Sorting, and Tear Projectile Combat

local player = nil
local enemies = {}
local shoot_cooldown = 0

function init()
    restgine:log_info("=== Initializing Isaac Odyssey Demo ===")

    -- 1. Spawn Player at origin
    player = restgine:spawn({
        x = 0,
        y = 0,
        z = 0,
        radius = 8,
        z_height = 16,
        is_solid = true,
        layer = "ground_entities"
    })

    -- 2. Spawn Static Obstacles (Rocks/Pillars)
    for i = -2, 2 do
        if i ~= 0 then
            restgine:spawn({
                x = i * 48,
                y = -40,
                z = 0,
                radius = 12,
                z_height = 24,
                is_solid = true,
                layer = "ground_entities"
            })
        end
    end

    -- 3. Spawn Test Enemy (Fly / Monstro)
    local enemy = restgine:spawn({
        x = 60,
        y = 40,
        z = 0,
        radius = 10,
        z_height = 14,
        is_solid = true,
        layer = "ground_entities"
    })
    table.insert(enemies, enemy)

    -- 4. Check Platform & Steam
    if restgine:is_steam_deck() then
        restgine:log_info("Running on Steam Deck hardware! Adjusting UI scale...")
    end
    restgine:unlock_achievement("DEMO_STARTED")
end

function update(dt: number)
    if not player then return end

    -- Player 8-directional movement (WASD or Gamepad)
    local move_x, move_y = restgine:get_vector("a", "d", "w", "s")
    local player_speed = 130
    restgine:set_velocity(player, move_x * player_speed, move_y * player_speed, 0)

    -- Update camera to follow player smoothly
    local px, py, _ = restgine:get_position(player)
    restgine:set_camera_position(px, py)
    restgine:set_listener_position(px, py)

    -- Shooting tears (Arrow keys)
    shoot_cooldown = shoot_cooldown - dt
    local shoot_x, shoot_y = restgine:get_vector("left", "right", "up", "down")
    if (shoot_x ~= 0 or shoot_y ~= 0) and shoot_cooldown <= 0 then
        shoot_cooldown = 0.22
        restgine:play_sound("assets/audio/tear_shoot.wav", 0.7, 1.0)

        -- Spawn tear with forward velocity and slight vertical boost (z-axis arc)
        local tear_speed = 260
        local tear = restgine:spawn({
            x = px,
            y = py,
            z = 8,
            radius = 4,
            z_height = 6,
            is_solid = false,
            layer = "airborne"
        })
        restgine:set_velocity(tear, shoot_x * tear_speed, shoot_y * tear_speed, 15)
    end
end
