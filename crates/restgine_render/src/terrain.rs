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
    let s4 = (x * 0.48).sin() * (z * 0.45).cos() * 0.08;
    let s5 = (x * 1.15 + 0.5).cos() * (z * 1.05 - 0.4).sin() * 0.035;
    s1 + s2 + s3 + s4 + s5 + 1.0
}

/// Continuous analytical surface normal computed from exact partial derivatives (dy/dx, dy/dz)
/// across all 5 terrain harmonics (zero finite-difference approximation error, 4x fewer trig ops).
#[inline]
pub fn terrain_normal(x: f32, z: f32) -> Vec3 {
    let (s1x, c1x) = (x * 0.025).sin_cos();
    let (s1z, c1z) = (z * 0.025).sin_cos();

    let (s2x, c2x) = (x * 0.07 + 1.3).sin_cos();
    let (s2z, c2z) = (z * 0.06 - 0.7).sin_cos();

    let (s3x, c3x) = (x * 0.18 - 2.1).sin_cos();
    let (s3z, c3z) = (z * 0.15 + 1.8).sin_cos();

    let (s4x, c4x) = (x * 0.48).sin_cos();
    let (s4z, c4z) = (z * 0.45).sin_cos();

    let (s5x, c5x) = (x * 1.15 + 0.5).sin_cos();
    let (s5z, c5z) = (z * 1.05 - 0.4).sin_cos();

    let dydx = 0.02375 * c1x * c1z
        + 0.02800 * c2x * c2z
        - 0.02700 * s3x * s3z
        + 0.03840 * c4x * c4z
        - 0.04025 * s5x * s5z;

    let dydz = -0.02375 * s1x * s1z
        - 0.02400 * s2x * s2z
        + 0.02250 * c3x * c3z
        - 0.03600 * s4x * s4z
        + 0.03675 * c5x * c5z;

    Vec3::new(-dydx, 1.0, -dydz).normalize()
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

// Unified single-pass evaluation of terrain height (x) and exact analytical surface normal (yzw)
fn get_terrain_surface(x: f32, z: f32) -> vec4<f32> {
    let ax1 = x * 0.025; let az1 = z * 0.025;
    let ax2 = x * 0.07 + 1.3; let az2 = z * 0.06 - 0.7;
    let ax3 = x * 0.18 - 2.1; let az3 = z * 0.15 + 1.8;
    let ax4 = x * 0.48; let az4 = z * 0.45;
    let ax5 = x * 1.15 + 0.5; let az5 = z * 1.05 - 0.4;

    let s1x = sin(ax1); let c1x = cos(ax1); let s1z = sin(az1); let c1z = cos(az1);
    let s2x = sin(ax2); let c2x = cos(ax2); let s2z = sin(az2); let c2z = cos(az2);
    let s3x = sin(ax3); let c3x = cos(ax3); let s3z = sin(az3); let c3z = cos(az3);
    let s4x = sin(ax4); let c4x = cos(ax4); let s4z = sin(az4); let c4z = cos(az4);
    let s5x = sin(ax5); let c5x = cos(ax5); let s5z = sin(az5); let c5z = cos(az5);

    let h = s1x * c1z * 0.95
          + s2x * c2z * 0.40
          + c3x * s3z * 0.15
          + s4x * c4z * 0.08
          + c5x * s5z * 0.035
          + 1.0;

    let dydx = 0.02375 * c1x * c1z
             + 0.02800 * c2x * c2z
             - 0.02700 * s3x * s3z
             + 0.03840 * c4x * c4z
             - 0.04025 * s5x * s5z;

    let dydz = -0.02375 * s1x * s1z
             - 0.02400 * s2x * s2z
             + 0.02250 * c3x * c3z
             - 0.03600 * s4x * s4z
             + 0.03675 * c5x * c5z;

    let n = normalize(vec3<f32>(-dydx, 1.0, -dydz));
    return vec4<f32>(h, n.x, n.y, n.z);
}

fn ign(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
}

// Physical Microfacet Cook-Torrance GGX BRDF Terms
fn d_ggx(ndoth: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = max(a * a, 0.0001);
    let d = (ndoth * a2 - ndoth) * ndoth + 1.0;
    return a2 / (3.14159265 * d * d + 0.00001);
}

fn v_smith_ggx(ndotv: f32, ndotl: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let gv = ndotl * sqrt(ndotv * ndotv * (1.0 - a2) + a2);
    let gl = ndotv * sqrt(ndotl * ndotl * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 0.00001);
}

fn f_schlick(vdoth: f32, f0: f32) -> f32 {
    let x = clamp(1.0 - vdoth, 0.0, 1.0);
    let x2 = x * x;
    return f0 + (1.0 - f0) * (x2 * x2 * x);
}

fn get_poisson_16(idx: u32) -> vec2<f32> {
    switch (idx) {
        case 0u: { return vec2<f32>(-0.94201624, -0.39906216); }
        case 1u: { return vec2<f32>( 0.94558609, -0.76890725); }
        case 2u: { return vec2<f32>(-0.09418410, -0.92938870); }
        case 3u: { return vec2<f32>( 0.34495938,  0.29387760); }
        case 4u: { return vec2<f32>(-0.91588581,  0.45771432); }
        case 5u: { return vec2<f32>(-0.81544232, -0.87912464); }
        case 6u: { return vec2<f32>(-0.38277543,  0.27676845); }
        case 7u: { return vec2<f32>( 0.97484398,  0.75648377); }
        case 8u: { return vec2<f32>( 0.44323325, -0.97511554); }
        case 9u: { return vec2<f32>( 0.53742981, -0.47373420); }
        case 10u: { return vec2<f32>(-0.26496911, -0.41893023); }
        case 11u: { return vec2<f32>( 0.79197514,  0.19090188); }
        case 12u: { return vec2<f32>(-0.24188840,  0.99706507); }
        case 13u: { return vec2<f32>(-0.81409955,  0.91437590); }
        case 14u: { return vec2<f32>( 0.19984126,  0.78641367); }
        default: { return vec2<f32>( 0.14383161, -0.14100790); }
    }
}

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let world_x = in.local_pos.x + inst.chunk_offset.x;
    let world_z = in.local_pos.z + inst.chunk_offset.y;
    let surf = get_terrain_surface(world_x, world_z);
    let world_pos = vec3<f32>(world_x, surf.x, world_z);

    out.world_pos = world_pos;
    out.world_normal = surf.yzw;
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
    let n = normalize(in.world_normal);
    let to_cam = camera.camera_pos.xyz - in.world_pos;
    let cam_dist = length(to_cam);
    let view_dir = to_cam / max(cam_dist, 0.0001);

    // 1. Parallax Height Displacement (near-field < 24m) & Dual-Scale Stochastic Anti-Tiling
    let ao_rough = textureSample(t_ao_roughness, s_ao_roughness, in.uv);
    let ao = ao_rough.r;
    let raw_roughness = ao_rough.g;
    let height = ao_rough.b;

    let t = normalize(vec3<f32>(1.0, 0.0, 0.0) - n * n.x);
    let bitan = cross(t, n);
    let parallax_fade = clamp((24.0 - cam_dist) / 10.0, 0.0, 1.0);
    let v_tan = vec3<f32>(dot(view_dir, t), dot(view_dir, bitan), dot(view_dir, n));
    let parallax_offset = (v_tan.xy / max(v_tan.z, 0.22)) * ((height - 0.5) * 0.024 * parallax_fade);
    let uv = in.uv - parallax_offset;

    let micro_albedo = textureSample(t_albedo, s_albedo, uv).rgb;

    var normal = n;
    var grass_contact: f32 = 0.75;

    if (cam_dist < 48.0) {
        let norm_mip = clamp(log2(max(cam_dist * 0.22, 1.0)), 0.0, 5.0);
        let raw_normal = textureSampleLevel(t_normal, s_normal, uv, norm_mip).xyz * 2.0 - 1.0;
        let detail_fade = clamp((48.0 - cam_dist) / 12.0, 0.0, 1.0);
        let tbn = mat3x3<f32>(t, bitan, n);
        normal = normalize(tbn * (raw_normal * vec3<f32>(2.1 * detail_fade, 2.1 * detail_fade, 1.0)));

        let clump_shadow_pattern = sin(in.world_pos.x * 2.8 + sin(in.world_pos.z * 1.5)) * cos(in.world_pos.z * 2.8 + cos(in.world_pos.x * 1.5));
        grass_contact = mix(0.75, clamp(clump_shadow_pattern * 0.35 + 0.75, 0.45, 1.0), detail_fade);
    }

    // Dual-scale macro soil/moss organic variation breaks up 4m x 4m texture repetition across clearings
    let macro_wave = sin(in.world_pos.x * 0.19 + cos(in.world_pos.z * 0.17))
                   * cos(in.world_pos.z * 0.21 - sin(in.world_pos.x * 0.14));
    let moss_patch = smoothstep(-0.2, 0.65, macro_wave);
    let macro_tint = mix(vec3<f32>(0.86, 0.82, 0.78), vec3<f32>(0.92, 1.06, 0.88), moss_patch);
    let albedo = micro_albedo * macro_tint;
    let roughness = clamp(raw_roughness * mix(1.06, 0.88, moss_patch), 0.05, 0.98);

    // 2. Topographical Valley Puddle Water Accumulation:
    // Standing rainwater pools naturally in low-lying terrain swales (y < 1.05m) and flat basins
    let valley_swale = clamp((1.05 - in.world_pos.y) * 1.45, 0.0, 1.0) * smoothstep(0.92, 0.992, n.y);
    let water_level = mix(0.28, 0.58, valley_swale);
    let puddle_depth = clamp((water_level - height) * 9.5, 0.0, 1.0) * (0.20 + 0.80 * valley_swale);

    // Subtle dual-octave capillary wind ripple perturbation on water surface
    let time = camera.settings.y;
    let rip1 = sin(time * 2.2 + in.world_pos.x * 6.5) * cos(time * 2.6 + in.world_pos.z * 6.5) * 0.010;
    let rip2 = sin(time * 4.2 - in.world_pos.x * 13.5) * cos(time * 4.6 + in.world_pos.z * 13.5) * 0.005;
    let ripple = rip1 + rip2;
    let water_normal = normalize(mix(normal, vec3<f32>(ripple, 1.0, ripple), puddle_depth * 0.95));

    // Standing water flattens surface normal into a calm mirror and darkens porous soil via internal reflection
    normal = normalize(mix(normal, water_normal, puddle_depth * 0.90));
    let surface_albedo = mix(albedo, albedo * 0.28, puddle_depth * 0.92);
    let surface_roughness = mix(roughness, 0.02, puddle_depth * 0.98);
    let f0 = mix(0.04, 0.02, puddle_depth);

    let light_dir = normalize(camera.light_dir.xyz);

    // 3. High-Sample 16-Tap Rotated Poisson PCF Shadow Filtering
    var shadow: f32 = 1.0;
    let shadow_tier = camera.quality_tiers.x;
    if (cam_dist < 72.0 && shadow_tier > 0u && in.shadow_pos.x >= 0.0 && in.shadow_pos.x <= 1.0 && in.shadow_pos.y >= 0.0 && in.shadow_pos.y <= 1.0 && in.shadow_pos.z >= 0.0 && in.shadow_pos.z <= 1.0) {
        let bias = 0.0026;
        let shadow_depth = in.shadow_pos.z - bias;

        if (shadow_tier == 1u) {
            shadow = textureSampleCompareLevel(t_shadow, s_shadow, in.shadow_pos.xy, shadow_depth);
        } else {
            let texel_size = 1.0 / 2048.0;
            let spread = texel_size * select(1.5, 2.2, shadow_tier >= 3u);

            var shadow_sum: f32 = 0.0;
            for (var p = 0u; p < 16u; p = p + 1u) {
                let offset = get_poisson_16(p) * spread;
                shadow_sum += textureSampleCompareLevel(t_shadow, s_shadow, in.shadow_pos.xy + offset, shadow_depth);
            }
            shadow = shadow_sum * (1.0 / 16.0);
        }
    }

    // 4. Ambient Lighting & High-Contrast Crevice + Foliage Contact AO
    let gi_tier = camera.quality_tiers.y;
    let ambient_factor = select(0.14, select(0.20, 0.28, gi_tier >= 2u), gi_tier >= 1u);

    let crevice_ao = ao * ao * sqrt(ao) * mix(0.36, 1.0, height);
    let final_ao = crevice_ao * grass_contact;

    let sky_ambient = mix(vec3<f32>(0.06, 0.10, 0.18), vec3<f32>(0.11, 0.17, 0.29), normal.y * 0.5 + 0.5);
    let ambient = sky_ambient * (ambient_factor * final_ao);

    // 5. Physical Microfacet Cook-Torrance GGX Moonlight Direct Lighting
    let ndotl = max(dot(normal, light_dir), 0.0);
    let ndotv = max(dot(normal, view_dir), 0.001);
    let half_dir = normalize(light_dir + view_dir);
    let ndoth = max(dot(normal, half_dir), 0.0);
    let vdoth = max(dot(view_dir, half_dir), 0.0);

    let sun_color = vec3<f32>(0.85, 0.92, 1.0);
    let f_moon = f_schlick(vdoth, f0);
    let d_moon = d_ggx(ndoth, surface_roughness);
    let v_moon = v_smith_ggx(ndotv, ndotl, surface_roughness);
    let spec_moon = min(d_moon * v_moon * f_moon, 6.0) * ndotl * shadow * (1.0 + puddle_depth * 2.2);
    let diff_moon = (1.0 - f_moon) * ndotl * shadow * final_ao;

    var lit_color = ambient * surface_albedo + (diff_moon * surface_albedo + spec_moon) * sun_color;

    // 6. Glassy Mirror Sky, Trees & Lunar Reflection in Water Puddles
    if (puddle_depth > 0.05) {
        let refl_ray = reflect(-view_dir, normal);
        if (refl_ray.y > 0.0) {
            let sky_zenith = vec3<f32>(0.008, 0.016, 0.052);
            let sky_mid = vec3<f32>(0.022, 0.042, 0.088);
            let sky_horiz = vec3<f32>(0.04, 0.07, 0.13);
            let r_up = refl_ray.y;
            let refl_sky = mix(mix(sky_horiz, sky_mid, clamp(r_up * 3.0, 0.0, 1.0)), sky_zenith, pow(r_up, 0.72));

            // Reflected Lunar Disc & Corona in standing water
            let refl_moon_dot = max(dot(refl_ray, light_dir), 0.0);
            let refl_moon_disc = smoothstep(0.9985, 0.9995, refl_moon_dot) * 7.5;
            let refl_corona = (pow(refl_moon_dot, 48.0) * 1.2 + pow(refl_moon_dot, 14.0) * 0.35) * vec3<f32>(0.86, 0.93, 1.0);
            let sky_reflection = refl_sky + (refl_corona + vec3<f32>(0.92, 0.96, 1.0) * refl_moon_disc) * shadow;

            // Horizon tree canopy silhouettes reflected along clearing edges
            let tree_angle = atan2(refl_ray.z, refl_ray.x);
            let tree_canopy_noise = sin(tree_angle * 12.0) * 0.035 + cos(tree_angle * 26.0) * 0.02 + 0.12;
            let tree_refl_mask = smoothstep(tree_canopy_noise + 0.04, tree_canopy_noise - 0.02, r_up);
            let tree_refl_color = vec3<f32>(0.006, 0.009, 0.015);
            let composite_reflection = mix(sky_reflection, tree_refl_color, tree_refl_mask * 0.85);

            // True nocturnal water Fresnel curve (45% reflectivity looking straight down, 100% at grazing angle)
            let f_water = mix(0.45, 1.0, pow(clamp(1.0 - ndotv, 0.0, 1.0), 3.5));
            lit_color = mix(lit_color, composite_reflection, puddle_depth * f_water);
        }
    }

    // 7. Dual-Zone Tactical Flashlight with Reflector Caustics & GGX Water Specular
    if (camera.flashlight_pos.w > 0.5) {
        let to_light = camera.flashlight_pos.xyz - in.world_pos;
        let proj = -dot(to_light, camera.flashlight_dir.xyz);
        let dist_sq = dot(to_light, to_light);
        if (proj > 0.0 && dist_sq < 2025.0 && proj * proj > 0.7744 * dist_sq) {
            let inv_dist = inverseSqrt(max(dist_sq, 0.000001));
            let dist = dist_sq * inv_dist;
            let l_dir = to_light * inv_dist;
            let spot_dot = proj * inv_dist;

            let hotspot = smoothstep(0.965, 0.992, spot_dot);
            let flood = smoothstep(0.86, 0.965, spot_dot);
            let beam_profile = hotspot * 0.65 + flood * 0.22;

            let near_soften = smoothstep(0.4, 2.0, dist);
            let atten = (1.0 / (1.0 + 0.09 * dist + 0.022 * dist_sq)) * near_soften;
            let dist_cutoff = clamp((50.0 - dist) / 10.0, 0.0, 1.0);

            let f_ndotl = max(dot(normal, l_dir), 0.0);
            let half_flash = normalize(l_dir + view_dir);
            let f_ndoth = max(dot(normal, half_flash), 0.0);
            let f_vdoth = max(dot(view_dir, half_flash), 0.0);

            let ff = f_schlick(f_vdoth, f0);
            let fd = d_ggx(f_ndoth, surface_roughness);
            let fv = v_smith_ggx(ndotv, f_ndotl, surface_roughness);
            let flash_spec = min(fd * fv * ff, 0.35) * f_ndotl * (0.6 + puddle_depth * 1.0);

            let flash_color = vec3<f32>(1.0, 0.97, 0.88);
            let flash_power = beam_profile * atten * dist_cutoff * camera.flashlight_dir.w * 0.52;

            lit_color += ((1.0 - ff) * f_ndotl * surface_albedo * final_ao + flash_spec * final_ao) * flash_color * flash_power;
        }
    }

    // 7. Atmospheric Midnight Horizon Fog
    let view_dist = camera.settings.w;
    let fog_start = view_dist * 0.42;
    let fog_end = view_dist * 0.90;
    let fog_factor = clamp((cam_dist - fog_start) / (fog_end - fog_start + 0.001), 0.0, 1.0);
    let smooth_fog = fog_factor * fog_factor * (3.0 - 2.0 * fog_factor);
    let horizon_fog_color = vec3<f32>(0.04, 0.07, 0.13);
    lit_color = mix(lit_color, horizon_fog_color, smooth_fog);

    return vec4<f32>(lit_color, 1.0);
}
"#;

