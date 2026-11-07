use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct GrassVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct GrassInstanceRaw {
    pub model_0: [f32; 4],
    pub model_1: [f32; 4],
    pub model_2: [f32; 4],
    pub model_3: [f32; 4],
    pub color_tint: [f32; 4],
}

impl GrassInstanceRaw {
    pub fn new(model: Mat4, color_tint: [f32; 4]) -> Self {
        let cols = model.to_cols_array_2d();
        Self {
            model_0: cols[0],
            model_1: cols[1],
            model_2: cols[2],
            model_3: cols[3],
            color_tint,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GrassVariantRange {
    pub index_start: u32,
    pub index_count: u32,
    pub lod_ranges: [(u32, u32); 3],
}

/// Build a decimated index buffer for a grass clump variant by grouping connected triangles into
/// complete grass blades and keeping the largest/tallest blades distributed around the clump.
pub fn build_grass_lod_indices(
    vertices: &[GrassVertex],
    base_vertex: u16,
    global_indices: &[u16],
    keep_ratio: f32,
) -> Vec<u16> {
    let tri_count = global_indices.len() / 3;
    if tri_count <= 6 || keep_ratio >= 0.99 {
        return global_indices.to_vec();
    }

    let vert_count = vertices.len();
    let mut parent: Vec<usize> = (0..vert_count).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            let p = parent[x];
            parent[x] = parent[p];
            x = p;
        }
        x
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[rb] = ra;
        }
    }

    for t in 0..tri_count {
        let i0 = (global_indices[t * 3] - base_vertex) as usize;
        let i1 = (global_indices[t * 3 + 1] - base_vertex) as usize;
        let i2 = (global_indices[t * 3 + 2] - base_vertex) as usize;
        if i0 < vert_count && i1 < vert_count && i2 < vert_count {
            union(&mut parent, i0, i1);
            union(&mut parent, i1, i2);
        }
    }

    // Group triangles by connected blade component
    let mut comp_map: Vec<(usize, Vec<usize>, f32, f32)> = Vec::new(); // (root_v, tri_indices, score, angle)
    for t in 0..tri_count {
        let i0 = (global_indices[t * 3] - base_vertex) as usize;
        let i1 = (global_indices[t * 3 + 1] - base_vertex) as usize;
        let i2 = (global_indices[t * 3 + 2] - base_vertex) as usize;
        let root = if i0 < vert_count { find(&mut parent, i0) } else { t };

        let p0 = Vec3::from_slice(&vertices[i0.min(vert_count - 1)].position);
        let p1 = Vec3::from_slice(&vertices[i1.min(vert_count - 1)].position);
        let p2 = Vec3::from_slice(&vertices[i2.min(vert_count - 1)].position);
        let area = (p1 - p0).cross(p2 - p0).length();
        let max_y = p0.y.max(p1.y).max(p2.y).max(0.05);
        let tri_score = area * (0.4 + max_y);
        let center = (p0 + p1 + p2) * (1.0 / 3.0);
        let angle = center.z.atan2(center.x);

        if let Some(entry) = comp_map.iter_mut().find(|e| e.0 == root) {
            entry.1.push(t);
            entry.2 += tri_score;
        } else {
            comp_map.push((root, vec![t], tri_score, angle));
        }
    }

    let target_tris = ((tri_count as f32) * keep_ratio).round().max(6.0) as usize;

