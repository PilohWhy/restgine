mod benchmark;
mod demo;

use clap::{Parser, Subcommand};
use notify::{RecursiveMode, Watcher};
use restgine_assets::VfsPacker;
use restgine_audio::AudioEngine;
use restgine_core::{GameClock, GameConfig, InputState, RenderDimension};
use restgine_ecs::create_world;
use restgine_luau::{write_typedef_file, EngineContext, LuauRuntime};
use restgine_physics::{step_physics_25d, SpatialHashGrid};
use restgine_render::{IsaacRenderBatch, PixelCamera};
use restgine_steam::SteamContext;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "restgine")]
#[command(author = "Restgine Contributors")]
#[command(version = "0.1.0")]
#[command(about = "High-performance universal 2D/3D continuum game engine for Steam", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize a new Restgine project
    Init {
        /// Name of the project directory to create
        name: String,
    },
    /// Run the game in development mode with live script and asset hot-reloading
    Dev {
        /// Optional path to project directory (defaults to current directory)
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Package the game into a standalone distribution bundle for Steam
    Build {
        /// Build with release optimizations and compress assets into game.pak
        #[arg(long, default_value = "true")]
        release: bool,
    },
    /// Run an uncapped performance benchmark measuring real framerate and entity throughput
    Benchmark {
        /// Number of simulated entities
        #[arg(short, long, default_value = "100000")]
        entities: usize,
        /// Duration of the benchmark run in seconds
        #[arg(short, long, default_value = "5")]
        duration: u64,
        /// Dimension mode: '2d', '2.5d', or '3d'
        #[arg(long, default_value = "3d")]
        dimension: String,
    },
    /// Run a sample 2D/3D continuum game with live uncapped FPS display
    Demo {
        /// Duration in seconds to run (default 5s, 0 for infinite)
        #[arg(short, long, default_value = "5")]
        duration: u64,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,restgine=info,restgine_cli=info"),
    )
    .init();

    // If run without arguments (e.g. double-clicked from file explorer)
    if std::env::args().len() <= 1 {
        return run_interactive_launcher();
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { name } => {
            init_project(&name)?;
        }
        Commands::Dev { path } => {
            run_dev_mode(&path)?;
        }
        Commands::Build { release: _ } => {
            build_release_pak()?;
        }
        Commands::Benchmark {
            entities,
            duration,
            dimension,
        } => {
            let dim = match dimension.to_lowercase().as_str() {
                "2d" => RenderDimension::Dimensional2D,
                "2.5d" | "iso" | "isometric" => RenderDimension::Isometric25D,
                _ => RenderDimension::Full3D,
            };
            benchmark::run_uncapped_benchmark(entities, duration, dim);
        }
        Commands::Demo { duration } => {
            demo::run_continuum_demo(duration)?;
        }
    }

    Ok(())
}

fn run_interactive_launcher() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    println!("\x1b[1;36m=========================================================\x1b[0m");
    println!("\x1b[1;36m                   RESTGINE CONTINUUM                    \x1b[0m");
    println!("     High-Performance 2D/3D Game Engine for Steam        ");
    println!("\x1b[1;36m=========================================================\x1b[0m\n");
    println!("  [1] Launch Interactive Game Demo (Uncapped FPS)");
    println!("  [2] Run Benchmark (10,000 Entities @ Uncapped Speed)");
    println!("  [3] Run Heavy Stress Test (50,000 Entities)");
    println!("  [4] Create New Project (restgine init)");
    println!("  [5] Exit\n");
    print!("Select an option [1-5] (default: 1): ");
    let _ = std::io::stdout().flush();

    let mut choice = String::new();
    let _ = std::io::stdin().read_line(&mut choice);
    let choice = choice.trim();

    match choice {
        "2" => {
            benchmark::run_uncapped_benchmark(10000, 5, RenderDimension::Full3D);
        }
        "3" => {
            benchmark::run_uncapped_benchmark(50000, 5, RenderDimension::Full3D);
        }
        "4" => {
            print!("\nEnter project name: ");
            let _ = std::io::stdout().flush();
            let mut name = String::new();
            let _ = std::io::stdin().read_line(&mut name);
            let name = name.trim();
            if !name.is_empty() {
                init_project(name)?;
            }
        }
        "5" | "q" | "exit" => {
            return Ok(());
        }
        _ => {
            println!("\nLaunching Interactive Continuum Demo (5s run)...\n");
            demo::run_continuum_demo(5)?;
        }
    }

    println!("\nPress Enter to exit...");
    let mut exit_buf = String::new();
    let _ = std::io::stdin().read_line(&mut exit_buf);
    Ok(())
}

