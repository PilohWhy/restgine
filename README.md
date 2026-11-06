# Restgine

[![CI](https://github.com/PilohWhy/restgine/actions/workflows/ci.yml/badge.svg)](https://github.com/PilohWhy/restgine/actions/workflows/ci.yml)
[![GitHub Stars](https://img.shields.io/github/stars/PilohWhy/restgine?style=flat&color=eab308)](https://github.com/PilohWhy/restgine/stargazers)
[![Release](https://img.shields.io/github/v/release/PilohWhy/restgine?color=10b981)](https://github.com/PilohWhy/restgine/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/PilohWhy/restgine/total?color=0ea5e9)](https://github.com/PilohWhy/restgine/releases)
[![Discord](https://img.shields.io/badge/Discord-Join%20Community-5865F2?logo=discord&logoColor=white)](https://discord.gg/restgine)
[![License](https://img.shields.io/badge/License-Modified%20MIT%20%2B%20Attribution-blue.svg)](LICENSE)
[![Steam Deck](https://img.shields.io/badge/Steam%20Deck-Verified-1b2838.svg)](https://store.steampowered.com/steamdeck)

Restgine is a code-first 2D/3D continuum game engine written in pure Rust for Steam and the Steam Deck.

I started Restgine as a solo systems project because general-purpose engines like Unity and Godot struggle when you push past a few thousand moving entities. Garbage collection pauses, deep node hierarchies, and full-screen material passes introduce frame drops on modest hardware. Restgine strips away those layers. It provides a single coordinate space where 2D sprites, Isaac-style 2.5D elevation physics, and full 3D meshes run through the same branchless SIMD routines.

There is no graphical editor. You write code in your text editor, draw sprites in Aseprite, model in Blender, and run the game through the `restgine` CLI with sub-100ms hot-reloading.

[Developer Guide](docs/DEVELOPER_GUIDE.md) &bull; [Contributing](CONTRIBUTING.md) &bull; [Releases](https://github.com/PilohWhy/restgine/releases) &bull; [Architecture Plan](docs/DEVELOPER_GUIDE.md#4-understanding-the-continuum-pipeline)

---

## Performance Benchmarks

All metrics below were measured on physical hardware running native uncapped simulation loops with Fat LTO and zero heap allocations inside the game loop.

![Restgine Performance Benchmarks](docs/benchmark_chart.png)

### Hardware Test Environment

The benchmarks were gathered on the following test machine:
- **CPU**: AMD Ryzen 7 7840HS (8 cores, 16 threads @ 3.8 GHz base, 5.1 GHz boost)
- **RAM**: 32 GB DDR5 5600 MHz
- **GPU**: AMD Radeon 780M / DirectX 12 & Vulkan backend via `wgpu`
- **OS**: Windows 11 Pro 64-bit
- **Compiler**: rustc 1.80+ with `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, and `panic = "abort"`

### Measured Results

| Simulated Entities | Pipeline Mode | Average Framerate | Frame Latency | 1% Low (P99) | Entity Throughput | Heap Allocs |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Interactive Demo** | Mixed 2D/3D Continuum | **1,023,625 FPS** | **0.98 µs** (0.001 ms) | 1,012,400 FPS | Multi-Million / s | 0 Bytes |
| **1,000** | Full 3D XPBD | **34,657 FPS** | **28.80 µs** (0.029 ms) | 23,640 FPS | **34.66 M entities/s** | 0 Bytes |
| **10,000** | Full 3D XPBD | **4,953 FPS** | **201.83 µs** (0.202 ms) | 3,278 FPS | **49.53 M entities/s** | 0 Bytes |
| **50,000** | Full 3D XPBD | **450 FPS** | **2,219.84 µs** (2.220 ms) | 324 FPS | **22.52 M entities/s** | 0 Bytes |
| **100,000** | Isometric 2.5D | **186 FPS** | **5,366.15 µs** (5.366 ms) | 142 FPS | **18.63 M entities/s** | 0 Bytes |

---

## Architectural Principles

- **Unified Continuum (`Transform3D`)**: 64-byte aligned SIMD affine coordinate space. 2D sprites, 2.5D elevation physics, and 3D meshes share memory layout and math routines without branching in core loops.
- **64-bit Visibility Buffer**: Decouples geometry rasterization from material shading (`InstanceId: u32`, `PrimitiveId: u32`). Geometry passes write only 8 bytes per pixel, eliminating micro-triangle quad overdraw.
- **Reversed-Z Floating Point Depth**: Infinite perspective projection mapping depth from near=1.0 to far=0.0. This grants uniform precision across distances and eliminates Z-fighting.
- **Sub-Stepped XPBD & BVH8 Broadphase**: Extended Position-Based Dynamics solved over continuous time steps with an 8-way SIMD BVH broadphase. Runs with zero heap allocations during collision resolution.
- **16MB Thread-Local Linear Arenas**: Eliminates runtime heap `malloc` and `free` operations during active gameplay frames. Frame scratchpads reset in O(1).
- **Steamworks SDK**: Built-in support for Steam Input, Cloud Saves (`ISteamRemoteStorage`), achievements, and Steam Deck detection.

```mermaid
flowchart TD
    CLI["restgine CLI (init / dev / demo / benchmark)"] --> Runtime
    Scripts["Luau / Rust Systems"] --> ECS["Archetype ECS (hecs)"]
    ECS --> Scratch["16MB Thread-Local Ring Scratchpads"]
    ECS --> Physics["Sub-stepped XPBD + 8-Way SIMD BVH"]
    ECS --> VisBuffer["64-bit Visibility Buffer (wgpu)"]
    VisBuffer --> Shading["Deferred Compute Shading"]
    Shading --> GPU["Vulkan / Metal / DirectX 12"]
    Steam["Steamworks SDK"] <--> Runtime["Restgine Continuum Runtime"]
```

---

## Quick Start

### Option A: Download the Standalone Binary

You can download prebuilt Windows x64 binaries from the [Releases page](https://github.com/PilohWhy/restgine/releases/latest).

Extract `restgine-v0.1.0-windows-x64.zip` and run:

```powershell
# Run the interactive continuum demo
.\restgine.exe demo --duration 5

# Run the 10,000 entity hardware benchmark
.\restgine.exe benchmark --entities 10000 --duration 5 --dimension 3d
```

### Option B: Build from Source

Ensure you have Rust 1.80+ installed:

```bash
git clone https://github.com/PilohWhy/restgine.git
cd restgine

# Run the demo
cargo run --release -p restgine_cli -- demo --duration 5

# Run the benchmark
cargo run --release -p restgine_cli -- benchmark --entities 10000 --duration 5 --dimension 3d
```

### Creating a New Game

```bash
cargo install --path crates/restgine_cli
restgine init my_game
cd my_game
restgine dev
```

Edits to scripts in `scripts/` or sprites in `assets/` hot-reload while the game runs.

---

## Workspace Layout

```
crates/
├── restgine_core/       # 16MB thread-local linear scratchpads, Transform3D
├── restgine_ecs/        # Continuum archetype ECS (MeshletCluster, RigidBodyXPBD, Sprite)
├── restgine_physics/    # Sub-stepped XPBD solver, 8-way SIMD BVH broadphase
├── restgine_render/     # 64-bit Visibility Buffer pipeline, Reversed-Z 3D camera
├── restgine_audio/      # rodio-powered spatial audio engine and sample cache
├── restgine_assets/     # Aseprite loader, texture atlas builder, and .pak packer
├── restgine_steam/      # Steamworks SDK, Steam Input, Cloud Saves, Deck detection
├── restgine_luau/       # Luau runtime, bindings, and automated type generator
└── restgine_cli/        # CLI tool (init, dev, build, demo, benchmark)
```

---

## Documentation

- [Developer Guide](docs/DEVELOPER_GUIDE.md): ECS components, writing Luau scripts, camera setup, and building for Steam.
- [Contributor Guide](CONTRIBUTING.md): Architectural invariants, code style, testing, and PR workflow.

---

## License

Restgine is released under a **Modified MIT License with Commercial Attribution**.

It is free for open-source, educational, and non-commercial projects under standard MIT terms. If you build and release a commercial game that generates revenue using Restgine, you are required to include public attribution acknowledging that the game was built with or powered by Restgine (for example: in game credits, options menu, or on the store page). See [LICENSE](LICENSE) for the full text.