    // Sort components around azimuth and interleave selection so all 360° of the clump stay balanced
    if comp_map.len() >= 4 {
        comp_map.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        let mut kept_tris = Vec::with_capacity(target_tris);
        for (_, tris, _, _) in &comp_map {
            if kept_tris.len() + tris.len() <= target_tris || kept_tris.len() < 6 {
                kept_tris.extend_from_slice(tris);
            }
            if kept_tris.len() >= target_tris {
                break;
            }
        }
        kept_tris.sort_unstable();
        let mut out = Vec::with_capacity(kept_tris.len() * 3);
        for t in kept_tris {
            out.push(global_indices[t * 3]);
            out.push(global_indices[t * 3 + 1]);
            out.push(global_indices[t * 3 + 2]);
        }
        out
    } else {
        // Fallback: rank individual triangles by area * height
        let mut scored: Vec<(usize, f32)> = (0..tri_count)
            .map(|t| {
                let i0 = (global_indices[t * 3] - base_vertex) as usize;
                let i1 = (global_indices[t * 3 + 1] - base_vertex) as usize;
                let i2 = (global_indices[t * 3 + 2] - base_vertex) as usize;
                let p0 = Vec3::from_slice(&vertices[i0.min(vert_count - 1)].position);
                let p1 = Vec3::from_slice(&vertices[i1.min(vert_count - 1)].position);
                let p2 = Vec3::from_slice(&vertices[i2.min(vert_count - 1)].position);
                let area = (p1 - p0).cross(p2 - p0).length();
                (t, area * (0.4 + p0.y.max(p1.y).max(p2.y)))
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(target_tris.min(tri_count));
        scored.sort_by_key(|&(t, _)| t);
        let mut out = Vec::with_capacity(scored.len() * 3);
        for (t, _) in scored {
            out.push(global_indices[t * 3]);
            out.push(global_indices[t * 3 + 1]);
            out.push(global_indices[t * 3 + 2]);
        }
        out
    }
}

pub const GRASS_SHADER_WGSL: &str = r#"
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
var t_grass_albedo: texture_2d<f32>;
@group(1) @binding(1)
var s_grass_albedo: sampler;
@group(1) @binding(2)
var t_grass_normal: texture_2d<f32>;
@group(1) @binding(3)
var s_grass_normal: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct InstanceInput {
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(7) color_tint: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>, // rgb = inst.color_tint.rgb * root_ao, a = root_ao
    @location(4) shadow_pos: vec3<f32>,
    @location(5) height_factor: f32,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let model = mat4x4<f32>(
        inst.model_0,
        inst.model_1,
        inst.model_2,
        inst.model_3,
    );

    // Instance world position (translation column)
    let inst_world_pos = inst.model_3.xyz;
    let cam_dist = length(inst_world_pos - camera.camera_pos.xyz);

    // Distance Cull & Smooth Fade Out:
    // Smoothly scale clumps down to the ground between 26m and 34m,
    // and clip completely outside the viewing volume past 34m.
    let dist_fade = clamp((34.0 - cam_dist) * 0.125, 0.0, 1.0);
    if (dist_fade <= 0.0) {
        out.clip_position = vec4<f32>(0.0, 0.0, 2.0, 1.0);
        return out;
    }

    // Shrink clump seamlessly into ground as distance increases
    let scaled_pos = in.position * dist_fade;
    let world_pos = (model * vec4<f32>(scaled_pos, 1.0)).xyz;

    // ROOT-ANCHORED WIND ANIMATION
    let height_factor = clamp(in.position.y * 3.125, 0.0, 1.0);
    let wind_bend = height_factor * height_factor * dist_fade;

    let time = camera.settings.y;
    let foliage_tier = f32(camera.quality_extra.y);
    let wind_strength = 0.08 * (1.0 + foliage_tier * 0.35);

    let wave1 = sin(time * 3.2 + world_pos.x * 0.7 + world_pos.z * 0.5);
    let wave2 = sin(time * 5.8 + world_pos.x * 1.3 + world_pos.z * 1.1) * 0.35;
    let gust = sin(time * 1.4 + (world_pos.x + world_pos.z) * 0.25) * 0.5 + 0.5;

    let wind_disp = vec3<f32>(
        (wave1 + wave2) * wind_strength * gust * wind_bend,
        -abs(wave1 * 0.02 * wind_bend),
        (wave1 * 0.7 - wave2 * 0.4) * wind_strength * gust * wind_bend
    );

    let final_world_pos = world_pos + wind_disp;

    let base_normal = normalize((model * vec4<f32>(in.normal, 0.0)).xyz);
    let wind_tilt = vec3<f32>(-wind_disp.x * 2.2, 0.0, -wind_disp.z * 2.2);

    // Precompute vertical root-to-tip contact occlusion in vertex shader (saves per-pixel pow())
    let root_ao = mix(0.14, 1.0, height_factor * sqrt(height_factor));

    out.world_pos = final_world_pos;
    out.world_normal = normalize(base_normal + wind_tilt * height_factor);
    out.clip_position = camera.view_proj * vec4<f32>(final_world_pos, 1.0);
    out.uv = in.uv;
    out.color = vec4<f32>(inst.color_tint.rgb * root_ao, root_ao);
    out.height_factor = height_factor;

    if (camera.quality_tiers.x > 0u) {
        let shadow_clip = camera.light_view_proj * vec4<f32>(final_world_pos, 1.0);
        out.shadow_pos = vec3<f32>(
            shadow_clip.x * 0.5 + 0.5,
            -shadow_clip.y * 0.5 + 0.5,
            shadow_clip.z
        );
    } else {
        out.shadow_pos = vec3<f32>(-1.0, -1.0, -1.0);
    }

    return out;
}

fn ign(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
}

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

@fragment
fn fs_main(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_grass_albedo, s_grass_albedo, in.uv);

    // Direct hardware alpha cutout without pre-discard fwidth() quad synchronization stall
    if (tex_color.a < 0.30) {
        discard;
    }

    let to_cam = camera.camera_pos.xyz - in.world_pos;
    let dist_sq = dot(to_cam, to_cam);
    let inv_dist = inverseSqrt(max(dist_sq, 0.0001));
    let dist = dist_sq * inv_dist;
    let view_dir = to_cam * inv_dist;

    // 1. Distance-Gated 2K Grass Normal Map (< 22m)
    var normal_dir: vec3<f32>;
    if (dist < 22.0) {
        let raw_normal = textureSampleLevel(t_grass_normal, s_grass_normal, in.uv, 0.0).xyz * 2.0 - 1.0;
        let v_up = vec3<f32>(0.0, 1.0, 0.0);
        var t_blade = cross(in.world_normal, v_up);
        let t_len_sq = dot(t_blade, t_blade);
        t_blade = select(vec3<f32>(1.0, 0.0, 0.0), t_blade * inverseSqrt(max(t_len_sq, 0.0001)), t_len_sq > 0.0001);
        let b_blade = cross(in.world_normal, t_blade);
        let norm_fade = clamp((22.0 - dist) * 0.20, 0.0, 1.0);
        let n_scale = 1.6 * norm_fade;
        let perturbed_normal = normalize(
            t_blade * (raw_normal.x * n_scale) +
            b_blade * (raw_normal.y * n_scale) +
            in.world_normal * raw_normal.z
        );
        normal_dir = select(-perturbed_normal, perturbed_normal, is_front);
    } else {
        normal_dir = select(-in.world_normal, in.world_normal, is_front);
    }

    let light_dir = normalize(camera.light_dir.xyz);

    // 2. Vertical Root-to-Tip Contact Occlusion (pre-multiplied in vs_main)
    let root_ao = in.color.a;
    let albedo = tex_color.rgb * in.color.rgb;

    // 16-Tap Rotated Poisson PCF Soft Shadow Filtering on Grass
    var shadow: f32 = 1.0;
    let shadow_tier = camera.quality_tiers.x;
    if (shadow_tier > 0u && in.shadow_pos.x >= 0.0 && in.shadow_pos.x <= 1.0 && in.shadow_pos.y >= 0.0 && in.shadow_pos.y <= 1.0 && in.shadow_pos.z >= 0.0 && in.shadow_pos.z <= 1.0) {
        let bias = 0.0030;
        let shadow_depth = in.shadow_pos.z - bias;

        if (shadow_tier == 1u) {
            shadow = textureSampleCompareLevel(t_shadow, s_shadow, in.shadow_pos.xy, shadow_depth);
        } else {
            let texel_size = 1.0 / 2048.0;
            let ang = ign(in.clip_position.xy) * 6.2831853;
            let ca = cos(ang);
            let sa = sin(ang);
            let rot = mat2x2<f32>(vec2<f32>(ca, sa), vec2<f32>(-sa, ca));
            let spread = texel_size * select(1.5, 2.2, shadow_tier >= 3u);

            var shadow_sum: f32 = 0.0;
            for (var p = 0u; p < 16u; p = p + 1u) {
                let offset = rot * get_poisson_16(p) * spread;
                shadow_sum += textureSampleCompareLevel(t_shadow, s_shadow, in.shadow_pos.xy + offset, shadow_depth);
            }
            shadow = shadow_sum * (1.0 / 16.0);
        }
    }

    // Ambient Lighting
    let gi_tier = camera.quality_tiers.y;
    let ambient_factor = select(0.22, select(0.28, 0.34, gi_tier >= 2u), gi_tier >= 1u);
    let ambient = vec3<f32>(0.10, 0.16, 0.26) * (ambient_factor * root_ao);

    // Sun Diffuse + Shadowed Subsurface Scattering
    let ndotl = max(dot(normal_dir, light_dir), 0.0);
    let ndotv = max(dot(normal_dir, view_dir), 0.001);
    let diff = max(dot(normal_dir, light_dir) * 0.65 + 0.35, 0.0);
    let sun_color = vec3<f32>(0.85, 0.92, 1.0);

    // Chlorophyll Backlight Translucency
    let sun_back = max(dot(view_dir, -light_dir), 0.0);
    let sun_sss = (sun_back * sun_back * sun_back) * in.height_factor * 0.60;
    let sss_tint = vec3<f32>(0.32, 0.74, 0.16);

    // Waxy Cuticle Microfacet Cook-Torrance GGX Specular Wind Glints
    let blade_roughness: f32 = 0.36;
    let f0: f32 = 0.04;
    let half_moon = normalize(light_dir + view_dir);
    let ndoth = max(dot(normal_dir, half_moon), 0.0);
    let vdoth = max(dot(view_dir, half_moon), 0.0);
    let fm = f_schlick(vdoth, f0);
    let dm = d_ggx(ndoth, blade_roughness);
    let vm = v_smith_ggx(ndotv, ndotl, blade_roughness);
    let moon_spec = min(dm * vm * fm, 3.5) * ndotl * shadow * in.height_factor;

    let diffuse = (diff + sun_sss * sss_tint) * shadow * sun_color;
    var lit_color = (ambient + diffuse) * albedo + moon_spec * sun_color;

    // 3. Dual-Zone Tactical Flashlight with Fast Algebraic Cone Rejection (Zero sqrt outside cone)
    if (camera.flashlight_pos.w > 0.5) {
        let to_light = camera.flashlight_pos.xyz - in.world_pos;
        let proj = dot(-to_light, camera.flashlight_dir.xyz);
        if (proj > 0.0) {
            let l_dist_sq = dot(to_light, to_light);
            if (l_dist_sq < 3025.0 && proj * proj > 0.64 * l_dist_sq) {
                let inv_l_dist = inverseSqrt(max(l_dist_sq, 0.000001));
                let l_dist = l_dist_sq * inv_l_dist;
                let l_dir = to_light * inv_l_dist;
                let spot_dot = proj * inv_l_dist;

                let hotspot = smoothstep(0.925, 0.982, spot_dot);
                let flood = smoothstep(0.80, 0.935, spot_dot);
                let cone_angle = sqrt(max(2.0 - 2.0 * spot_dot, 0.0));
                let ring_ripple = 1.0 + 0.06 * sin(cone_angle * 48.0) * (1.0 - hotspot);
                let beam_profile = (hotspot * 1.45 + flood * 0.48) * ring_ripple;

                let atten = 1.0 / (1.0 + 0.055 * l_dist + 0.014 * l_dist_sq);
                let dist_cutoff = clamp((55.0 - l_dist) * (1.0 / 12.0), 0.0, 1.0);
                let flash_color = vec3<f32>(1.0, 0.97, 0.88);
                let flash_power = beam_profile * atten * dist_cutoff * camera.flashlight_dir.w * 4.2;

                let f_ndotl = max(dot(normal_dir, l_dir), 0.0);
                let flash_diff = max(dot(normal_dir, l_dir) * 0.65 + 0.35, 0.0);
                let flash_back = max(dot(view_dir, -l_dir), 0.0);
                let flash_sss = (flash_back * flash_back * sqrt(flash_back)) * in.height_factor * 0.65;

                let half_flash = normalize(l_dir + view_dir);
                let f_ndoth = max(dot(normal_dir, half_flash), 0.0);
                let f_vdoth = max(dot(view_dir, half_flash), 0.0);
                let ff = f_schlick(f_vdoth, f0);
                let fd = d_ggx(f_ndoth, blade_roughness);
                let fv = v_smith_ggx(ndotv, f_ndotl, blade_roughness);
                let blade_spec = min(fd * fv * ff, 4.0) * f_ndotl * in.height_factor;

                let total_flash = (flash_diff * albedo + flash_sss * albedo * sss_tint) * root_ao + blade_spec;
                lit_color += total_flash * flash_color * flash_power;
            }
        }
    }

    // Atmospheric Midnight Horizon Fog
    let view_dist = camera.settings.w;
    let fog_start = view_dist * 0.42;
    let fog_end = view_dist * 0.90;
    let fog_factor = clamp((dist - fog_start) / (fog_end - fog_start + 0.001), 0.0, 1.0);
    let smooth_fog = fog_factor * fog_factor * (3.0 - 2.0 * fog_factor);
    let horizon_fog_color = vec3<f32>(0.04, 0.07, 0.13);
    lit_color = mix(lit_color, horizon_fog_color, smooth_fog);

    return vec4<f32>(lit_color, 1.0);
}
"#;

pub struct FoliagePipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub variant_ranges: Vec<GrassVariantRange>,
    pub instance_buffers: Vec<wgpu::Buffer>,
    pub instance_capacity: usize,
    pub material_bind_group: wgpu::BindGroup,
}

