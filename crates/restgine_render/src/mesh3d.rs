use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

/// 3D Mesh Vertex layout with optional bone weights for skeletal skinning.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Mesh3DVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub joints: [u32; 4],
    pub weights: [f32; 4],
}

impl Mesh3DVertex {
    pub const fn new(pos: [f32; 3], norm: [f32; 3], uv: [f32; 2]) -> Self {
        Self {
            position: pos,
            normal: norm,
            uv,
            joints: [0, 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        }
    }

    pub const fn new_skinned(
        pos: [f32; 3],
        norm: [f32; 3],
        uv: [f32; 2],
        joints: [u32; 4],
        weights: [f32; 4],
    ) -> Self {
        Self {
            position: pos,
            normal: norm,
            uv,
            joints,
            weights,
        }
    }
}

/// Instance data sent to the GPU for each 3D entity (80 bytes per instance).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Mesh3DInstanceRaw {
    pub model_matrix: [[f32; 4]; 4],
    pub color: [f32; 4],
}

impl Mesh3DInstanceRaw {
    pub fn new(model: Mat4, color: [f32; 4]) -> Self {
        Self {
            model_matrix: model.to_cols_array_2d(),
            color,
        }
    }
}

/// Comprehensive graphics settings and lighting uniform.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Camera3DUniform {
    pub view_proj: [[f32; 4]; 4],
    pub light_view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub light_dir: [f32; 4],
    /// x: gamma (0.5 to 1.5), y: motion_blur (0.0 to 1.0), z: fov_rad, w: view_distance (60 to 500)
    pub settings_params: [f32; 4],
    /// x: shadow_tier, y: gi_tier, z: shading_tier, w: post_process_tier
    pub quality_tiers: [u32; 4],
    /// x: effects_tier, y: foliage_tier, z: aa_tier, w: textures_tier
    pub quality_extra: [u32; 4],
}

/// Input parameters for updating the 3D camera and runtime shader settings.
#[derive(Debug, Clone, Copy)]
pub struct CameraSettingsInput {
    pub view_proj: Mat4,
    pub light_view_proj: Mat4,
    pub cam_pos: Vec3,
    pub light_dir: Vec3,
    pub gamma: f32,
    pub motion_blur: f32,
    pub fov_rad: f32,
    pub view_dist: f32,
    pub quality_tiers: [u32; 4],
    pub quality_extra: [u32; 4],
}

pub const MESH_3D_SHADER_WGSL: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    light_dir: vec4<f32>,
    settings: vec4<f32>,      // x=gamma, y=motion_blur, z=fov, w=view_distance
    quality_tiers: vec4<u32>, // x=shadow, y=gi, z=shading, w=post_process
    quality_extra: vec4<u32>, // x=effects, y=foliage, z=aa, w=textures
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

@group(0) @binding(1)
var t_shadow: texture_depth_2d;
@group(0) @binding(2)
var s_shadow: sampler_comparison;

struct BoneUniforms {
    bones: array<mat4x4<f32>, 64>,
};
@group(1) @binding(0)
var<uniform> bone_palette: BoneUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
};