#[inline]
fn push_ccw_tri(indices: &mut Vec<u16>, stride: u16, a: (u16, u16), b: (u16, u16), c: (u16, u16)) {
    let cross = (b.1 as i32 - a.1 as i32) * (c.0 as i32 - a.0 as i32)
        - (b.0 as i32 - a.0 as i32) * (c.1 as i32 - a.1 as i32);
    let ia = a.1 * stride + a.0;
    let ib = b.1 * stride + b.0;
    let ic = c.1 * stride + c.0;
    if cross > 0 {
        indices.extend_from_slice(&[ia, ib, ic]);
    } else if cross < 0 {
        indices.extend_from_slice(&[ia, ic, ib]);
    }
}

fn append_grid_fill(indices: &mut Vec<u16>, stride: u16, min_c: u16, max_c: u16, step: u16) {
    for z in (min_c..max_c).step_by(step as usize) {
        for x in (min_c..max_c).step_by(step as usize) {
            push_ccw_tri(indices, stride, (x, z), (x, z + step), (x + step, z));
            push_ccw_tri(
                indices,
                stride,
                (x + step, z),
                (x, z + step),
                (x + step, z + step),
            );
        }
    }
}

fn append_ring_transition(
    indices: &mut Vec<u16>,
    stride: u16,
    outer_min: u16,
    outer_max: u16,
    s: u16,
) {
    let inner_step = s * 2;
    let inner_min = outer_min + inner_step;
    let inner_max = outer_max - inner_step;

    // 4 Corners (2s x 2s each, fanning from inner corner to 4 outer boundary segments of length s)
    // Top-Left
    push_ccw_tri(
        indices,
        stride,
        (inner_min, inner_min),
        (outer_min, inner_min),
        (outer_min, outer_min + s),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_min, inner_min),
        (outer_min, outer_min + s),
        (outer_min, outer_min),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_min, inner_min),
        (outer_min, outer_min),
        (outer_min + s, outer_min),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_min, inner_min),
        (outer_min + s, outer_min),
        (inner_min, outer_min),
    );

    // Top-Right
    push_ccw_tri(
        indices,
        stride,
        (inner_max, inner_min),
        (inner_max, outer_min),
        (inner_max + s, outer_min),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_max, inner_min),
        (inner_max + s, outer_min),
        (outer_max, outer_min),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_max, inner_min),
        (outer_max, outer_min),
        (outer_max, outer_min + s),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_max, inner_min),
        (outer_max, outer_min + s),
        (outer_max, inner_min),
    );

    // Bottom-Left
    push_ccw_tri(
        indices,
        stride,
        (inner_min, inner_max),
        (outer_min, inner_max),
        (outer_min, inner_max + s),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_min, inner_max),
        (outer_min, inner_max + s),
        (outer_min, outer_max),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_min, inner_max),
        (outer_min, outer_max),
        (outer_min + s, outer_max),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_min, inner_max),
        (outer_min + s, outer_max),
        (inner_min, outer_max),
    );

    // Bottom-Right
    push_ccw_tri(
        indices,
        stride,
        (inner_max, inner_max),
        (inner_max, outer_max),
        (inner_max + s, outer_max),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_max, inner_max),
        (inner_max + s, outer_max),
        (outer_max, outer_max),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_max, inner_max),
        (outer_max, outer_max),
        (outer_max, inner_max + s),
    );
    push_ccw_tri(
        indices,
        stride,
        (inner_max, inner_max),
        (outer_max, inner_max + s),
        (outer_max, inner_max),
    );

    // 4 Side Transition Strips
    for t in (inner_min..inner_max).step_by(inner_step as usize) {
        // Top strip
        push_ccw_tri(indices, stride, (t, inner_min), (t, outer_min), (t + s, outer_min));
        push_ccw_tri(
            indices,
            stride,
            (t, inner_min),
            (t + s, outer_min),
            (t + inner_step, inner_min),
        );
        push_ccw_tri(
            indices,
            stride,
            (t + inner_step, inner_min),
            (t + s, outer_min),
            (t + inner_step, outer_min),
        );

        // Bottom strip
        push_ccw_tri(indices, stride, (t, inner_max), (t, outer_max), (t + s, outer_max));
        push_ccw_tri(
            indices,
            stride,
            (t, inner_max),
            (t + s, outer_max),
            (t + inner_step, inner_max),
        );
        push_ccw_tri(
            indices,
            stride,
            (t + inner_step, inner_max),
            (t + s, outer_max),
            (t + inner_step, outer_max),
        );

        // Left strip
        push_ccw_tri(indices, stride, (inner_min, t), (outer_min, t), (outer_min, t + s));
        push_ccw_tri(
            indices,
            stride,
            (inner_min, t),
            (outer_min, t + s),
            (inner_min, t + inner_step),
        );
        push_ccw_tri(
            indices,
            stride,
            (inner_min, t + inner_step),
            (outer_min, t + s),
            (outer_min, t + inner_step),
        );

        // Right strip
        push_ccw_tri(indices, stride, (inner_max, t), (outer_max, t), (outer_max, t + s));
        push_ccw_tri(
            indices,
            stride,
            (inner_max, t),
            (outer_max, t + s),
            (inner_max, t + inner_step),
        );
        push_ccw_tri(
            indices,
            stride,
            (inner_max, t + inner_step),
            (outer_max, t + s),
            (outer_max, t + inner_step),
        );
    }
}

