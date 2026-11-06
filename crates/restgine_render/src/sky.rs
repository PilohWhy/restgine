pub const SKY_SHADER_WGSL: &str = r#"
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

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) screen_pos: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    // Generate fullscreen triangle covering [-1, -1] to [3, -1], [-1, 3]
    let x = f32(i32(in_vertex_index & 1u) * 4 - 1);
    let y = f32(i32(in_vertex_index & 2u) * 2 - 1);
    // In Reversed-Z, 0.0 is the infinite far plane
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.screen_pos = vec2<f32>(x, y);
    return out;
}

// Pseudo-random hash for starfield
fn hash(p: vec3<f32>) -> f32 {
    let p3 = fract(p * 0.1031);
    let d = dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * d);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Reconstruct world ray direction
    let clip_pos = vec4<f32>(in.screen_pos.x, in.screen_pos.y, 0.5, 1.0);
    let world_h = camera.inv_view_proj * clip_pos;
    let ray_dir = normalize(world_h.xyz / world_h.w - camera.camera_pos.xyz);

    let sun_dir = normalize(camera.light_dir.xyz);
    let sun_dot = max(dot(ray_dir, sun_dir), 0.0);

    // Atmospheric Dusk / Twilight Gradient
    // Zenith (straight up): deep rich twilight blue
    let zenith_color = vec3<f32>(0.03, 0.06, 0.14);
    // Mid sky: indigo-azure
    let mid_color = vec3<f32>(0.08, 0.15, 0.28);
    // Horizon glow: warm sunset/dusk amber & twilight peach
    let horizon_color = vec3<f32>(0.38, 0.24, 0.28);
    // Below horizon ground tone
    let ground_color = vec3<f32>(0.04, 0.06, 0.09);

    let up = ray_dir.y;
    var sky: vec3<f32>;
    if (up > 0.0) {
        let t1 = pow(1.0 - up, 2.5);
        let t2 = pow(up, 0.75);
        sky = mix(zenith_color, mid_color, 1.0 - t2);
        sky = mix(sky, horizon_color, t1);
    } else {
        sky = mix(horizon_color, ground_color, clamp(-up * 4.0, 0.0, 1.0));
    }

    // Sun / Moon Disc & Atmospheric Halo
    let sun_disc = smoothstep(0.9985, 0.9995, sun_dot);
    let sun_halo = pow(sun_dot, 8.0) * 0.45 + pow(sun_dot, 48.0) * 0.85;
    let sun_light_color = vec3<f32>(1.0, 0.90, 0.75);
    sky += (sun_disc * 4.0 + sun_halo) * sun_light_color;

    // Twinkling Starfield in Upper Hemisphere
    if (up > 0.06) {
        let star_coord = floor(ray_dir * 180.0);
        let star_rand = hash(star_coord);
        if (star_rand > 0.978) {
            let twinkle = sin(camera.settings.y * 3.0 + star_rand * 62.0) * 0.5 + 0.5;
            let star_bright = (star_rand - 0.978) / (1.0 - 0.978) * twinkle;
            let star_fade = smoothstep(0.06, 0.30, up);
            sky += vec3<f32>(0.85, 0.92, 1.0) * (star_bright * 1.6 * star_fade);
        }
    }

    // ACES Filmic Tonemapping
    let a: f32 = 2.51;
    let b: f32 = 0.03;
    let c: f32 = 2.43;
    let d: f32 = 0.59;
    let e: f32 = 0.14;
    let tonemapped = clamp((sky * (a * sky + b)) / (sky * (c * sky + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));

    // Dynamic Gamma
    let gamma = camera.settings.x;
    let gamma_corrected = pow(tonemapped, vec3<f32>(1.0 / max(gamma, 0.01)));

    return vec4<f32>(gamma_corrected, 1.0);
}
"#;

pub struct SkyPipeline {
    pub pipeline: wgpu::RenderPipeline,
}

impl SkyPipeline {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sky Shader Module"),
            source: wgpu::ShaderSource::Wgsl(SKY_SHADER_WGSL.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sky Pipeline Layout"),
            bind_group_layouts: &[camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sky Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[], // Procedural fullscreen triangle via vertex_index
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
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: false,
                // In Reversed-Z, 0.0 is the far plane. Sky is drawn with Z=0.0.
                // GreaterEqual allows drawing where depth == 0.0 (where no geometry was rendered).
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self { pipeline }
    }
}
