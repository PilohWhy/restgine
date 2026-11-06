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
    let post_tier = camera.quality_tiers.w;

    // ------------------------------------------------------------------------
    // 0. CHROMATIC ABERRATION (Screen-Edge Lens Dispersion on High & Epic)
    // ------------------------------------------------------------------------
    var uv_r = in.uv;
    var uv_b = in.uv;
    if (post_tier >= 2u) {
        let ca_strength = select(0.0035, 0.0075, post_tier >= 3u);
        let ca_dir = in.uv - vec2<f32>(0.5, 0.5);
        let ca_dist = dot(ca_dir, ca_dir);
        let ca_offset = ca_dir * (ca_dist * ca_strength);
        uv_r = in.uv + ca_offset;
        uv_b = in.uv - ca_offset;
    }

    var scene_color = vec3<f32>(
        textureSample(t_scene, s_linear, uv_r).r,
        textureSample(t_scene, s_linear, in.uv).g,
        textureSample(t_scene, s_linear, uv_b).b
    );

    let raw_depth = textureSample(t_depth, s_nearest, in.uv);
    let is_sky = raw_depth <= 0.00002;

    // ------------------------------------------------------------------------
    // 1. SCREEN-SPACE AMBIENT OCCLUSION (SSAO - Deep Contact Darkening)
    // ------------------------------------------------------------------------
    let gi_tier = camera.quality_tiers.y;
    if (gi_tier >= 2u && !is_sky) {
        var occlusion: f32 = 0.0;
        let sample_rad = select(0.014, 0.024, gi_tier >= 3u);
        let kernel_samples: u32 = select(4u, 8u, gi_tier >= 3u);
        let inv_kernel = 1.0 / f32(kernel_samples);

        for (var k: u32 = 0u; k < kernel_samples; k = k + 1u) {
            let angle = f32(k) * 2.3999632; // Golden angle
            let r = sqrt(f32(k) + 0.5) * inv_kernel * sample_rad;
            let offset_uv = in.uv + vec2<f32>(cos(angle), sin(angle)) * r;
            let sample_d = textureSample(t_depth, s_nearest, offset_uv);
            // In Reversed-Z, closer geometry has LARGER depth value
            let diff = sample_d - raw_depth;
            if (diff > 0.0003 && diff < 0.05) {
                occlusion += (1.0 - diff / 0.05);
            }
        }
        let ao_darkening = clamp(1.0 - (occlusion * inv_kernel) * select(1.3, 2.0, gi_tier >= 3u), 0.20, 1.0);
        scene_color *= ao_darkening;
    }

    // Reconstruct world ray and world position from Reversed-Z depth
    let clip_pos = vec4<f32>(in.screen_pos.x, in.screen_pos.y, raw_depth, 1.0);
    let world_h = camera.inv_view_proj * clip_pos;
    let world_pos = world_h.xyz / max(world_h.w, 0.0001);

    let ray_vec = world_pos - camera.camera_pos.xyz;
    let total_dist = select(length(ray_vec), 140.0, is_sky);
    let ray_dir = normalize(ray_vec);
    let max_march_dist = min(total_dist, 60.0);

    // ------------------------------------------------------------------------
    // 2. VOLUMETRIC FOG & FLASHLIGHT GOD RAYS (Effects Tier)
    // ------------------------------------------------------------------------
    let effects_tier = camera.quality_extra.x;
    if (effects_tier > 0u) {
        var steps: u32 = 10u;
        if (effects_tier == 2u) {
            steps = 22u;
        } else if (effects_tier >= 3u) {
            steps = 34u;
        }

        // Screen-space dither hash to eliminate raymarch banding
        let dither = fract(sin(dot(in.uv * 1280.0, vec2<f32>(12.9898, 78.233)) + camera.settings.y * 12.0) * 43758.5453);
        let step_size = max_march_dist / f32(steps);
        var march_t = step_size * dither;
        var accum_light = vec3<f32>(0.0);
        var extinction: f32 = 1.0;

        let moon_dir = normalize(camera.light_dir.xyz);
        let moon_cos = max(dot(-ray_dir, moon_dir), 0.0);
        let moon_mie = pow(moon_cos, 16.0) * 0.45 + 0.06;

        for (var i: u32 = 0u; i < steps; i = i + 1u) {
            if (march_t >= max_march_dist || extinction <= 0.02) {
                break;
            }
            let p = camera.camera_pos.xyz + ray_dir * march_t;

            // Ground Mist Layer (drifting low-lying height fog between 0m and 4.0m)
            let ground_y = p.y;
            let mist_density = clamp(exp(-ground_y * 0.22) * select(0.025, 0.055, effects_tier >= 3u), 0.0, 0.07);

            // Flashlight Volumetric God Rays
            var flash_scatter: f32 = 0.0;
            if (camera.flashlight_pos.w > 0.5) {
                let to_light = p - camera.flashlight_pos.xyz;
                let l_dist = length(to_light);
                if (l_dist < 52.0) {
                    let l_dir = to_light / max(l_dist, 0.001);
                    let cos_angle = dot(l_dir, camera.flashlight_dir.xyz);
                    let cos_outer = 0.83; // ~34 deg cone
                    let cos_inner = 0.94; // ~20 deg hotspot
                    let spot = clamp((cos_angle - cos_outer) / (cos_inner - cos_outer), 0.0, 1.0);
                    if (spot > 0.0) {
                        let atten = 1.0 / (1.0 + 0.07 * l_dist + 0.015 * l_dist * l_dist);
                        // Forward Mie scattering phase function
                        let cos_view = dot(-ray_dir, l_dir);
                        let g = select(0.60, 0.74, effects_tier >= 3u);
                        let mie = (1.0 - g * g) / pow(1.0 + g * g - 2.0 * g * cos_view, 1.5) * 0.35;
                        let intensity = select(4.0, 6.8, effects_tier >= 3u);
                        flash_scatter = spot * spot * atten * mie * camera.flashlight_dir.w * intensity;
                    }
                }
            }

            let step_extinction = exp(-(mist_density + flash_scatter * 0.35) * step_size);
            extinction *= step_extinction;

            let flash_color = vec3<f32>(1.0, 0.97, 0.88);
            let mist_color = vec3<f32>(0.05, 0.09, 0.18) * (1.0 + moon_mie * 1.5);
            let step_inscatter = (flash_scatter * flash_color + mist_density * mist_color) * step_size;
            accum_light += step_inscatter * (1.0 - step_extinction);

            march_t += step_size;
        }

        scene_color = scene_color * extinction + accum_light;
    }

    // ------------------------------------------------------------------------
    // 3. MULTI-TAP CINEMATIC BLOOM (Post-Processing Tier)
    // ------------------------------------------------------------------------
    if (post_tier >= 1u) {
        let bloom_thresh = select(0.85, select(0.70, 0.55, post_tier >= 3u), post_tier >= 2u);
        let bloom_intensity = select(0.20, select(0.45, 0.85, post_tier >= 3u), post_tier >= 2u);

        let b0 = textureSample(t_scene, s_linear, in.uv + vec2<f32>(-0.003, -0.003)).rgb;
        let b1 = textureSample(t_scene, s_linear, in.uv + vec2<f32>( 0.003, -0.003)).rgb;
        let b2 = textureSample(t_scene, s_linear, in.uv + vec2<f32>(-0.003,  0.003)).rgb;
        let b3 = textureSample(t_scene, s_linear, in.uv + vec2<f32>( 0.003,  0.003)).rgb;

        var bloom_accum = max(b0 - vec3<f32>(bloom_thresh), vec3<f32>(0.0)) +
                          max(b1 - vec3<f32>(bloom_thresh), vec3<f32>(0.0)) +
                          max(b2 - vec3<f32>(bloom_thresh), vec3<f32>(0.0)) +
                          max(b3 - vec3<f32>(bloom_thresh), vec3<f32>(0.0));
        bloom_accum *= 0.25;

        let bloom_tint = select(vec3<f32>(0.85, 0.92, 1.0), vec3<f32>(1.0, 0.97, 0.90), camera.flashlight_pos.w > 0.5);
        scene_color += bloom_accum * bloom_intensity * bloom_tint;
    }

    // ------------------------------------------------------------------------
    // 4. VIGNETTE & HORROR ATMOSPHERE
    // ------------------------------------------------------------------------
    if (post_tier >= 1u) {
        let dist_center = length(in.screen_pos * vec2<f32>(0.48, 0.62));
        let vig_power = select(0.35, select(0.60, 0.85, post_tier >= 3u), post_tier >= 2u);
        let vignette = clamp(1.0 - dist_center * dist_center * vig_power, 0.0, 1.0);
        scene_color *= (vignette * 0.88 + 0.12);
    }

    // ------------------------------------------------------------------------
    // 5. ANALOG 35MM FILM GRAIN
    // ------------------------------------------------------------------------
    if (post_tier >= 2u) {
        let grain_amount = select(0.015, 0.026, post_tier >= 3u);
        let grain = fract(sin(dot(in.uv * 1920.0, vec2<f32>(12.9898, 78.233)) + camera.settings.y * 36.0) * 43758.5453);
        scene_color += (grain - 0.5) * grain_amount;
    }

    // ------------------------------------------------------------------------
    // 6. ACES TONEMAPPING & GAMMA CORRECTION
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