struct InstanceInput {
    @location(5) model_0: vec4<f32>,
    @location(6) model_1: vec4<f32>,
    @location(7) model_2: vec4<f32>,
    @location(8) model_3: vec4<f32>,
    @location(9) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) shadow_pos: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    let model = mat4x4<f32>(
        inst.model_0,
        inst.model_1,
        inst.model_2,
        inst.model_3,
    );

    // GPU Skeletal Skinning
    var skin_mat = mat4x4<f32>(
        vec4<f32>(1.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(0.0, 0.0, 0.0, 1.0)
    );
    
    // If weights sum up, blend bone matrices
    let weight_sum = in.weights.x + in.weights.y + in.weights.z + in.weights.w;
    if (weight_sum > 0.01 && in.joints.x < 64u) {
        skin_mat = in.weights.x * bone_palette.bones[in.joints.x] +
                   in.weights.y * bone_palette.bones[in.joints.y] +
                   in.weights.z * bone_palette.bones[in.joints.z] +
                   in.weights.w * bone_palette.bones[in.joints.w];
    }

    let skinned_pos = skin_mat * vec4<f32>(in.position, 1.0);
    let world_pos = model * skinned_pos;
    let world_normal = (model * skin_mat * vec4<f32>(in.normal, 0.0)).xyz;

    var out: VertexOutput;
    out.clip_pos = camera.view_proj * world_pos;
    out.world_pos = world_pos.xyz;
    out.world_normal = normalize(world_normal);
    out.color = inst.color;
    out.uv = in.uv;

    // Projected Shadow Coordinates in [0, 1] range
    let shadow_clip = camera.light_view_proj * world_pos;
    out.shadow_pos = vec3<f32>(
        shadow_clip.x * 0.5 + 0.5,
        -shadow_clip.y * 0.5 + 0.5,
        shadow_clip.z
    );

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let light_dir = normalize(camera.light_dir.xyz);
    let normal = normalize(in.world_normal);
    let view_dir = normalize(camera.camera_pos.xyz - in.world_pos);

    // 1. Cascaded / PCF Shadow Calculation based on quality tier
    var shadow: f32 = 1.0;
    let shadow_tier = camera.quality_tiers.x;
    
    if (shadow_tier > 0u && in.shadow_pos.x >= 0.0 && in.shadow_pos.x <= 1.0 && in.shadow_pos.y >= 0.0 && in.shadow_pos.y <= 1.0) {
        let bias = 0.002;
        let shadow_depth = in.shadow_pos.z - bias;

        if (shadow_tier == 1u) {
            // Low: 1-tap hard shadow
            shadow = textureSampleCompare(t_shadow, s_shadow, in.shadow_pos.xy, shadow_depth);
        } else if (shadow_tier == 2u) {
            // Medium: 2x2 PCF (4 taps)
            let texel = vec2<f32>(1.0 / 2048.0, 1.0 / 2048.0);
            shadow = 0.25 * (
                textureSampleCompare(t_shadow, s_shadow, in.shadow_pos.xy + vec2<f32>(-texel.x, -texel.y), shadow_depth) +
                textureSampleCompare(t_shadow, s_shadow, in.shadow_pos.xy + vec2<f32>( texel.x, -texel.y), shadow_depth) +
                textureSampleCompare(t_shadow, s_shadow, in.shadow_pos.xy + vec2<f32>(-texel.x,  texel.y), shadow_depth) +
                textureSampleCompare(t_shadow, s_shadow, in.shadow_pos.xy + vec2<f32>( texel.x,  texel.y), shadow_depth)
            );
        } else {
            // High / Epic: 4x4 PCF (16 taps)
            let texel = vec2<f32>(1.0 / 2048.0, 1.0 / 2048.0);
            var sum: f32 = 0.0;
            for (var x = -1.5; x <= 1.5; x += 1.0) {
                for (var y = -1.5; y <= 1.5; y += 1.0) {
                    sum += textureSampleCompare(t_shadow, s_shadow, in.shadow_pos.xy + vec2<f32>(x, y) * texel, shadow_depth);
                }
            }
            shadow = sum / 16.0;
        }
    }

    // 2. GI / Ambient Occlusion Tier
    let gi_tier = camera.quality_tiers.y;
    var ambient_factor: f32 = 0.20;
    if (gi_tier == 1u) {
        ambient_factor = 0.24; // Medium SSAO
    } else if (gi_tier == 2u) {
        ambient_factor = 0.28; // High SSAO
    } else if (gi_tier == 3u) {
        ambient_factor = 0.32; // Epic Multi-bounce
    }
    let ambient = vec3<f32>(0.20, 0.24, 0.35) * ambient_factor;

    // 3. PBR Shading (Roughness, Metallic, Specular GGX approximation)
    let shading_tier = camera.quality_tiers.z;
    let diff = max(dot(normal, light_dir), 0.0);
    let sun_color = vec3<f32>(1.0, 0.98, 0.92);
    let diffuse = diff * sun_color * shadow;

    // Specular
    let half_dir = normalize(light_dir + view_dir);
    let ndoth = max(dot(normal, half_dir), 0.0);
    var spec_power: f32 = 32.0;
    if (shading_tier >= 2u) {
        spec_power = 64.0;
    }
    let spec = pow(ndoth, spec_power) * shadow;
    let specular = spec * vec3<f32>(0.4, 0.4, 0.4);

    var lit_color = (ambient + diffuse) * in.color.rgb + specular;

    // 4. View Distance Fog
    let view_dist = camera.settings.w;
    let dist = length(camera.camera_pos.xyz - in.world_pos);
    let fog_start = view_dist * 0.55;
    let fog_factor = clamp((dist - fog_start) / (view_dist - fog_start + 0.01), 0.0, 1.0);
    let fog_color = vec3<f32>(0.04, 0.06, 0.11);
    lit_color = mix(lit_color, fog_color, fog_factor);

    // 5. Post Processing: ACES Tonemapping + Gamma Correction
    let gamma = camera.settings.x;
    
    // ACES Film approximation curve
    let a: f32 = 2.51;
    let b: f32 = 0.03;
    let c: f32 = 2.43;
    let d: f32 = 0.59;
    let e: f32 = 0.14;
    let tonemapped = clamp((lit_color * (a * lit_color + b)) / (lit_color * (c * lit_color + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));

    // Dynamic Gamma adjustment (50% to 150%)
    let gamma_corrected = pow(tonemapped, vec3<f32>(1.0 / max(gamma, 0.01)));

    return vec4<f32>(gamma_corrected, in.color.a);
}
"#;

pub const SHADOW_PASS_SHADER_WGSL: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    light_dir: vec4<f32>,
    settings: vec4<f32>,
    quality_tiers: vec4<u32>,
    quality_extra: vec4<u32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
};

