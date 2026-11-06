# Contributing to Restgine

Restgine is an open-source, code-first game engine built for extreme performance. Whether you are fixing a bug, adding a platform feature, or optimizing a hot loop, your contributions are welcome.

## Design Rules

Before writing code, keep these core architectural invariants in mind:

1. **Zero Runtime Allocations**: No `Box`, `Vec::push`, `String`, or OS heap `malloc`/`free` calls during active frame loops. Use the 16MB thread-local linear arenas (`ThreadScratchpad`) for transient per-frame allocations.
2. **Branchless Continuum**: Never branch on dimension type in core loops (`if is_3d { ... } else { ... }`). All entities exist in the same 64-byte aligned SIMD affine coordinate space (`Transform3D`).
3. **Visibility Buffer Integrity**: The primary geometry target is 64-bit packed (`InstanceId: u32`, `PrimitiveId: u32`). Material shading happens in deferred compute. Never draw full-screen material geometry passes.
4. **Mechanical Sympathy**: Prefer contiguous flat arrays and structure-of-arrays over pointer chasing.

## Development Workflow

### 1. Prerequisites
- Rust 1.80+ (stable toolchain)
- CMake and a C++ compiler (required for vendored `mlua` / `luau`)
- DirectX 12, Vulkan, or Metal graphics driver

### 2. Branching Strategy
- `main`: Production-ready releases matching published tags (`v0.1.0`, etc.).
- `develop`: Active integration branch. Submit your pull requests against `develop`.

### 3. Local Verification

Before opening a pull request, run all four checks locally:

```bash
# 1. Format check
cargo fmt --all -- --check

# 2. Strict clippy lints (0 warnings tolerated)
cargo clippy --workspace --all-targets -- -D warnings

# 3. Unit test suite
cargo test --workspace

# 4. Release benchmark check
cargo run --release -p restgine_cli -- benchmark --entities 10000 --duration 2
```

### 4. Commit Conventions

Use clear, conventional commit messages:
- `feat: add reversed-z projection to 3d camera`
- `fix: resolve bvh8 boundary check on zero-sized entities`
- `perf: vectorize xpbd constraint resolution with avx2`
- `docs: update developer guide with asset packaging steps`

Avoid AI co-author trailers or corporate boilerplate. Keep commit logs clean, direct, and focused on the code diff.