impl FoliagePipeline {
    pub const MAX_INSTANCES_PER_VARIANT: usize = 32768;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
        variants_bin: &[u8],
        atlas_albedo_bytes: &[u8],
        atlas_normal_bytes: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // 1. Parse grass variants binary
        let mut cursor = 0;
        let num_variants = u32::from_le_bytes(variants_bin[cursor..cursor + 4].try_into()?);
        cursor += 4;

        let mut all_vertices: Vec<GrassVertex> = Vec::new();
        let mut all_indices: Vec<u16> = Vec::new();
        let mut variant_ranges: Vec<GrassVariantRange> = Vec::new();

        for _ in 0..num_variants {
            let vert_count =
                u32::from_le_bytes(variants_bin[cursor..cursor + 4].try_into()?) as usize;
            cursor += 4;
            let idx_count =
                u32::from_le_bytes(variants_bin[cursor..cursor + 4].try_into()?) as usize;
            cursor += 4;

            let base_vertex = all_vertices.len() as u16;
            let vert_start = all_vertices.len();
            let index_start = all_indices.len() as u32;

            for _ in 0..vert_count {
                let x = f32::from_le_bytes(variants_bin[cursor..cursor + 4].try_into()?);
                cursor += 4;
                let y = f32::from_le_bytes(variants_bin[cursor..cursor + 4].try_into()?);
                cursor += 4;
                let z = f32::from_le_bytes(variants_bin[cursor..cursor + 4].try_into()?);
                cursor += 4;
                let nx = f32::from_le_bytes(variants_bin[cursor..cursor + 4].try_into()?);
                cursor += 4;
                let ny = f32::from_le_bytes(variants_bin[cursor..cursor + 4].try_into()?);
                cursor += 4;
                let nz = f32::from_le_bytes(variants_bin[cursor..cursor + 4].try_into()?);
                cursor += 4;
                let u = f32::from_le_bytes(variants_bin[cursor..cursor + 4].try_into()?);
                cursor += 4;
                let v = f32::from_le_bytes(variants_bin[cursor..cursor + 4].try_into()?);
                cursor += 4;

                all_vertices.push(GrassVertex {
                    position: [x, y, z],
                    normal: [nx, ny, nz],
                    uv: [u, v],
                });
            }

            let mut lod0_indices = Vec::with_capacity(idx_count);
            for _ in 0..idx_count {
                let idx = u16::from_le_bytes(variants_bin[cursor..cursor + 2].try_into()?);
                cursor += 2;
                lod0_indices.push(base_vertex + idx);
            }
            all_indices.extend_from_slice(&lod0_indices);

            let variant_verts = &all_vertices[vert_start..vert_start + vert_count];
            let lod1_indices = build_grass_lod_indices(variant_verts, base_vertex, &lod0_indices, 0.50);
            let lod1_start = all_indices.len() as u32;
            let lod1_count = lod1_indices.len() as u32;
            all_indices.extend_from_slice(&lod1_indices);

            let lod2_indices = build_grass_lod_indices(variant_verts, base_vertex, &lod0_indices, 0.25);
            let lod2_start = all_indices.len() as u32;
            let lod2_count = lod2_indices.len() as u32;
            all_indices.extend_from_slice(&lod2_indices);

            variant_ranges.push(GrassVariantRange {
                index_start,
                index_count: idx_count as u32,
                lod_ranges: [
                    (index_start, idx_count as u32),
                    (lod1_start, lod1_count),
                    (lod2_start, lod2_count),
                ],
            });
        }

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Foliage Vertex Buffer"),
            contents: bytemuck::cast_slice(&all_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Foliage Index Buffer"),
            contents: bytemuck::cast_slice(&all_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // 2. Instance buffers for each variant
        let mut instance_buffers = Vec::new();
        for i in 0..num_variants {
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("Foliage Instance Buffer Variant {}", i)),
                size: (Self::MAX_INSTANCES_PER_VARIANT * std::mem::size_of::<GrassInstanceRaw>())
                    as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            instance_buffers.push(buf);
        }

        // 3. Load Grass Atlas Textures
        let (_, albedo_view) = crate::terrain::create_texture_from_bytes(
            device,
            queue,
            atlas_albedo_bytes,
            "Grass Atlas Albedo",
            true,
        )?;
        let (_, normal_view) = crate::terrain::create_texture_from_bytes(
            device,
            queue,
            atlas_normal_bytes,
            "Grass Atlas Normal",
            false,
        )?;

        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Grass Texture Linear Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 4,
            ..Default::default()
        });

        let material_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Grass Material Bind Group Layout"),
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
                ],
            });

        let material_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Grass Material Bind Group"),
            layout: &material_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&albedo_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&linear_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&normal_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&linear_sampler),
                },
            ],
        });

        // 4. Render Pipeline
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Grass Foliage Shader Module"),
            source: wgpu::ShaderSource::Wgsl(GRASS_SHADER_WGSL.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Foliage Pipeline Layout"),
            bind_group_layouts: &[camera_bind_group_layout, &material_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Foliage Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[
                    // Vertex layout
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<GrassVertex>() as wgpu::BufferAddress,
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
                        ],
                    },
                    // Instance layout
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<GrassInstanceRaw>()
                            as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            wgpu::VertexAttribute {
                                offset: 0,
                                shader_location: 3,
                                format: wgpu::VertexFormat::Float32x4, // model_0
                            },
                            wgpu::VertexAttribute {
                                offset: 16,
                                shader_location: 4,
                                format: wgpu::VertexFormat::Float32x4, // model_1
                            },
                            wgpu::VertexAttribute {
                                offset: 32,
                                shader_location: 5,
                                format: wgpu::VertexFormat::Float32x4, // model_2
                            },
                            wgpu::VertexAttribute {
                                offset: 48,
                                shader_location: 6,
                                format: wgpu::VertexFormat::Float32x4, // model_3
                            },
                            wgpu::VertexAttribute {
                                offset: 64,
                                shader_location: 7,
                                format: wgpu::VertexFormat::Float32x4, // color_tint
                            },
                        ],
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
                cull_mode: None, // Two-sided grass blades
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
            vertex_buffer,
            index_buffer,
            variant_ranges,
            instance_buffers,
            instance_capacity: Self::MAX_INSTANCES_PER_VARIANT,
            material_bind_group,
        })
    }
}