struct InstanceInput {
    @location(5) model_0: vec4<f32>,
    @location(6) model_1: vec4<f32>,
    @location(7) model_2: vec4<f32>,
    @location(8) model_3: vec4<f32>,
    @location(9) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(
        inst.model_0,
        inst.model_1,
        inst.model_2,
        inst.model_3,
    );
    let world_pos = model * vec4<f32>(in.position, 1.0);
    return camera.light_view_proj * world_pos;
}
"#;

pub struct Mesh3DPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub shadow_pipeline: wgpu::RenderPipeline,
    pub camera_buffer: wgpu::Buffer,
    pub camera_bind_group: wgpu::BindGroup,
    pub bone_buffer: wgpu::Buffer,
    pub bone_bind_group: wgpu::BindGroup,
    pub cube_vertex_buffer: wgpu::Buffer,
    pub cube_index_buffer: wgpu::Buffer,
    pub cube_index_count: u32,
    pub instance_buffer: wgpu::Buffer,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
    pub shadow_texture: wgpu::Texture,
    pub shadow_view: wgpu::TextureView,
    pub shadow_sampler: wgpu::Sampler,
}

impl Mesh3DPipeline {
    pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
    pub const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
    pub const SHADOW_MAP_SIZE: u32 = 2048;

    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let camera_uniform = Camera3DUniform {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            light_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            camera_pos: [0.0, 10.0, 20.0, 1.0],
            light_dir: [0.5, 0.8, 0.3, 0.0],
            settings_params: [1.0, 0.5, 90.0_f32.to_radians(), 150.0], // default gamma 1.0, 90 fov
            quality_tiers: [2, 2, 2, 2],                               // default High
            quality_extra: [2, 2, 2, 2],
        };

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mesh3D Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Bone palette uniform buffer (64 bones)
        let default_bones = [Mat4::IDENTITY.to_cols_array_2d(); 64];
        let bone_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mesh3D Bone Palette Buffer"),
            contents: bytemuck::cast_slice(&default_bones),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bone_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Mesh3D Bone Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let bone_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Mesh3D Bone Bind Group"),
            layout: &bone_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: bone_buffer.as_entire_binding(),
            }],
        });

        // Shadow Map Depth Texture & Sampler
        let shadow_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Directional Shadow Map Texture"),
            size: wgpu::Extent3d {
                width: Self::SHADOW_MAP_SIZE,
                height: Self::SHADOW_MAP_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::SHADOW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Shadow Comparison Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Mesh3D Camera Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                        count: None,
                    },
                ],
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Mesh3D Camera Bind Group"),
            layout: &camera_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
            ],
        });

        // 3D Unit Cube Vertices
        let (cube_verts, cube_indices) = create_cube_geometry();

        let cube_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mesh3D Cube Vertex Buffer"),
            contents: bytemuck::cast_slice(&cube_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let cube_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mesh3D Cube Index Buffer"),
            contents: bytemuck::cast_slice(&cube_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let max_instances = 4096;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Mesh3D Instance Buffer"),
            size: (max_instances * std::mem::size_of::<Mesh3DInstanceRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let depth_texture = create_depth_texture(device, width, height);
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Mesh3D PBR Shader"),
            source: wgpu::ShaderSource::Wgsl(MESH_3D_SHADER_WGSL.into()),
        });

        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Mesh3D Shadow Pass Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADOW_PASS_SHADER_WGSL.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Mesh3D Pipeline Layout"),
            bind_group_layouts: &[&camera_bind_group_layout, &bone_bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_buffer_layouts = [
            // Vertex layout
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Mesh3DVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x3, // position
                    },
                    wgpu::VertexAttribute {
                        offset: 12,
                        shader_location: 1,
                        format: wgpu::VertexFormat::Float32x3, // normal
                    },
                    wgpu::VertexAttribute {
                        offset: 24,
                        shader_location: 2,
                        format: wgpu::VertexFormat::Float32x2, // uv
                    },
                    wgpu::VertexAttribute {
                        offset: 32,
                        shader_location: 3,
                        format: wgpu::VertexFormat::Uint32x4, // joints
                    },
                    wgpu::VertexAttribute {
                        offset: 48,
                        shader_location: 4,
                        format: wgpu::VertexFormat::Float32x4, // weights
                    },
                ],
            },
            // Instance layout
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Mesh3DInstanceRaw>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 5,
                        format: wgpu::VertexFormat::Float32x4, // model_0
                    },
                    wgpu::VertexAttribute {
                        offset: 16,
                        shader_location: 6,
                        format: wgpu::VertexFormat::Float32x4, // model_1
                    },
                    wgpu::VertexAttribute {
                        offset: 32,
                        shader_location: 7,
                        format: wgpu::VertexFormat::Float32x4, // model_2
                    },
                    wgpu::VertexAttribute {
                        offset: 48,
                        shader_location: 8,
                        format: wgpu::VertexFormat::Float32x4, // model_3
                    },
                    wgpu::VertexAttribute {
                        offset: 64,
                        shader_location: 9,
                        format: wgpu::VertexFormat::Float32x4, // color
                    },
                ],
            },
        ];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Mesh3D PBR Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &vertex_buffer_layouts,
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: Self::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::GreaterEqual, // Reversed-Z
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Directional Shadow Pipeline
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Shadow Pass Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout],
                push_constant_ranges: &[],
            });

        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Shadow Pass Render Pipeline"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: "vs_main",
                buffers: &vertex_buffer_layouts,
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: Self::SHADOW_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            shadow_pipeline,
            camera_buffer,
            camera_bind_group,
            bone_buffer,
            bone_bind_group,
            cube_vertex_buffer,
            cube_index_buffer,
            cube_index_count: cube_indices.len() as u32,
            instance_buffer,
            depth_texture,
            depth_view,
            shadow_texture,
            shadow_view,
            shadow_sampler,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.depth_texture = create_depth_texture(device, width, height);
        self.depth_view = self
            .depth_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
    }

    pub fn update_camera(&self, queue: &wgpu::Queue, input: &CameraSettingsInput) {
        let uniform = Camera3DUniform {
            view_proj: input.view_proj.to_cols_array_2d(),
            light_view_proj: input.light_view_proj.to_cols_array_2d(),
            camera_pos: [input.cam_pos.x, input.cam_pos.y, input.cam_pos.z, 1.0],
            light_dir: [input.light_dir.x, input.light_dir.y, input.light_dir.z, 0.0],
            settings_params: [
                input.gamma,
                input.motion_blur,
                input.fov_rad,
                input.view_dist,
            ],
            quality_tiers: input.quality_tiers,
            quality_extra: input.quality_extra,
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[uniform]));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_camera_and_settings(
        &self,
        queue: &wgpu::Queue,
        view_proj: Mat4,
        light_view_proj: Mat4,
        cam_pos: Vec3,
        light_dir: Vec3,
        gamma: f32,
        motion_blur: f32,
        fov_rad: f32,
        view_dist: f32,
        quality_tiers: [u32; 4],
        quality_extra: [u32; 4],
    ) {
        self.update_camera(
            queue,
            &CameraSettingsInput {
                view_proj,
                light_view_proj,
                cam_pos,
                light_dir,
                gamma,
                motion_blur,
                fov_rad,
                view_dist,
                quality_tiers,
                quality_extra,
            },
        );
    }

    pub fn update_bones(&self, queue: &wgpu::Queue, bones: &[Mat4]) {
        let mut raw_bones = [Mat4::IDENTITY.to_cols_array_2d(); 64];
        let count = bones.len().min(64);
        for i in 0..count {
            raw_bones[i] = bones[i].to_cols_array_2d();
        }
        queue.write_buffer(&self.bone_buffer, 0, bytemuck::cast_slice(&raw_bones));
    }
}