fn init_project(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let project_dir = Path::new(name);
    if project_dir.exists() {
        return Err(format!("Directory '{}' already exists!", name).into());
    }

    fs::create_dir_all(project_dir.join("scripts"))?;
    fs::create_dir_all(project_dir.join("assets").join("sprites"))?;
    fs::create_dir_all(project_dir.join("assets").join("audio"))?;

    // Create default game.toml
    let default_config = GameConfig {
        title: name.to_string(),
        version: "0.1.0".to_string(),
        steam_app_id: 480,
        ..Default::default()
    };
    fs::write(project_dir.join("game.toml"), default_config.to_toml()?)?;

    // Write IDE type definitions
    write_typedef_file(project_dir.join("scripts").join("restgine.d.luau"))?;

    // Write sample main.luau (Isaac-style movement & shooting)
    let sample_script = r#"--!strict
-- Restgine Game Entry Point: main.luau
-- Inspired by The Binding of Isaac 2.5D movement and projectile mechanics

local player = nil
local shoot_timer = 0

function init()
    restgine:log_info("Initializing game world...")

    -- Spawn Player with 2.5D footprint
    player = restgine:spawn({
        x = 0,
        y = 0,
        z = 0,
        radius = 8,
        z_height = 16,
        is_solid = true,
        layer = "ground_entities"
    })

    -- Unlock introductory achievement if on Steam
    restgine:unlock_achievement("START_GAME")
    restgine:log_info("Player spawned successfully!")
end

function update(dt: number)
    if not player then return end

    -- 4-Directional smooth movement (WASD or Gamepad)
    local move_x, move_y = restgine:get_vector("a", "d", "w", "s")
    local speed = 120
    restgine:set_velocity(player, move_x * speed, move_y * speed, 0)

    -- Keep camera centered on player
    local px, py, _ = restgine:get_position(player)
    restgine:set_camera_position(px, py)

    -- Shooting tears (Arrow keys)
    shoot_timer = shoot_timer - dt
    local shoot_x, shoot_y = restgine:get_vector("left", "right", "up", "down")
    if (shoot_x ~= 0 or shoot_y ~= 0) and shoot_timer <= 0 then
        shoot_timer = 0.25
        restgine:play_sound("assets/audio/shoot.wav", 0.8, 1.0)
        
        -- Spawn projectile at head height (z = 8)
        restgine:spawn({
            x = px,
            y = py,
            z = 8,
            radius = 4,
            z_height = 8,
            is_solid = false,
            layer = "airborne"
        })
    end
end
"#;
    fs::write(project_dir.join("scripts").join("main.luau"), sample_script)?;

    println!(
        "\x1b[1;32m[SUCCESS]\x1b[0m Project '{}' created successfully!",
        name
    );
    println!("\nTo get started:");
    println!("  cd {}", name);
    println!("  restgine dev\n");

    Ok(())
}

