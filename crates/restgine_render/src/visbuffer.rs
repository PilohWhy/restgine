use bytemuck::{Pod, Zeroable};
use glam::Mat4;

/// 3D Mesh vertex layout for compact caching (32 bytes per vertex).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

impl MeshVertex {
    pub const fn new(position: [f32; 3], normal: [f32; 3], uv: [f32; 2]) -> Self {
        Self {
            position,
            normal,
            uv,
        }
    }
}

/// 64-byte aligned instance transform and metadata for 3D GPU instanced rasterization.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MeshInstanceRaw {
    pub model_matrix: [[f32; 4]; 4],
    pub instance_id: u32,
    pub _padding: [u32; 3],
}

impl MeshInstanceRaw {
    pub fn from_mat4(mat: Mat4, instance_id: u32) -> Self {
        Self {
            model_matrix: mat.to_cols_array_2d(),
            instance_id,
            _padding: [0, 0, 0],
        }
    }
}

/// 64-bit Visibility Buffer pixel format:
/// Packed `(instance_id: u32, primitive_id: u32)` = 8 bytes per pixel.
/// Eliminates material quad overdraw by separating geometry rasterization
/// from material shading.
#[derive(Debug)]
pub struct VisibilityBufferTarget {
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
}

impl VisibilityBufferTarget {
    /// Texture format used by the 64-bit Visibility Buffer: 2x 32-bit unsigned integers.
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Uint;
    /// Depth format using Reversed-Z 32-bit floating point.
    pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Self::FORMAT,
        }
    }

    /// Recreate textures on resize.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
    }
}

/// WGSL shader code for 64-bit Visibility Buffer rasterization.
pub const VISIBILITY_BUFFER_SHADER: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
};
@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct InstanceInput {
    @location(3) model_col0: vec4<f32>,
    @location(4) model_col1: vec4<f32>,
    @location(5) model_col2: vec4<f32>,
    @location(6) model_col3: vec4<f32>,
    @location(7) instance_id: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) instance_id: u32,
};

@vertex
fn vs_main(
    in: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    let model = mat4x4<f32>(
        instance.model_col0,
        instance.model_col1,
        instance.model_col2,
        instance.model_col3,
    );
    let world_pos = model * vec4<f32>(in.position, 1.0);
    
    var out: VertexOutput;
    out.clip_position = camera.view_proj * world_pos;
    out.instance_id = instance.instance_id;
    return out;
}

struct FragmentOutput {
    @location(0) vis_data: vec2<u32>, // (instance_id, primitive_id)
};

@fragment
fn fs_main(
    in: VertexOutput,
    @builtin(primitive_index) primitive_index: u32,
) -> FragmentOutput {
    var out: FragmentOutput;
    // 64-bit packed payload: (instance_id, primitive_index)
    out.vis_data = vec2<u32>(in.instance_id, primitive_index);
    return out;
}
"#;