/// Deterministically scatter grass clumps in a 32m x 32m chunk using Jittered Grid distribution.
/// Mathematically guarantees 100% gapless carpet coverage with zero flat bare spots.
pub fn scatter_chunk_grass(
    chunk_x: i32,
    chunk_z: i32,
    clumps_per_chunk: usize,
    variant_count: usize,
    instances_by_variant: &mut [Vec<GrassInstanceRaw>],
) {
    if variant_count == 0 || clumps_per_chunk == 0 {
        return;
    }

    // Grid dimension (e.g. sqrt(1200) = 35 cells per axis -> 35x35 = 1225 cells)
    let grid_dim = (clumps_per_chunk as f32).sqrt().ceil().max(2.0) as usize;
    let cell_size = 32.0 / (grid_dim as f32);

    let mut seed = ((chunk_x as u64) << 32) ^ (chunk_z as u64 & 0xFFFF_FFFF);
    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);

    for gz in 0..grid_dim {
        for gx in 0..grid_dim {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let jx = (((seed >> 32) & 0xFFFF) as f32 / 65535.0 - 0.5) * 0.65;

            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let jz = (((seed >> 32) & 0xFFFF) as f32 / 65535.0 - 0.5) * 0.65;

            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let ryaw = (((seed >> 32) & 0xFFFF) as f32 / 65535.0) * std::f32::consts::TAU;

            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            // Oversized broad footprint (1.45m to 2.25m) guarantees full overlap across neighboring cells
            let rscale = 1.45 + (((seed >> 32) & 0xFFFF) as f32 / 65535.0) * 0.80;

            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let variant_idx = ((seed >> 32) as usize) % variant_count;

            let local_x = ((gx as f32) + 0.5 + jx) * cell_size;
            let local_z = ((gz as f32) + 0.5 + jz) * cell_size;

            let wx = (chunk_x as f32) * 32.0 + local_x;
            let wz = (chunk_z as f32) * 32.0 + local_z;
            let norm = crate::terrain::terrain_normal(wx, wz);
            let slope_sink = (1.0 - norm.y).max(0.0) * 0.28;
            // Sink into terrain with slope compensation so downhill blades never float in mid-air
            let wy = crate::terrain::terrain_height(wx, wz) - 0.05 - slope_sink;

            let rot = Quat::from_rotation_arc(Vec3::Y, norm) * Quat::from_rotation_y(ryaw);
            let model = Mat4::from_scale_rotation_translation(
                Vec3::new(rscale, rscale * 1.05, rscale),
                rot,
                Vec3::new(wx, wy, wz),
            );

            // Continuous world-space chlorophyll tint (zero 32m chunk boundary checkerboard seams)
            let macro_r = (wx * 0.065 + wz * 0.045).sin() * 0.5 + 0.5;
            let macro_g = (wx * 0.052 - wz * 0.071).cos() * 0.5 + 0.5;
            let color_tint = [
                0.80 + macro_r * 0.10,
                0.86 + macro_g * 0.10,
                0.68 + (macro_r + macro_g) * 0.07,
                1.0,
            ];

            if variant_idx < instances_by_variant.len()
                && instances_by_variant[variant_idx].len()
                    < FoliagePipeline::MAX_INSTANCES_PER_VARIANT
            {
                instances_by_variant[variant_idx].push(GrassInstanceRaw::new(model, color_tint));
            }
        }
    }
}

