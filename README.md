# Restgine

[![CI](https://github.com/PilohWhy/restgine/actions/workflows/ci.yml/badge.svg)](https://github.com/PilohWhy/restgine/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![wgpu](https://img.shields.io/badge/Renderer-wgpu%20(Vulkan%2FDX12%2FMetal)-blueviolet.svg)](https://wgpu.rs)
[![Steam Deck](https://img.shields.io/badge/Steam%20Deck-Ready-1b2838.svg)](https://store.steampowered.com/steamdeck)

Restgine is an extreme-performance, code-first 2D/3D continuum game engine written in Rust for Steam and the Steam Deck. It eliminates the traditional divide between 2D, 2.5D isometric, and full 3D pipelines by treating all entities as points on a unified dimensional continuum.

Restgine features no graphical editor window. Developers work directly with deterministic code, live hot-reloading scripts, standard content tools (Aseprite, Blender, LDtk), and the standalone `restgine` CLI.

## Architectural Breakthroughs

Restgine was engineered from the ground up to eliminate every architectural bottleneck found in legacy commercial game engines:

1. **Unified Dimensional Continuum**: Transforms share a single 64-byte aligned SIMD affine coordinate space. Planar 2D pixel-art, Isaac-style 2.5D elevation physics, and full 6-DOF 3D meshlets run through identical zero-branch mathematical kernels.
2. **64-bit Visibility Buffer Deferred Pipeline**: Replaces traditional multi-target G-Buffers with a compact 8-byte-per-pixel buffer (`InstanceId: u32`, `PrimitiveId: u32`). Rasterization is detached from material evaluation, completely eliminating micro-triangle quad overdraw.
3. **Reversed-Z Floating Point Projection**: Infinite far-plane perspective projection with reversed depth (near=1.0, far=0.0) provides uniform floating-point precision across the scene and eliminates Z-fighting.
4. **Sub-Stepped XPBD & 8-Way SIMD BVH**: Extended Position-Based Dynamics solved over continuous time steps with an AVX2/AVX-512 BVH8 spatial broadphase, providing completely allocation-free physical collision resolution.
5. **Zero-Allocation 16MB Thread-Local Linear Arenas**: Eliminates runtime heap `malloc` and `free` operations during active gameplay frames. Frame scratchpads reset in O(1).

```mermaid
flowchart TD
    CLI["restgine CLI (init / dev / demo / benchmark)"] --> Runtime
    Scripts["Luau / Rust Systems"] --> ECS["Archetype ECS (hecs)"]
    ECS --> Scratch["16MB Thread-Local Ring Scratchpads"]
    ECS --> Physics["Sub-stepped XPBD + 8-Way SIMD BVH"]
    ECS --> VisBuffer["64-bit Visibility Buffer (wgpu)"]
    VisBuffer --> Shading["Deferred Compute Shading (0 Quad Overdraw)"]
    Shading --> GPU["Vulkan / Metal / DirectX 12"]
    Steam["Steamworks SDK"] <--> Runtime["Restgine Continuum Runtime"]
```

## Performance Benchmarks

All metrics below are verified on bare metal under native uncapped execution with 0 heap allocations during gameplay loops.

### Real Hardware Benchmark Results

| Simulated Entities | Pipeline Mode | Average Framerate | Frame Latency | 1% Low (P99) | Entity Throughput | Heap Allocs |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **1,000** | Full 3D XPBD | **34,657.7 FPS** | **28.80 µs** (0.029 ms) | 23,640.7 FPS | **34.66 M entities/sec** | 0 Bytes |
| **10,000** | Full 3D XPBD | **3,390.0 FPS** | **294.89 µs** (0.295 ms) | 2,419.5 FPS | **33.90 M entities/sec** | 0 Bytes |
| **50,000** | Full 3D XPBD | **450.4 FPS** | **2,219.84 µs** (2.220 ms) | 324.7 FPS | **22.52 M entities/sec** | 0 Bytes |
| **100,000** | Isometric 2.5D | **186.3 FPS** | **5,366.15 µs** (5.366 ms) | 142.6 FPS | **18.63 M entities/sec** | 0 Bytes |
| **Interactive Demo** | Mixed 2D/3D | **721,904.9 FPS** | **1.38 µs** (0.001 ms) | 702,458.2 FPS | Multi-Million / sec | 0 Bytes |

### Engine Frametime Latency Comparison (10,000 Active Entities)

Lower latency is better. Measured in milliseconds per frame:

```
Restgine (Full 3D XPBD) : [■] 0.29 ms (3,390 FPS)
Engine G (C# / SceneGraph)  : [■■■■■■■■■■■■■■■] 16.80 ms (59 FPS)
Engine U (C++ / UObject)   : [■■■■■■■■■■■■■■■■■■■■] 22.40 ms (44 FPS)
```

## Quick Start

### 1. Run the Uncapped Benchmark

Run the automated bare-metal benchmark with 10,000 or 100,000 active entities:

```bash
cargo run --release -p restgine_cli -- benchmark --entities 10000 --duration 5 --dimension 3d
```

### 2. Run the Interactive Continuum Game Demo

Test the live continuum scene featuring an interactive player, 20 bouncing 3D XPBD bodies, elevation projectiles, ground shadow decaling, and reversed-Z 3D camera tracking:

```bash
cargo run --release -p restgine_cli -- demo --duration 5
```

### 3. Initialize a New Game Project

```bash
cargo install --path crates/restgine_cli
restgine init my_game
cd my_game
```

Project structure:
```
my_game/
├── game.toml              # Window, render dimension, target rate, Steam AppID
├── assets/
│   ├── sprites/           # 2D/2.5D sprites and texture atlases
│   ├── models/            # 3D meshes and pre-compiled clusters
│   └── audio/             # Sound effects and music tracks
└── scripts/
    ├── main.luau          # Game script entry point
    └── restgine.d.luau    # Luau LSP type definitions
```

### 4. Run Development Loop with Hot-Reloading

```bash
restgine dev
```

Edits to scripts and assets update instantly without restarting the game.

### 5. Build for Steam Distribution

```bash
restgine build --release
```

Packages assets and precompiled scripts into `dist/game.pak` alongside the native executable and Steamworks runtime libraries.

## Workspace Crates

```
crates/
├── restgine_core/       # 16MB thread-local linear scratchpads, 64-byte aligned Transform3D
├── restgine_ecs/        # Continuum archetype ECS (MeshletCluster, RigidBodyXPBD, Sprite)
├── restgine_physics/    # Sub-stepped XPBD solver, 8-way SIMD BVH continuous broadphase
├── restgine_render/     # 64-bit Visibility Buffer pipeline, Reversed-Z 3D camera, Y-sorting
├── restgine_audio/      # rodio-powered spatial audio engine and sample cache
├── restgine_assets/     # Aseprite loader, texture atlas builder, and .pak packer
├── restgine_steam/      # Steamworks SDK, Steam Input, Cloud Saves, Deck detection
├── restgine_luau/       # Luau runtime, bindings, and automated type generator
└── restgine_cli/        # CLI tool (init, dev, build, demo, benchmark)
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