/// Builds consolidated crack-free stitched index buffer for Terrain LOD0 (1m), LOD1 (2m), and LOD2 (4m).
/// All three LOD levels share the exact same 33 vertices along all 4 outer chunk edges (step 1),
/// guaranteeing zero T-junctions or cracks between adjacent chunks of different LODs.
pub fn build_stitched_terrain_lod_indices() -> (Vec<u16>, [(u32, u32); 3]) {
    let stride = (TerrainPipeline::GRID_STEPS + 1) as u16;
    let mut indices = Vec::with_capacity(9200);

    // LOD 0: Full 32x32 1m grid (2,048 tris)
    let lod0_start = indices.len() as u32;
    append_grid_fill(&mut indices, stride, 0, 32, 1);
    let lod0_count = (indices.len() as u32) - lod0_start;

    // LOD 1: Stitched 1m border ring + 2m interior [2..30] (568 tris)
    let lod1_start = indices.len() as u32;
    append_ring_transition(&mut indices, stride, 0, 32, 1);
    append_grid_fill(&mut indices, stride, 2, 30, 2);
    let lod1_count = (indices.len() as u32) - lod1_start;

    // LOD 2: Stitched 1m outer ring + 2m transition ring + 4m interior [6..26] (302 tris)
    let lod2_start = indices.len() as u32;
    append_ring_transition(&mut indices, stride, 0, 32, 1);
    append_ring_transition(&mut indices, stride, 2, 30, 2);
    append_grid_fill(&mut indices, stride, 6, 26, 4);
    let lod2_count = (indices.len() as u32) - lod2_start;

    (
        indices,
        [
            (lod0_start, lod0_count),
            (lod1_start, lod1_count),
            (lod2_start, lod2_count),
        ],
    )
}