/// 8m x 8m sub-patch within a 32m x 32m grass chunk for fine-grained camera frustum & distance culling.
pub struct GrassSubPatch {
    pub min_x: f32,
    pub min_z: f32,
    pub instances_by_variant: Vec<Vec<GrassInstanceRaw>>,
}

/// Cached 32m x 32m grass chunk subdivided into 4x4 (16) 8m x 8m sub-patches.
pub struct GrassChunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub min_x: f32,
    pub min_z: f32,
    pub sub_patches: Vec<GrassSubPatch>,
}

impl GrassChunk {
    pub fn generate(
        chunk_x: i32,
        chunk_z: i32,
        clumps_per_chunk: usize,
        variant_count: usize,
    ) -> Self {
        let min_x = (chunk_x as f32) * 32.0;
        let min_z = (chunk_z as f32) * 32.0;

        let mut flat_by_variant: Vec<Vec<GrassInstanceRaw>> = (0..variant_count)
            .map(|_| Vec::with_capacity((clumps_per_chunk / variant_count.max(1)) + 32))
            .collect();
        scatter_chunk_grass(
            chunk_x,
            chunk_z,
            clumps_per_chunk,
            variant_count,
            &mut flat_by_variant,
        );

        let mut sub_patches = Vec::with_capacity(16);
        for sz in 0..4 {
            for sx in 0..4 {
                sub_patches.push(GrassSubPatch {
                    min_x: min_x + (sx as f32) * 8.0,
                    min_z: min_z + (sz as f32) * 8.0,
                    instances_by_variant: (0..variant_count)
                        .map(|_| Vec::with_capacity((clumps_per_chunk / (16 * variant_count.max(1))) + 8))
                        .collect(),
                });
            }
        }

        for (variant_idx, instances) in flat_by_variant.into_iter().enumerate() {
            for inst in instances {
                let lx = (inst.model_3[0] - min_x).clamp(0.0, 31.999);
                let lz = (inst.model_3[2] - min_z).clamp(0.0, 31.999);
                let sx = (lx * 0.125) as usize;
                let sz = (lz * 0.125) as usize;
                let sub_idx = (sz.min(3) * 4) + sx.min(3);
                sub_patches[sub_idx].instances_by_variant[variant_idx].push(inst);
            }
        }

        Self {
            chunk_x,
            chunk_z,
            min_x,
            min_z,
            sub_patches,
        }
    }
}

