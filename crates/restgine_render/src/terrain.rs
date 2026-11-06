use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wgpu::util::DeviceExt;

/// Continuous analytical terrain height function with gentle rolling relief (0.5m to 2.0m).
/// Continuous and deterministic across both CPU physics and GPU vertex shader.
#[inline]
pub fn terrain_height(x: f32, z: f32) -> f32 {
    let s1 = (x * 0.025).sin() * (z * 0.025).cos() * 0.95;
    let s2 = (x * 0.07 + 1.3).sin() * (z * 0.06 - 0.7).cos() * 0.40;
    let s3 = (x * 0.18 - 2.1).cos() * (z * 0.15 + 1.8).sin() * 0.15;
    s1 + s2 + s3 + 1.0
}

/// Continuous analytical surface normal for smooth lighting.
#[inline]
pub fn terrain_normal(x: f32, z: f32) -> Vec3 {
    let eps = 0.15;
    let hl = terrain_height(x - eps, z);
    let hr = terrain_height(x + eps, z);
    let hd = terrain_height(x, z - eps);
    let hu = terrain_height(x, z + eps);
    Vec3::new(hl - hr, 2.0 * eps, hd - hu).normalize()
}

/// Terrain chunk grid vertex in local chunk space [0..32, 0, 0..32].
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TerrainVertex {
    pub local_pos: [f32; 3],
}

/// Instance offset for a 32m x 32m chunk [world_x, world_z].
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TerrainChunkInstance {
    pub chunk_offset: [f32; 2],
    pub _padding: [f32; 2],
}

pub const TERRAIN_SHADER_WGSL: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    light_dir: vec4<f32>,
    settings: vec4<f32>,      // x=gamma, y=time, z=fov, w=view_distance
    quality_tiers: vec4<u32>, // x=shadow, y=gi, z=shading, w=post_process
    quality_extra: vec4<u32>, // x=effects, y=foliage, z=aa, w=textures
    flashlight_pos: vec4<f32>, // xyz = pos, w = is_on
    flashlight_dir: vec4<f32>, // xyz = dir, w = intensity
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;
@group(0) @binding(1)
var t_shadow: texture_depth_2d;
@group(0) @binding(2)
var s_shadow: sampler_comparison;

@group(1) @binding(0)
var t_albedo: texture_2d<f32>;
@group(1) @binding(1)
var s_albedo: sampler;
@group(1) @binding(2)
var t_normal: texture_2d<f32>;
@group(1) @binding(3)
var s_normal: sampler;
@group(1) @binding(4)
var t_ao_roughness: texture_2d<f32>;
@group(1) @binding(5)
var s_ao_roughness: sampler;

struct VertexInput {
    @location(0) local_pos: vec3<f32>,
};

struct InstanceInput {
    @location(1) chunk_offset: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) shadow_pos: vec3<f32>,
};

fn get_terrain_height(x: f32, z: f32) -> f32 {
    let s1 = sin(x * 0.025) * cos(z * 0.025) * 0.95;
    let s2 = sin(x * 0.07 + 1.3) * cos(z * 0.06 - 0.7) * 0.40;
    let s3 = cos(x * 0.18 - 2.1) * sin(z * 0.15 + 1.8) * 0.15;
    return s1 + s2 + s3 + 1.0;
}