pub struct TerrainPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub chunk_mesh_vertex_buffer: wgpu::Buffer,
    pub chunk_mesh_index_buffer: wgpu::Buffer,
    pub chunk_mesh_index_count: u32,
    pub lod_index_ranges: [(u32, u32); 3],
    pub instance_buffer: wgpu::Buffer,
    pub material_bind_group: wgpu::BindGroup,
}

impl TerrainPipeline {
    pub const CHUNK_SIZE: f32 = 32.0;
    pub const GRID_STEPS: usize = 32; // Nyquist-Shannon optimal 1.0m resolution (saves 55% vertices with 0 geometric loss)

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
        // 1. Create Terrain Shared Unit Grid Mesh (32m x 32m at 1m resolution) + Stitched Multi-Tier LOD Index Buffer
        let mut vertices = Vec::with_capacity(1089);

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

        let (indices, lod_index_ranges) = build_stitched_terrain_lod_indices();

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

        let chunk_mesh_index_count = lod_index_ranges[0].1;

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
            label: Some("Terrain Albedo Repeat Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });

        let aux_repeat_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Terrain Aux Repeat Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 2,
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
                    resource: wgpu::BindingResource::Sampler(&aux_repeat_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&ao_rough_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&aux_repeat_sampler),
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
            lod_index_ranges,
            instance_buffer,
            material_bind_group,
        })
    }
}