/// Compute a 144-bit visibility mask (`[u64; 3]`) for up to 9 grass chunks (9 * 16 = 144 sub-patches).
/// Hierarchically tests 32m chunks first, then 8m sub-patches against `frustum` and the 34m fade-out radius.
pub fn compute_visible_grass_mask(
    chunks: &[GrassChunk],
    frustum: &crate::culling::Frustum,
    cam_pos: Vec3,
) -> [u64; 3] {
    let mut mask = [0u64; 3];
    let max_dist_sq = 35.8 * 35.8;

    for (chunk_idx, chunk) in chunks.iter().enumerate().take(9) {
        // Level 1: Coarse 32m x 32m chunk distance & frustum test
        let c_closest_x = cam_pos.x.clamp(chunk.min_x, chunk.min_x + 32.0);
        let c_closest_z = cam_pos.z.clamp(chunk.min_z, chunk.min_z + 32.0);
        let cdx = cam_pos.x - c_closest_x;
        let cdz = cam_pos.z - c_closest_z;
        if cdx * cdx + cdz * cdz > max_dist_sq {
            continue;
        }
        if !frustum.intersects_chunk(chunk.min_x - 2.0, chunk.min_z - 2.0, 36.0, -2.0, 6.0) {
            continue;
        }

        // Level 2: Fine 8m x 8m sub-patch distance & frustum test
        for (sub_idx, sub) in chunk.sub_patches.iter().enumerate() {
            let s_closest_x = cam_pos.x.clamp(sub.min_x, sub.min_x + 8.0);
            let s_closest_z = cam_pos.z.clamp(sub.min_z, sub.min_z + 8.0);
            let sdx = cam_pos.x - s_closest_x;
            let sdz = cam_pos.z - s_closest_z;
            if sdx * sdx + sdz * sdz > max_dist_sq {
                continue;
            }
            if frustum.intersects_chunk(sub.min_x - 1.8, sub.min_z - 1.8, 11.6, -2.0, 5.5) {
                let bit_index = chunk_idx * 16 + sub_idx;
                mask[bit_index / 64] |= 1u64 << (bit_index % 64);
            }
        }
    }

    mask
}