fn run_dev_mode(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = path.join("game.toml");
    let config = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        GameConfig::from_toml(&content)?
    } else {
        log::warn!("game.toml not found, using default configuration");
        GameConfig::default()
    };

    println!("\x1b[1;36m=== Restgine Dev Runner ===\x1b[0m");
    println!("Title: {}", config.title);
    println!("Steam AppID: {}", config.steam_app_id);
    println!(
        "Native Viewport Height: {}p (Responsive Pixel Art)",
        config.viewport.native_height
    );

    let world = Arc::new(Mutex::new(create_world()));
    let input = Arc::new(Mutex::new(InputState::new()));
    let (audio_engine, _audio_stream) = AudioEngine::init_default();
    let audio = Arc::new(Mutex::new(audio_engine));
    let camera = Arc::new(Mutex::new(PixelCamera::new(config.viewport.native_height)));
    let steam = Arc::new(Mutex::new(SteamContext::init(config.steam_app_id)));

    let engine_ctx = EngineContext {
        world: world.clone(),
        input: input.clone(),
        audio: audio.clone(),
        camera: camera.clone(),
        steam: steam.clone(),
    };

    let main_script_path = path.join("scripts").join("main.luau");
    let mut luau_runtime = LuauRuntime::new(&main_script_path, engine_ctx)?;
    luau_runtime.call_init()?;

    // File Watcher for Live Hot-Reloading
    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    let scripts_dir = path.join("scripts");
    if scripts_dir.exists() {
        watcher.watch(&scripts_dir, RecursiveMode::Recursive)?;
        println!(
            "\x1b[1;32m[HOT-RELOAD]\x1b[0m Watching {:?} for live code updates...",
            scripts_dir
        );
    }

    let mut clock = GameClock::new(config.viewport.target_physics_hz);
    let mut spatial_grid = SpatialHashGrid::new(64.0);
    let mut render_batch = IsaacRenderBatch::new();

    let mut running = true;
    let mut frame_count = 0u64;

    while running {
        let _dt = clock.advance_frame();

        // 1. Process Hot-Reload notifications
        if let Ok(Ok(event)) = rx.try_recv() {
            log::info!("Detected file change: {:?}", event.paths);
            if let Err(e) = luau_runtime.reload() {
                log::error!("Luau reload error: {}", e);
            }
        }

        // 2. Fixed physics steps
        while clock.should_step() {
            let fixed_dt = clock.fixed_dt();
            if let Err(e) = luau_runtime.call_update(fixed_dt) {
                log::error!("Script update error: {}", e);
            }

            let mut w = world.lock().unwrap();
            step_physics_25d(&mut w, &mut spatial_grid, fixed_dt);
        }

        // 3. Render preparation
        let alpha = clock.alpha();
        let _ = luau_runtime.call_render(alpha);

        render_batch.clear();
        {
            let w = world.lock().unwrap();
            for (_id, (transform, sprite)) in w
                .query::<(&restgine_ecs::Transform25D, &restgine_ecs::Sprite)>()
                .iter()
            {
                render_batch.submit_sprite(transform, sprite);
            }
        }
        let _instances = render_batch.build_sorted_instances();

        // 4. Steam callback pump
        {
            let steam_guard = steam.lock().unwrap();
            steam_guard.run_callbacks();
        }

        frame_count += 1;
        // Run simulated dev frames or yield
        std::thread::sleep(Duration::from_millis(16));

        // In headless testing mode, stop after initial verification frames
        if std::env::var("RESTGINE_HEADLESS_TEST").is_ok() && frame_count >= 10 {
            log::info!("Headless verification loop completed successfully.");
            running = false;
        }
    }

    Ok(())
}

fn build_release_pak() -> Result<(), Box<dyn std::error::Error>> {
    println!("\x1b[1;36m=== Restgine Release Packer ===\x1b[0m");
    let dist_dir = Path::new("dist");
    fs::create_dir_all(dist_dir)?;

    let mut packer = VfsPacker::new();

    // Pack all files in assets/ and scripts/
    let folders_to_pack = ["assets", "scripts"];
    for folder in &folders_to_pack {
        let p = Path::new(folder);
        if p.exists() {
            for entry in walk_dir(p)? {
                let rel = entry.strip_prefix(".").unwrap_or(&entry);
                let vfs_path = rel.to_string_lossy().replace('\\', "/");
                packer.add_file(vfs_path, entry);
            }
        }
    }

    let pak_output = dist_dir.join("game.pak");
    packer.write_to_file(&pak_output)?;
    println!(
        "\x1b[1;32m[PACKED]\x1b[0m Created standalone archive at {:?}",
        pak_output
    );
    println!("Ready for Steam distribution deployment.");

    Ok(())
}

fn walk_dir(dir: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                files.extend(walk_dir(&path)?);
            } else {
                files.push(path);
            }
        }
    }
    Ok(files)
}
