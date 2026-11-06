pub const POSTPROCESS_SHADER_WGSL: &str = r#"
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
var t_scene: texture_2d<f32>;
@group(0) @binding(2)
var s_linear: sampler;
@group(0) @binding(3)
var t_depth: texture_depth_2d;
@group(0) @binding(4)
var s_nearest: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) screen_pos: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    // Generate fullscreen triangle covering [-1, -1] to [3, -1], [-1, 3]
    let x = f32(i32(in_vertex_index & 1u) * 4 - 1);
    let y = f32(i32(in_vertex_index & 2u) * 2 - 1);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.screen_pos = vec2<f32>(x, y);
    out.uv = vec2<f32>(x * 0.5 + 0.5, -y * 0.5 + 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var scene_color = textureSample(t_scene, s_linear, in.uv).rgb;
    let raw_depth = textureSample(t_depth, s_nearest, in.uv);
    let is_sky = raw_depth <= 0.00002;

    // Reconstruct world ray and world position from Reversed-Z depth
    let clip_pos = vec4<f32>(in.screen_pos.x, in.screen_pos.y, raw_depth, 1.0);
    let world_h = camera.inv_view_proj * clip_pos;
    let world_pos = world_h.xyz / max(world_h.w, 0.0001);

    let ray_vec = world_pos - camera.camera_pos.xyz;
    let total_dist = select(length(ray_vec), 140.0, is_sky);
    let ray_dir = normalize(ray_vec);
    let max_march_dist = min(total_dist, 55.0);

    // ------------------------------------------------------------------------
    // 1. VOLUMETRIC FOG & FLASHLIGHT GOD RAYS (Effects Tier)
    // ------------------------------------------------------------------------
    let effects_tier = camera.quality_extra.x;
    if (effects_tier > 0u) {
        var steps: u32 = 10u;
        if (effects_tier == 2u) {
            steps = 18u;
        } else if (effects_tier >= 3u) {
            steps = 28u;
        }

        // Screen-space dither hash to eliminate raymarch banding
        let dither = fract(sin(dot(in.uv * 1280.0, vec2<f32>(12.9898, 78.233)) + camera.settings.y * 12.0) * 43758.5453);
        let step_size = max_march_dist / f32(steps);
        var march_t = step_size * dither;
        var accum_light = vec3<f32>(0.0);
        var extinction: f32 = 1.0;

        for (var i: u32 = 0u; i < steps; i = i + 1u) {
            if (march_t >= max_march_dist || extinction <= 0.03) {
                break;
            }
            let p = camera.camera_pos.xyz + ray_dir * march_t;

            // Ground Mist Layer (drifting low-lying height fog between 0m and 3.5m)
            let ground_y = p.y;
            let height_density = clamp(exp(-ground_y * 0.25) * 0.035, 0.0, 0.045);

            // Flashlight Volumetric Beam
            var flash_scatter: f32 = 0.0;
            if (camera.flashlight_pos.w > 0.5) {
                let to_light = p - camera.flashlight_pos.xyz;
                let l_dist = length(to_light);
                if (l_dist < 48.0) {
                    let l_dir = to_light / max(l_dist, 0.001);
                    let cos_angle = dot(l_dir, camera.flashlight_dir.xyz);
                    let cos_outer = 0.84; // ~32 deg
                    let cos_inner = 0.94; // ~20 deg hotspot
                    let spot = clamp((cos_angle - cos_outer) / (cos_inner - cos_outer), 0.0, 1.0);
                    if (spot > 0.0) {
                        let atten = 1.0 / (1.0 + 0.08 * l_dist + 0.018 * l_dist * l_dist);
                        // Forward Mie scattering phase function
                        let cos_view = dot(-ray_dir, l_dir);
                        let g = 0.62;
                        let mie = (1.0 - g * g) / pow(1.0 + g * g - 2.0 * g * cos_view, 1.5) * 0.25;
                        flash_scatter = spot * spot * atten * mie * camera.flashlight_dir.w * 4.6;
                    }
                }
            }

            let step_extinction = exp(-(height_density + flash_scatter * 0.40) * step_size);
            extinction *= step_extinction;

            let flash_color = vec3<f32>(1.0, 0.98, 0.90);
            let mist_color = vec3<f32>(0.04, 0.08, 0.16);
            let step_inscatter = (flash_scatter * flash_color + height_density * mist_color) * step_size;
            accum_light += step_inscatter * (1.0 - step_extinction);

            march_t += step_size;
        }

        scene_color = scene_color * extinction + accum_light;
    }

    // ------------------------------------------------------------------------
    // 2. BLOOM & HIGHLIGHT HALO (Post-Processing Tier)
    // ------------------------------------------------------------------------
    let post_tier = camera.quality_tiers.w;
    if (post_tier >= 2u) {
        let luma = dot(scene_color, vec3<f32>(0.2126, 0.7152, 0.0722));
        let bloom_threshold = 0.82;
        let bloom_factor = clamp((luma - bloom_threshold) / 0.45, 0.0, 1.0);
        let bloom_tint = select(vec3<f32>(0.85, 0.92, 1.0), vec3<f32>(1.0, 0.97, 0.90), camera.flashlight_pos.w > 0.5);
        scene_color += scene_color * bloom_factor * bloom_factor * 0.42 * bloom_tint;
    }

    // ------------------------------------------------------------------------
    // 3. VIGNETTE & CINEMATIC EDGES
    // ------------------------------------------------------------------------
    if (post_tier >= 1u) {
        let dist_center = length(in.screen_pos * vec2<f32>(0.48, 0.62));
        let vignette = clamp(1.0 - dist_center * dist_center * 0.54, 0.0, 1.0);
        scene_color *= (vignette * 0.86 + 0.14);
    }

    // ------------------------------------------------------------------------
    // 4. ANALOG 35MM FILM GRAIN
    // ------------------------------------------------------------------------
    if (post_tier >= 2u) {
        let grain = fract(sin(dot(in.uv * 1920.0, vec2<f32>(12.9898, 78.233)) + camera.settings.y * 36.0) * 43758.5453);
        scene_color += (grain - 0.5) * 0.016;
    }

    // ------------------------------------------------------------------------
    // 5. ACES TONEMAPPING & GAMMA CORRECTION
    // ------------------------------------------------------------------------
    let a: f32 = 2.51;
    let b: f32 = 0.03;
    let c: f32 = 2.43;
    let d: f32 = 0.59;
    let e: f32 = 0.14;
    let tonemapped = clamp((scene_color * (a * scene_color + b)) / (scene_color * (c * scene_color + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));

    let gamma = camera.settings.x;
    let final_color = pow(tonemapped, vec3<f32>(1.0 / max(gamma, 0.01)));
    return vec4<f32>(final_color, 1.0);
}
"#;

pub struct PostProcessPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub linear_sampler: wgpu::Sampler,
    pub nearest_sampler: wgpu::Sampler,
}

impl PostProcessPipeline {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("PostProcess Bind Group Layout"),
            entries: &[
                // Binding 0: CameraUniform
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Binding 1: HDR Scene Texture
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Binding 2: Linear Sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Binding 3: Depth Texture
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Binding 4: Nearest Sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });

        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("PostProcess Linear Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("PostProcess Nearest Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("PostProcess Shader Module"),
            source: wgpu::ShaderSource::Wgsl(POSTPROCESS_SHADER_WGSL.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("PostProcess Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("PostProcess Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
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
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
            linear_sampler,
            nearest_sampler,
        }
    }

    pub fn create_bind_group(
        &self,
        device: &wgpu::Device,
        camera_buffer: &wgpu::Buffer,
        hdr_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("PostProcess Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(hdr_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.nearest_sampler),
                },
            ],
        })
    }
}