/// Gather visible grass instances matching `mask` into `out_instances_by_variant`,
/// ordering visible 8m x 8m sub-patches front-to-back relative to `cam_pos` on the stack,
/// culling individual corner clumps beyond 33.8m, and partitioning instances into contiguous
/// [LOD0 (<=11.5m), LOD1 (11.5..20.5m), LOD2 (20.5..33.8m)] bands with zero heap allocations.
pub fn gather_visible_grass_instances(
    chunks: &[GrassChunk],
    mask: &[u64; 3],
    cam_pos: Vec3,
    out_instances_by_variant: &mut [Vec<GrassInstanceRaw>],
) {
    for list in out_instances_by_variant.iter_mut() {
        list.clear();
    }

    let mut visible_subs = [(0u8, 0u8, 0.0f32); 144];
    let mut visible_count = 0usize;

    for (chunk_idx, chunk) in chunks.iter().enumerate().take(9) {
        for (sub_idx, sub) in chunk.sub_patches.iter().enumerate().take(16) {
            let bit_index = chunk_idx * 16 + sub_idx;
            if (mask[bit_index / 64] & (1u64 << (bit_index % 64))) != 0 {
                let dx = (sub.min_x + 4.0) - cam_pos.x;
                let dz = (sub.min_z + 4.0) - cam_pos.z;
                visible_subs[visible_count] = (chunk_idx as u8, sub_idx as u8, dx * dx + dz * dz);
                visible_count += 1;
            }
        }
    }

    visible_subs[..visible_count].sort_unstable_by(|a, b| {
        a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal)
    });

    const D0_SQ: f32 = 11.5 * 11.5;
    const D1_SQ: f32 = 20.5 * 20.5;
    const D2_SQ: f32 = 33.8 * 33.8;
    let bands = [(0.0f32, D0_SQ), (D0_SQ, D1_SQ), (D1_SQ, D2_SQ)];

    for &(min_d_sq, max_d_sq) in &bands {
        for &(c_idx, s_idx, _) in &visible_subs[..visible_count] {
            let sub = &chunks[c_idx as usize].sub_patches[s_idx as usize];
            for (variant_idx, src) in sub.instances_by_variant.iter().enumerate() {
                if variant_idx < out_instances_by_variant.len() && !src.is_empty() {
                    let dst = &mut out_instances_by_variant[variant_idx];
                    for inst in src {
                        if dst.len() >= FoliagePipeline::MAX_INSTANCES_PER_VARIANT {
                            break;
                        }
                        let idx = inst.model_3[0] - cam_pos.x;
                        let idz = inst.model_3[2] - cam_pos.z;
                        let dist_sq = idx * idx + idz * idz;
                        if dist_sq > min_d_sq && dist_sq <= max_d_sq {
                            dst.push(*inst);
                        } else if min_d_sq == 0.0 && dist_sq == 0.0 {
                            dst.push(*inst);
                        }
                    }
                }
            }
        }
    }
}