/// Computes the fraction of texels `[0.0..=1.0]` whose alpha channel is `>= alpha_cutoff`.
#[inline]
pub fn compute_alpha_coverage(rgba: &[u8], alpha_cutoff: u8) -> f32 {
    let total = rgba.len() / 4;
    if total == 0 {
        return 1.0;
    }
    let mut passing = 0usize;
    for px in rgba.chunks_exact(4) {
        if px[3] >= alpha_cutoff {
            passing += 1;
        }
    }
    (passing as f32) / (total as f32)
}

/// Castano / NVIDIA alpha-coverage preservation: finds an alpha multiplier via 8-step binary search
/// so the downsampled mipmap level retains `target_coverage` at `alpha_cutoff`, preventing distant
/// grass and tree canopy leaves from thinning out or eroding at high mip levels.
pub fn scale_rgba_alpha_to_coverage(rgba: &mut [u8], target_coverage: f32, alpha_cutoff: u8) {
    let total = rgba.len() / 4;
    if total == 0 {
        return;
    }
    let target = target_coverage.clamp(0.01, 0.98);
    let cutoff_f = alpha_cutoff as f32;

    let mut lo = 0.5f32;
    let mut hi = 4.0f32;
    for _ in 0..8 {
        let mid = 0.5 * (lo + hi);
        let thresh = (cutoff_f / mid).ceil() as u32;
        let mut passing = 0usize;
        if thresh <= 255 {
            let thresh_u8 = thresh as u8;
            for px in rgba.chunks_exact(4) {
                if px[3] >= thresh_u8 {
                    passing += 1;
                }
            }
        }
        let cov = (passing as f32) / (total as f32);
        if cov < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    let best_scale = 0.5 * (lo + hi);
    if (best_scale - 1.0).abs() > 0.01 {
        for px in rgba.chunks_exact_mut(4) {
            px[3] = ((px[3] as f32 * best_scale).round() as u32).min(255) as u8;
        }
    }
}

/// Replaces the RGB channels of completely transparent texels (`alpha < 8`) with the mean RGB
/// of opaque texels (`alpha >= alpha_cutoff`), preventing black `(0, 0, 0, 0)` background texels
/// from darkening foliage/leaf edges during box/triangle mipmap downsampling.
pub fn flood_transparent_rgb_with_average(rgba: &mut [u8], alpha_cutoff: u8) {
    let mut sum_r = 0u64;
    let mut sum_g = 0u64;
    let mut sum_b = 0u64;
    let mut count = 0u64;

    for px in rgba.chunks_exact(4) {
        if px[3] >= alpha_cutoff {
            sum_r += px[0] as u64;
            sum_g += px[1] as u64;
            sum_b += px[2] as u64;
            count += 1;
        }
    }

    if count == 0 {
        return;
    }

    let avg_r = (sum_r / count) as u8;
    let avg_g = (sum_g / count) as u8;
    let avg_b = (sum_b / count) as u8;

    for px in rgba.chunks_exact_mut(4) {
        if px[3] < 8 {
            px[0] = avg_r;
            px[1] = avg_g;
            px[2] = avg_b;
        }
    }
}

pub fn create_texture_from_bytes(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bytes: &[u8],
    label: &str,
    is_srgb: bool,
) -> Result<(wgpu::Texture, wgpu::TextureView), Box<dyn std::error::Error>> {
    let mut current_img = image::load_from_memory(bytes)?.to_rgba8();
    let (base_width, base_height) = current_img.dimensions();

    // Detect alpha-cutout foliage/leaf textures (0.30..0.32 shader cutoff ~= 78/255)
    let alpha_cutoff = 78u8;
    let base_coverage = if is_srgb {
        compute_alpha_coverage(current_img.as_raw(), alpha_cutoff)
    } else {
        1.0
    };
    let preserve_alpha_coverage = is_srgb && base_coverage > 0.01 && base_coverage < 0.98;
    if preserve_alpha_coverage {
        flood_transparent_rgb_with_average(current_img.as_mut(), alpha_cutoff);
    }

    // Full power-of-two mipmap chain to eliminate texture aliasing and distant sparkling
    let mip_level_count = (base_width.max(base_height) as f32).log2().floor() as u32 + 1;

    let format = if is_srgb {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: base_width,
            height: base_height,
            depth_or_array_layers: 1,
        },
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let mut w = base_width;
    let mut h = base_height;

    for mip in 0..mip_level_count {
        if mip > 0 {
            w = (w / 2).max(1);
            h = (h / 2).max(1);
            current_img =
                image::imageops::resize(&current_img, w, h, image::imageops::FilterType::Triangle);
            if preserve_alpha_coverage {
                let target_cov = (base_coverage * 1.015_f32.powi(mip as i32)).min(0.95);
                scale_rgba_alpha_to_coverage(current_img.as_mut(), target_cov, alpha_cutoff);
            }
        }

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: mip,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &current_img,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Ok((texture, view))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stitched_terrain_lod_manifold_and_area() {
        let (indices, ranges) = build_stitched_terrain_lod_indices();
        let stride = 33i32;

        // Verify progressive triangle reduction across LOD0 -> LOD1 -> LOD2
        let tris_lod0 = ranges[0].1 / 3;
        let tris_lod1 = ranges[1].1 / 3;
        let tris_lod2 = ranges[2].1 / 3;
        assert_eq!(tris_lod0, 2048);
        assert!(tris_lod1 < 600 && tris_lod1 > 500);
        assert!(tris_lod2 < 350 && tris_lod2 > 250);

        // Verify every LOD partitions the 32m x 32m square (area = 1024.0 m^2) with positive CCW winding
        for (lod_idx, &(start, count)) in ranges.iter().enumerate() {
            let slice = &indices[start as usize..(start + count) as usize];
            let mut double_area = 0i32;
            for tri in slice.chunks_exact(3) {
                let ia = tri[0] as i32;
                let ib = tri[1] as i32;
                let ic = tri[2] as i32;
                let (ax, az) = (ia % stride, ia / stride);
                let (bx, bz) = (ib % stride, ib / stride);
                let (cx, cz) = (ic % stride, ic / stride);

                let cross = (bz - az) * (cx - ax) - (bx - ax) * (cz - az);
                assert!(
                    cross > 0,
                    "LOD {} contains degenerate or clockwise-inverted triangle: {:?}",
                    lod_idx,
                    [(ax, az), (bx, bz), (cx, cz)]
                );
                double_area += cross;
            }
            assert_eq!(
                double_area,
                2048,
                "LOD {} must cover exact 32x32m area (2 * 1024 = 2048), got {}",
                lod_idx,
                double_area
            );
        }
    }

    #[test]
    fn test_analytical_terrain_normal_matches_central_difference() {
        let coords = [
            (0.0, 0.0),
            (12.5, -7.3),
            (-48.2, 91.6),
            (128.0, 256.0),
            (-3.1415, -2.7182),
        ];
        let eps = 1e-3_f32;
        for (x, z) in coords {
            let n_analytical = terrain_normal(x, z);
            assert!(
                (n_analytical.length() - 1.0).abs() < 1e-5,
                "Normal must be unit length"
            );
            assert!(n_analytical.y > 0.85, "Terrain normal must point upward");

            let hl = terrain_height(x - eps, z);
            let hr = terrain_height(x + eps, z);
            let hd = terrain_height(x, z - eps);
            let hu = terrain_height(x, z + eps);
            let n_fd = Vec3::new(hl - hr, 2.0 * eps, hd - hu).normalize();

            let dot = n_analytical.dot(n_fd);
            assert!(
                dot > 0.99999,
                "Analytical normal {:?} diverged from finite-difference {:?} at ({}, {})",
                n_analytical,
                n_fd,
                x,
                z
            );
        }
    }

    #[test]
    fn test_castano_alpha_coverage_preservation() {
        // Construct a synthetic 64-pixel RGBA buffer where box-filtering eroded alpha values below cutoff=78
        let mut rgba = Vec::with_capacity(64 * 4);
        for i in 0..64 {
            let alpha = (i as u8) * 2; // 0, 2, 4, ..., 126
            let rgb = if alpha < 8 { [0, 0, 0] } else { [80, 140, 60] };
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
        }
        flood_transparent_rgb_with_average(&mut rgba, 78);
        assert_eq!(
            &rgba[0..4],
            &[80, 140, 60, 0],
            "Transparent black texels must inherit average opaque RGB while keeping alpha=0"
        );

        let initial_cov = compute_alpha_coverage(&rgba, 78);
        assert!(initial_cov < 0.45);

        // Preserve target coverage = 0.65
        scale_rgba_alpha_to_coverage(&mut rgba, 0.65, 78);
        let new_cov = compute_alpha_coverage(&rgba, 78);
        assert!(
            (new_cov - 0.65).abs() <= 0.05,
            "Expected coverage ~0.65 after Castano scaling, got {}",
            new_cov
        );

        // Edge case: empty slice must not panic
        let mut empty: [u8; 0] = [];
        assert_eq!(compute_alpha_coverage(&empty, 78), 1.0);
        scale_rgba_alpha_to_coverage(&mut empty, 0.5, 78);
        flood_transparent_rgb_with_average(&mut empty, 78);
    }
}