fn get_terrain_normal(x: f32, z: f32) -> vec3<f32> {
    let eps = 0.15;
    let hl = get_terrain_height(x - eps, z);
    let hr = get_terrain_height(x + eps, z);
    let hd = get_terrain_height(x, z - eps);
    let hu = get_terrain_height(x, z + eps);
    return normalize(vec3<f32>(hl - hr, 2.0 * eps, hd - hu));
}

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let world_x = in.local_pos.x + inst.chunk_offset.x;
    let world_z = in.local_pos.z + inst.chunk_offset.y;
    let world_y = get_terrain_height(world_x, world_z);
    let world_pos = vec3<f32>(world_x, world_y, world_z);

    out.world_pos = world_pos;
    out.world_normal = get_terrain_normal(world_x, world_z);
    out.clip_position = camera.view_proj * vec4<f32>(world_pos, 1.0);

    // 4m x 4m world space tiling for Poliigon ground
    out.uv = vec2<f32>(world_x * 0.25, world_z * 0.25);

    // Transform to shadow map space [-1..1] -> [0..1]
    let shadow_clip = camera.light_view_proj * vec4<f32>(world_pos, 1.0);
    out.shadow_pos = vec3<f32>(
        shadow_clip.x * 0.5 + 0.5,
        -shadow_clip.y * 0.5 + 0.5,
        shadow_clip.z
    );

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // 1. Sample PBR Textures
    let albedo = textureSample(t_albedo, s_albedo, in.uv).rgb;
    let raw_normal = textureSample(t_normal, s_normal, in.uv).xyz * 2.0 - 1.0;
    let ao_rough = textureSample(t_ao_roughness, s_ao_roughness, in.uv);
    let ao = ao_rough.r;
    let roughness = ao_rough.g;

    // Perturb surface normal via Tangent Space
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let tangent = normalize(cross(up, in.world_normal));
    let bitangent = cross(in.world_normal, tangent);
    let tbn = mat3x3<f32>(tangent, bitangent, in.world_normal);
    let normal = normalize(tbn * raw_normal);

    let light_dir = normalize(camera.light_dir.xyz);
    let view_dir = normalize(camera.camera_pos.xyz - in.world_pos);

    // 2. Cascaded / Directional Shadow Sampling (PCF filtering)
    var shadow: f32 = 1.0;
    let shadow_tier = camera.quality_tiers.x;
    if (shadow_tier > 0u && in.shadow_pos.z >= 0.0 && in.shadow_pos.z <= 1.0) {
        let texel_size = 1.0 / 2048.0;
        let bias = 0.0025;
        let shadow_depth = in.shadow_pos.z - bias;

        if (shadow_tier == 1u) {
            shadow = textureSampleCompare(t_shadow, s_shadow, in.shadow_pos.xy, shadow_depth);
        } else {
            var sum: f32 = 0.0;
            for (var y = -1; y <= 1; y++) {
                for (var x = -1; x <= 1; x++) {
                    let offset = vec2<f32>(f32(x), f32(y)) * texel_size;
                    sum += textureSampleCompare(t_shadow, s_shadow, in.shadow_pos.xy + offset, shadow_depth);
                }
            }
            shadow = sum / 9.0;
        }
    }

    // 3. Ambient Lighting & GI
    let gi_tier = camera.quality_tiers.y;
    var ambient_factor: f32 = 0.16;
    if (gi_tier == 1u) {
        ambient_factor = 0.22;
    } else if (gi_tier >= 2u) {
        ambient_factor = 0.28;
    }
    let ambient = vec3<f32>(0.18, 0.22, 0.32) * (ambient_factor * ao);

    // 4. Diffuse & PBR GGX Specular
    let diff = max(dot(normal, light_dir), 0.0);
    let sun_color = vec3<f32>(1.0, 0.98, 0.92);
    let diffuse = diff * sun_color * shadow;

    let half_dir = normalize(light_dir + view_dir);
    let ndoth = max(dot(normal, half_dir), 0.0);
    let spec_power = mix(64.0, 8.0, roughness);
    let spec = pow(ndoth, spec_power) * (1.0 - roughness * 0.7) * shadow;
    let specular = spec * vec3<f32>(0.15, 0.15, 0.15);

    var lit_color = (ambient + diffuse) * albedo + specular;

    // 5. Realistic Flashlight (Spotlight with smooth cone & inverse-square falloff)
    if (camera.flashlight_pos.w > 0.5) {
        let to_light = camera.flashlight_pos.xyz - in.world_pos;
        let dist = length(to_light);
        let l_dir = to_light / max(dist, 0.001);

        let spot_dot = dot(-l_dir, camera.flashlight_dir.xyz);
        let cos_outer = 0.86; // ~30 deg cone
        let cos_inner = 0.94; // ~20 deg hotspot
        let spot_factor = clamp((spot_dot - cos_outer) / (cos_inner - cos_outer), 0.0, 1.0);
        let smooth_spot = spot_factor * spot_factor * (3.0 - 2.0 * spot_factor);

        if (smooth_spot > 0.0) {
            let atten = 1.0 / (1.0 + 0.06 * dist + 0.015 * dist * dist);
            let dist_cutoff = clamp((55.0 - dist) / 12.0, 0.0, 1.0);

            let flash_diff = max(dot(normal, l_dir), 0.0);
            let flash_color = vec3<f32>(1.0, 0.97, 0.88);
            let flash_intensity = camera.flashlight_dir.w * 4.2;

            let half_flash = normalize(l_dir + view_dir);
            let flash_ndoth = max(dot(normal, half_flash), 0.0);
            let flash_spec = pow(flash_ndoth, mix(32.0, 96.0, 1.0 - roughness)) * (1.0 - roughness * 0.5);

            lit_color += (flash_diff * albedo + flash_spec * vec3<f32>(0.22)) * flash_color * (smooth_spot * atten * dist_cutoff * flash_intensity);
        }
    }

    // 6. Atmospheric Twilight Horizon Fog (Smoothly eliminates chunk boundary teeth)
    let dist = length(camera.camera_pos.xyz - in.world_pos);
    let view_dist = camera.settings.w;
    let fog_start = view_dist * 0.50;
    let fog_end = view_dist * 0.98;
    let fog_factor = clamp((dist - fog_start) / (fog_end - fog_start + 0.001), 0.0, 1.0);
    let smooth_fog = fog_factor * fog_factor * (3.0 - 2.0 * fog_factor);
    let horizon_fog_color = vec3<f32>(0.38, 0.24, 0.28);
    lit_color = mix(lit_color, horizon_fog_color, smooth_fog);

    // 7. ACES Filmic Tonemapping
    let a: f32 = 2.51;
    let b: f32 = 0.03;
    let c: f32 = 2.43;
    let d: f32 = 0.59;
    let e: f32 = 0.14;
    let tonemapped = clamp((lit_color * (a * lit_color + b)) / (lit_color * (c * lit_color + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));

    // Dynamic Gamma
    let gamma = camera.settings.x;
    let gamma_corrected = pow(tonemapped, vec3<f32>(1.0 / max(gamma, 0.01)));

    return vec4<f32>(gamma_corrected, 1.0);
}
"#;