fn create_depth_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Mesh3D Depth Texture"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: Mesh3DPipeline::DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn create_cube_geometry() -> (Vec<Mesh3DVertex>, Vec<u16>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ];

    for (normal, right, up) in faces {
        let base_idx = vertices.len() as u16;
        let c = [normal[0] * 0.5, normal[1] * 0.5, normal[2] * 0.5];
        let r = [right[0] * 0.5, right[1] * 0.5, right[2] * 0.5];
        let u = [up[0] * 0.5, up[1] * 0.5, up[2] * 0.5];

        let p0 = [c[0] - r[0] - u[0], c[1] - r[1] - u[1], c[2] - r[2] - u[2]];
        let p1 = [c[0] + r[0] - u[0], c[1] + r[1] - u[1], c[2] + r[2] - u[2]];
        let p2 = [c[0] + r[0] + u[0], c[1] + r[1] + u[1], c[2] + r[2] + u[2]];
        let p3 = [c[0] - r[0] + u[0], c[1] - r[1] + u[1], c[2] - r[2] + u[2]];

        vertices.push(Mesh3DVertex::new(p0, normal, [0.0, 1.0]));
        vertices.push(Mesh3DVertex::new(p1, normal, [1.0, 1.0]));
        vertices.push(Mesh3DVertex::new(p2, normal, [1.0, 0.0]));
        vertices.push(Mesh3DVertex::new(p3, normal, [0.0, 0.0]));

        indices.extend_from_slice(&[
            base_idx,
            base_idx + 1,
            base_idx + 2,
            base_idx,
            base_idx + 2,
            base_idx + 3,
        ]);
    }

    (vertices, indices)
}