/// Compute cumulative instance end indices `[lod0_end, lod1_end, lod2_end]` for each grass variant.
pub fn compute_grass_lod_ends(
    instances_by_variant: &[Vec<GrassInstanceRaw>],
    cam_pos: Vec3,
    out_lod_ends: &mut [[u32; 3]],
) {
    const D0_SQ: f32 = 11.5 * 11.5;
    const D1_SQ: f32 = 20.5 * 20.5;

    for (variant_idx, instances) in instances_by_variant.iter().enumerate() {
        if variant_idx >= out_lod_ends.len() {
            break;
        }
        let total = instances.len().min(FoliagePipeline::MAX_INSTANCES_PER_VARIANT);
        let mut end0 = 0u32;
        let mut end1 = 0u32;
        for (i, inst) in instances.iter().take(total).enumerate() {
            let dx = inst.model_3[0] - cam_pos.x;
            let dz = inst.model_3[2] - cam_pos.z;
            let dist_sq = dx * dx + dz * dz;
            if dist_sq <= D0_SQ {
                end0 = (i + 1) as u32;
                end1 = (i + 1) as u32;
            } else if dist_sq <= D1_SQ {
                end1 = (i + 1) as u32;
            }
        }
        out_lod_ends[variant_idx] = [end0, end1, total as u32];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera3d::Camera3D;

    #[test]
    fn test_grass_chunk_subpatch_culling_and_empty_inputs() {
        // 1. Empty inputs should not panic
        let mut empty_out: Vec<Vec<GrassInstanceRaw>> = vec![Vec::new(), Vec::new()];
        scatter_chunk_grass(0, 0, 0, 2, &mut empty_out);
        assert!(empty_out.iter().all(|v| v.is_empty()));

        let cam_pos = Vec3::new(16.0, 2.5, 16.0);
        let cam = Camera3D::new(cam_pos, Vec3::new(16.0, 2.5, 0.0), 16.0 / 9.0);
        let frustum = cam.frustum();
        let empty_mask = compute_visible_grass_mask(&[], &frustum, cam_pos);
        assert_eq!(empty_mask, [0, 0, 0]);

        // 2. Generate 3x3 grass chunks and verify sub-patch culling removes back/far patches
        let mut chunks = Vec::new();
        for cz in -1..=1 {
            for cx in -1..=1 {
                chunks.push(GrassChunk::generate(cx, cz, 400, 4));
            }
        }

        let total_unfiltered: usize = chunks
            .iter()
            .flat_map(|c| c.sub_patches.iter())
            .flat_map(|s| s.instances_by_variant.iter())
            .map(|v| v.len())
            .sum();
        assert!(total_unfiltered >= 3600);

        let mask = compute_visible_grass_mask(&chunks, &frustum, cam_pos);
        assert_ne!(mask, [0, 0, 0]);

        let mut culled_out: Vec<Vec<GrassInstanceRaw>> = (0..4).map(|_| Vec::new()).collect();
        gather_visible_grass_instances(&chunks, &mask, cam_pos, &mut culled_out);
        let total_culled: usize = culled_out.iter().map(|v| v.len()).sum();

        // Looking North (-Z) from (16, 2.5, 16) with 60 deg FOV should cull the majority of the 96m x 96m grid
        assert!(total_culled > 0);
        assert!(total_culled < total_unfiltered / 2);

        // Verify front-to-back sub-patch ordering and LOD band boundaries
        let v0 = &culled_out[0];
        let first_dist_sq = (v0[0].model_3[0] - cam_pos.x).powi(2) + (v0[0].model_3[2] - cam_pos.z).powi(2);
        let last = v0.last().unwrap();
        let last_dist_sq = (last.model_3[0] - cam_pos.x).powi(2) + (last.model_3[2] - cam_pos.z).powi(2);
        assert!(first_dist_sq < last_dist_sq, "Visible grass sub-patches must be gathered front-to-back");
        assert!(last_dist_sq <= 33.8 * 33.8 + 1e-3, "Clumps beyond 33.8m must be culled on CPU");

        let mut lod_ends = [[0u32; 3]; 4];
        compute_grass_lod_ends(&culled_out, cam_pos, &mut lod_ends);
        for (v_idx, ends) in lod_ends.iter().enumerate() {
            assert!(ends[0] <= ends[1] && ends[1] <= ends[2]);
            assert_eq!(ends[2] as usize, culled_out[v_idx].len());
        }

        // Verify slope-normal alignment: local Y column (model_1.xyz normalized) must match terrain_normal(wx, wz)
        let inst = &v0[0];
        let up_col = Vec3::from_slice(&inst.model_1[0..3]).normalize();
        let expected_norm = crate::terrain::terrain_normal(inst.model_3[0], inst.model_3[2]);
        assert!(
            up_col.dot(expected_norm) > 0.9999,
            "Grass clump Y-axis {:?} must align with analytical terrain normal {:?}",
            up_col,
            expected_norm
        );
    }

    #[test]
    fn test_grass_blade_lod_decimation() {
        // Build 12 disconnected 2-triangle grass blades around a clump
        let mut verts = Vec::new();
        let mut indices = Vec::new();
        for b in 0..12u16 {
            let angle = (b as f32) * std::f32::consts::TAU / 12.0;
            let bx = angle.cos() * 0.4;
            let bz = angle.sin() * 0.4;
            let h = 0.20 + (b as f32) * 0.02;
            let base = verts.len() as u16;
            verts.push(GrassVertex { position: [bx, 0.0, bz], normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0] });
            verts.push(GrassVertex { position: [bx + 0.08, 0.0, bz], normal: [0.0, 1.0, 0.0], uv: [1.0, 0.0] });
            verts.push(GrassVertex { position: [bx + 0.08, h, bz], normal: [0.0, 1.0, 0.0], uv: [1.0, 1.0] });
            verts.push(GrassVertex { position: [bx, h, bz], normal: [0.0, 1.0, 0.0], uv: [0.0, 1.0] });
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }

        let lod1 = build_grass_lod_indices(&verts, 0, &indices, 0.50);
        let lod2 = build_grass_lod_indices(&verts, 0, &indices, 0.25);
        assert_eq!(lod1.len() % 6, 0, "Grass LOD1 must keep complete connected 2-triangle blades");
        assert_eq!(lod2.len() % 6, 0, "Grass LOD2 must keep complete connected 2-triangle blades");
        assert!(lod1.len() < indices.len() && lod2.len() < lod1.len());
    }
}