pub struct TerrainPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub chunk_mesh_vertex_buffer: wgpu::Buffer,
    pub chunk_mesh_index_buffer: wgpu::Buffer,
    pub chunk_mesh_index_count: u32,
    pub instance_buffer: wgpu::Buffer,
    pub material_bind_group: wgpu::BindGroup,
}

impl TerrainPipeline {
    pub const CHUNK_SIZE: f32 = 32.0;
    pub const GRID_STEPS: usize = 32; // 1m resolution (33x33 vertices per chunk)

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
        base_color_bytes: &[u8],
        normal_bytes: &[u8],
        ao_rough_bytes: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // 1. Create Terrain Shared Unit Grid Mesh (32m x 32m at 1m resolution)
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let steps = Self::GRID_STEPS;
        let step_size = Self::CHUNK_SIZE / (steps as f32);

        for z in 0..=steps {
            for x in 0..=steps {
                let lx = (x as f32) * step_size;
                let lz = (z as f32) * step_size;
                vertices.push(TerrainVertex {
                    local_pos: [lx, 0.0, lz],
                });
            }
        }

        let stride = (steps + 1) as u16;
        for z in 0..steps as u16 {
            for x in 0..steps as u16 {
                let top_left = z * stride + x;
                let top_right = top_left + 1;
                let bottom_left = (z + 1) * stride + x;
                let bottom_right = bottom_left + 1;

                indices.push(top_left);
                indices.push(bottom_left);
                indices.push(top_right);

                indices.push(top_right);
                indices.push(bottom_left);
                indices.push(bottom_right);
            }
        }

        let chunk_mesh_vertex_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Terrain Grid Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let chunk_mesh_index_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Terrain Grid Index Buffer"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });

        let chunk_mesh_index_count = indices.len() as u32;

        let max_chunks = 256;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Terrain Chunk Instance Buffer"),
            size: (max_chunks * std::mem::size_of::<TerrainChunkInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 2. Load PBR Textures
        let (_, albedo_view) = create_texture_from_bytes(
            device,
            queue,
            base_color_bytes,
            "Poliigon Ground Albedo",
            true,
        )?;
        let (_, normal_view) = create_texture_from_bytes(
            device,
            queue,
            normal_bytes,
            "Poliigon Ground Normal",
            false,
        )?;
        let (_, ao_rough_view) = create_texture_from_bytes(
            device,
            queue,
            ao_rough_bytes,
            "Poliigon Ground AO Roughness",
            false,
        )?;

        let repeat_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Terrain Texture Repeat Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let material_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Terrain Material Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let material_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Terrain Material Bind Group"),
            layout: &material_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&albedo_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&repeat_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&normal_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&repeat_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&ao_rough_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&repeat_sampler),
                },
            ],
        });

        // 3. Render Pipeline
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Terrain PBR Shader Module"),
            source: wgpu::ShaderSource::Wgsl(TERRAIN_SHADER_WGSL.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Terrain Pipeline Layout"),
            bind_group_layouts: &[camera_bind_group_layout, &material_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Terrain Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[
                    // Local grid vertex buffer
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<TerrainVertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x3,
                        }],
                    },
                    // Chunk instance buffer
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<TerrainChunkInstance>()
                            as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        }],
                    },
                ],
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
                format: depth_format,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::GreaterEqual, // Reversed-Z
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Ok(Self {
            pipeline,
            chunk_mesh_vertex_buffer,
            chunk_mesh_index_buffer,
            chunk_mesh_index_count,
            instance_buffer,
            material_bind_group,
        })
    }
}

pub fn create_texture_from_bytes(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bytes: &[u8],
    label: &str,
    is_srgb: bool,
) -> Result<(wgpu::Texture, wgpu::TextureView), Box<dyn std::error::Error>> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    let (width, height) = img.dimensions();

    let format = if is_srgb {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &img,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Ok((texture, view))
}
