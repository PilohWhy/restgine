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
    @location(0) ray_dir: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    // Generate fullscreen triangle covering [-1, -1] to [3, -1], [-1, 3]
    let x = f32(i32(in_vertex_index & 1u) * 4 - 1);
    let y = f32(i32(in_vertex_index & 2u) * 2 - 1);
    // In Reversed-Z, 0.0 is the infinite far plane
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    let near_h = camera.inv_view_proj * vec4<f32>(x, y, 1.0, 1.0);
    out.ray_dir = near_h.xyz / max(near_h.w, 0.0001) - camera.camera_pos.xyz;
    return out;
}

// Pseudo-random hash for starfield & lunar craters
fn hash(p: vec3<f32>) -> f32 {
    let p3 = fract(p * 0.1031);
    let d = dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * d);
}

fn hash2d(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Smooth 2D value noise for lunar maria, Milky Way nebula, and high-altitude cirrus clouds (Zero trig!)
fn noise2d(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);

    let a = hash2d(i);
    let b = hash2d(i + vec2<f32>(1.0, 0.0));
    let c = hash2d(i + vec2<f32>(0.0, 1.0));
    let d = hash2d(i + vec2<f32>(1.0, 1.0));

    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let ray_dir = normalize(in.ray_dir);

    let sun_dir = normalize(camera.light_dir.xyz);
    let sun_dot = max(dot(ray_dir, sun_dir), 0.0);

    // 1. Rayleigh & Mie Nocturnal Atmospheric Gradient:
    let zenith_color = vec3<f32>(0.008, 0.016, 0.052);
    let mid_color = vec3<f32>(0.022, 0.042, 0.088);
    let horizon_color = vec3<f32>(0.04, 0.07, 0.13);
    let ground_color = vec3<f32>(0.02, 0.03, 0.05);

    let up = ray_dir.y;
    var sky: vec3<f32>;
    if (up > 0.0) {
        // Optical airmass approximation along zenith angle
        let optical_horizon = exp(-up * 4.2);
        let t_mid = pow(up, 0.72);
        sky = mix(zenith_color, mid_color, 1.0 - t_mid);
        sky = mix(sky, horizon_color, optical_horizon);
        // Subtle silver-cyan Rayleigh sky glow toward the moon azimuth
        sky += vec3<f32>(0.025, 0.045, 0.085) * pow(sun_dot, 3.0) * (1.0 - up * 0.5);
    } else {
        sky = mix(horizon_color, ground_color, clamp(-up * 4.0, 0.0, 1.0));
    }

    let time = camera.settings.y;

    // 2. High-Altitude Cirrus Clouds & Milky Way Band in Upper Hemisphere
    var cloud_density: f32 = 0.0;
    if (up > 0.04) {
        let dome_uv = ray_dir.xz / (up + 0.18);
        let horizon_fade = smoothstep(0.04, 0.24, up);

        // Diagonal Milky Way Galactic Band & Dark Rift Dust Lanes
        let gal_coord = vec2<f32>(
            ray_dir.x * 0.78 - ray_dir.z * 0.62,
            ray_dir.x * 0.62 + ray_dir.z * 0.78 - (up - 0.52) * 0.85
        );
        let gal_band = exp(-gal_coord.y * gal_coord.y * 8.5);
        if (gal_band > 0.015) {
            let dust_n1 = noise2d(gal_coord * 4.2 + vec2<f32>(2.1, -1.3));
            let dust_n2 = noise2d(gal_coord * 9.5 - vec2<f32>(4.7, 3.2));
            let dust_n3 = noise2d(gal_coord * 18.0 + vec2<f32>(1.2, 5.8));
            let dark_rift = smoothstep(0.32, 0.75, dust_n1 * 0.55 + dust_n2 * 0.30 + dust_n3 * 0.15);
            let mw_occlusion = 1.0 - smoothstep(0.82, 0.999, sun_dot);
            let core_bulge = exp(-dot(gal_coord, gal_coord) * 4.5);
            let mw_core_color = mix(vec3<f32>(0.08, 0.13, 0.24), vec3<f32>(0.22, 0.18, 0.14), core_bulge * 0.65);
            let mw_glow = gal_band * dark_rift * horizon_fade * mw_occlusion * (1.0 + core_bulge * 1.8);
            sky += mw_core_color * mw_glow;
        }

        // Drifting High-Altitude Wispy Cirrus Clouds (Domain-Warped Fibratus)
        let wind_uv = dome_uv * 1.25 + vec2<f32>(time * 0.007, time * 0.004);
        let warp = vec2<f32>(
            noise2d(wind_uv * 1.8 + vec2<f32>(1.7, -2.1)),
            noise2d(wind_uv * 1.8 - vec2<f32>(3.2, 1.4))
        );
        let warped_uv = wind_uv + warp * 0.35;
        let c1 = noise2d(warped_uv);
        let c2 = noise2d(warped_uv * 2.6 + vec2<f32>(c1 * 0.7, -time * 0.005));
        let c3 = noise2d(warped_uv * 5.8 - vec2<f32>(time * 0.003, c2 * 0.5));
        let cirrus_fbm = c1 * 0.50 + c2 * 0.32 + c3 * 0.18;
        cloud_density = smoothstep(0.52, 0.85, cirrus_fbm) * horizon_fade;

        let moon_scatter = pow(sun_dot, 6.0) * 0.95 + pow(sun_dot, 28.0) * 1.6 + 0.22;
        let cirrus_color = vec3<f32>(0.16, 0.24, 0.38) * moon_scatter;
        sky = mix(sky, sky + cirrus_color, cloud_density * 0.65);
    }

    // 3. Multi-Magnitude Blackbody Colored Starfield (Occluded by Moon & Cirrus Clouds)
    if (up > 0.05 && sun_dot < 0.9985) {
        let star_coord = floor(ray_dir * 200.0);
        let star_rand = hash(star_coord);
        if (star_rand > 0.976) {
            let temp_hash = fract(star_rand * 43.17);
            // Blackbody stellar chromaticity: blue-white (Rigel/Vega), gold (Capella), orange-red (Betelgeuse)
            var star_color = vec3<f32>(0.85, 0.92, 1.0);
            if (temp_hash < 0.30) {
                star_color = vec3<f32>(0.72, 0.86, 1.15);
            } else if (temp_hash > 0.78) {
                star_color = vec3<f32>(1.12, 0.84, 0.66);
            } else if (temp_hash > 0.55) {
                star_color = vec3<f32>(1.04, 0.96, 0.84);
            }

            // Atmospheric scintillation is stronger at low elevation (high airmass) and calmer near zenith
            let scint_depth = mix(0.55, 0.20, clamp(up, 0.0, 1.0));
            let twinkle = 1.0 - scint_depth * (0.5 - 0.5 * sin(time * 3.4 + star_rand * 71.0));
            let mag = pow((star_rand - 0.976) / (1.0 - 0.976), 1.35);
            let star_fade = smoothstep(0.05, 0.26, up) * (1.0 - cloud_density * 0.80);
            sky += star_color * (mag * twinkle * 2.1 * star_fade);
        }
    }

    // 4. Procedural Lunar Disc (Basaltic Maria, Rayed Craters & Lommel-Seeliger Limb Darkening)
    //    + Atmospheric Diffraction Corona Halo
    let moon_light_color = vec3<f32>(0.86, 0.93, 1.0);
    let inner_aureole = pow(sun_dot, 80.0) * 1.25;
    let outer_corona = pow(sun_dot, 16.0) * 0.45;
    let corona_ring = smoothstep(0.992, 0.996, sun_dot) * (1.0 - smoothstep(0.996, 0.9985, sun_dot));
    let ring_color = vec3<f32>(0.75, 0.92, 1.15) * (corona_ring * 0.40);
    sky += (inner_aureole + outer_corona) * moon_light_color + ring_color;

    // Lunar angular radius: cos(theta_moon) = 0.9991 (~2.4 deg stylized cinematic full moon)
    let cos_moon_edge = 0.9991;
    if (sun_dot > 0.9989) {
        let up_ref = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(sun_dir.y) > 0.99);
        let moon_right = normalize(cross(up_ref, sun_dir));
        let moon_up = cross(sun_dir, moon_right);
        let sin_moon_rad = sqrt(1.0 - cos_moon_edge * cos_moon_edge); // ~0.0424

        let local_uv = vec2<f32>(
            dot(ray_dir, moon_right) / sin_moon_rad,
            dot(ray_dir, moon_up) / sin_moon_rad
        );
        let r2 = dot(local_uv, local_uv);
        let disc_mask = smoothstep(1.04, 0.92, r2);

        if (disc_mask > 0.0) {
            let sphere_z = sqrt(max(1.0 - r2, 0.0));
            // Lommel-Seeliger / Minnaert lunar regolith limb profile (opposition surge + soft limb)
            let limb = mix(0.68, 1.08, pow(sphere_z, 0.42));

            // Multi-octave basaltic lunar maria (Mare Imbrium / Serenitatis / Procellarum)
            let m1 = noise2d(local_uv * 2.2 + vec2<f32>(1.4, -0.8));
            let m2 = noise2d(local_uv * 4.8 - vec2<f32>(2.7, 1.9));
            let m3 = noise2d(local_uv * 10.5 + vec2<f32>(0.6, 3.1));
            let maria_fbm = m1 * 0.55 + m2 * 0.30 + m3 * 0.15;
            let maria_mask = smoothstep(0.42, 0.68, maria_fbm);

            // Bright rayed highland impact craters (Tycho / Copernicus)
            let crater_ring = smoothstep(0.72, 0.88, m3) * (1.0 - maria_mask * 0.6);

            let highland_albedo = vec3<f32>(0.92, 0.96, 1.02);
            let maria_albedo = vec3<f32>(0.46, 0.53, 0.64);
            let lunar_surface = (mix(highland_albedo, maria_albedo, maria_mask * 0.72) + vec3<f32>(crater_ring * 0.24)) * limb;

            sky += lunar_surface * disc_mask * 4.6 * (1.0 - cloud_density * 0.35);
        }
    }

    return vec4<f32>(sky, 1.0);
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
