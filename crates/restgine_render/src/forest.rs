use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec2, Vec3};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ForestVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ForestInstanceRaw {
    pub model_0: [f32; 4],
    pub model_1: [f32; 4],
    pub model_2: [f32; 4],
    pub model_3: [f32; 4],
    pub color_tint: [f32; 4],
}

impl ForestInstanceRaw {
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
pub struct MeshRange {
    pub vertex_offset: u32,
    pub vertex_count: u32,
    pub index_offset: u32,
    pub index_count: u32,
}

pub struct ForestVariant {
    pub name: String,
    pub kind: u32, // 0 = Oak Tree, 1 = Lilac Bush
    pub collider_radius: f32,
    pub bound_y_offset: f32,
    pub bound_radius: f32,
    pub bark_y_offset: f32,
    pub bark_radius: f32,
    pub bark_range: MeshRange,
    pub leaf_range: MeshRange,
    pub bark_lods: [MeshRange; 3],
    pub leaf_lods: [MeshRange; 3],
}

/// Decimates bark trunk/branch triangles to `target_tris` by scoring triangles by surface area
/// weighted toward the lower/middle main trunk (`area / (1 + dy * 0.35)`), shedding sub-pixel upper twigs first
/// while preserving original index buffer ordering for post-transform vertex cache locality.
pub fn build_bark_lod_indices(
    vertices: &[ForestVertex],
    base_indices: &[u16],
    target_tris: usize,
) -> Vec<u16> {
    let num_tris = base_indices.len() / 3;
    if num_tris <= target_tris || target_tris == 0 {
        return base_indices.to_vec();
    }

    let mut min_y = f32::MAX;
    for v in vertices {
        min_y = min_y.min(v.position[1]);
    }
    if !min_y.is_finite() {
        min_y = 0.0;
    }

    let mut scored: Vec<(usize, f32)> = Vec::with_capacity(num_tris);
    for t in 0..num_tris {
        let i0 = base_indices[t * 3] as usize;
        let i1 = base_indices[t * 3 + 1] as usize;
        let i2 = base_indices[t * 3 + 2] as usize;
        if i0 >= vertices.len() || i1 >= vertices.len() || i2 >= vertices.len() {
            continue;
        }
        let p0 = Vec3::from_Array(vertices[i0].position);
        let p1 = Vec3::from_Array(vertices[i1].position);
        let p2 = Vec3::from_Array(vertices[i2].position);
        let area_sq = (p1 - p0).cross(p2 - p0).length_squared();
        let cy = (p0.y + p1.y + p2.y) * (1.0 / 3.0);
        let height_penalty = 1.0 + (cy - min_y).max(0.0) * 0.35;
        scored.push((t, area_sq / height_penalty));
    }

    scored.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(target_tris.min(scored.len()));
    scored.sort_unstable_by_key(|&(t, _)| t);

    let mut out = Vec::with_capacity(scored.len() * 3);
    for (t, _) in scored {
        out.push(base_indices[t * 3]);
        out.push(base_indices[t * 3 + 1]);
        out.push(base_indices[t * 3 + 2]);
    }
    out
}

/// Decimates leaf canopy cards to `target_tris` at intact 2-triangle quad-card granularity (`6` indices per card),
/// stratified across 16 spatial crown sectors (8 azimuth octants x 2 elevation bands) and scored by
/// `card_area * outer_shell_radius` so the 360-degree crown silhouette is preserved without gaps.
pub fn build_leaf_lod_indices(
    vertices: &[ForestVertex],
    base_indices: &[u16],
    target_tris: usize,
) -> Vec<u16> {
    let num_tris = base_indices.len() / 3;
    if num_tris <= target_tris || target_tris == 0 {
        return base_indices.to_vec();
    }

    let num_quads = base_indices.len() / 6;
    let target_quads = (target_tris / 2).max(1).min(num_quads);
    if num_quads == 0 || target_quads >= num_quads {
        return base_indices.to_vec();
    }

    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    let mut max_r2 = 0.01f32;
    for v in vertices {
        min_y = min_y.min(v.position[1]);
        max_y = max_y.max(v.position[1]);
        let r2 = v.position[0] * v.position[0] + v.position[2] * v.position[2];
        max_r2 = max_r2.max(r2);
    }
    let mid_y = 0.5 * (min_y + max_y);

    let mut sectors: [Vec<(usize, f32)>; 16] = Default::default();
    for q in 0..num_quads {
        let base = q * 6;
        let i0 = base_indices[base] as usize;
        let i1 = base_indices[base + 1] as usize;
        let i2 = base_indices[base + 2] as usize;
        let i3 = base_indices[base + 3] as usize;
        let i4 = base_indices[base + 4] as usize;
        let i5 = base_indices[base + 5] as usize;
        if i0 >= vertices.len()
            || i1 >= vertices.len()
            || i2 >= vertices.len()
            || i3 >= vertices.len()
            || i4 >= vertices.len()
            || i5 >= vertices.len()
        {
            continue;
        }
        let p0 = Vec3::from_Array(vertices[i0].position);
        let p1 = Vec3::from_Array(vertices[i1].position);
        let p2 = Vec3::from_Array(vertices[i2].position);
        let p3 = Vec3::from_Array(vertices[i3].position);
        let p4 = Vec3::from_Array(vertices[i4].position);
        let p5 = Vec3::from_Array(vertices[i5].position);

        let a1 = (p1 - p0).cross(p2 - p0).length();
        let a2 = (p4 - p3).cross(p5 - p3).length();
        let area = a1 + a2;

        let center = (p0 + p1 + p2 + p5) * 0.25;
        let r2 = center.x * center.x + center.z * center.z;
        let shell_weight = 0.45 + 0.55 * (r2 / max_r2).clamp(0.0, 1.0);
        let score = area * shell_weight;

        let angle = center.z.atan2(center.x) + std::f32::consts::PI; // [0..2*PI]
        let octant = ((angle / std::f32::consts::TAU * 8.0).floor() as usize).min(7);
        let elev = if center.y >= mid_y { 1usize } else { 0usize };
        let sector_idx = elev * 8 + octant;
        sectors[sector_idx].push((q, score));
    }

    let mut selected_quads: Vec<usize> = Vec::with_capacity(target_quads + 16);
    for sector in &mut sectors {
        if sector.is_empty() {
            continue;
        }
        sector.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let quota = ((sector.len() * target_quads + num_quads / 2) / num_quads)
            .max(1)
            .min(sector.len());
        for &(q, _) in &sector[..quota] {
            selected_quads.push(q);
        }
    }

    selected_quads.sort_unstable();
    let mut out = Vec::with_capacity(selected_quads.len() * 6);
    for q in selected_quads {
        let base = q * 6;
        out.extend_from_slice(&base_indices[base..base + 6]);
    }
    out
}

#[allow(non_snake_case)]
trait Vec3FromArray {
    fn from_Array(a: [f32; 3]) -> Vec3;
}
#[allow(non_snake_case)]
impl Vec3FromArray for Vec3 {
    #[inline]
    fn from_Array(a: [f32; 3]) -> Vec3 {
        Vec3::new(a[0], a[1], a[2])
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TreeCollider {
    pub center: Vec2,
    pub radius: f32,
}

pub const FOREST_SHADER_WGSL: &str = r#"
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
var t_bark_albedo: texture_2d<f32>;
@group(1) @binding(1)
var s_bark: sampler;
@group(1) @binding(2)
var t_bark_normal: texture_2d<f32>;
@group(1) @binding(3)
var t_leaf_albedo: texture_2d<f32>;
@group(1) @binding(4)
var s_leaf: sampler;
@group(1) @binding(5)
var t_leaf_normal: texture_2d<f32>;

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
    @location(3) color: vec4<f32>,
    @location(4) height: f32,
    @location(5) shadow_pos: vec3<f32>,
};

@vertex
fn vs_shadow(in: VertexInput, inst: InstanceInput) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(inst.model_0, inst.model_1, inst.model_2, inst.model_3);
    let world_pos = (model * vec4<f32>(in.position, 1.0)).xyz;
    let height_factor = clamp(in.position.y / 16.0, 0.0, 1.0);
    let time = camera.settings.y;
    let sway = sin(time * 1.8 + world_pos.x * 0.15 + world_pos.z * 0.12) * (height_factor * height_factor * 0.08);
    let final_world_pos = world_pos + vec3<f32>(sway, 0.0, sway * 0.6);
    return camera.light_view_proj * vec4<f32>(final_world_pos, 1.0);
}

@vertex
fn vs_bark(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let model = mat4x4<f32>(inst.model_0, inst.model_1, inst.model_2, inst.model_3);
    let world_pos = (model * vec4<f32>(in.position, 1.0)).xyz;

    // Gentle trunk sway for very tall upper branches
    let time = camera.settings.y;
    let height_factor = clamp(in.position.y / 16.0, 0.0, 1.0);
    let sway = sin(time * 1.8 + world_pos.x * 0.15 + world_pos.z * 0.12) * (height_factor * height_factor * 0.08);
    let final_world_pos = world_pos + vec3<f32>(sway, 0.0, sway * 0.6);

    out.world_pos = final_world_pos;
    out.world_normal = normalize((model * vec4<f32>(in.normal, 0.0)).xyz);
    out.clip_position = camera.view_proj * vec4<f32>(final_world_pos, 1.0);
    out.uv = in.uv;
    out.color = inst.color_tint;
    out.height = in.position.y;

    // Transform to shadow map space [-1..1] -> [0..1]
    let shadow_clip = camera.light_view_proj * vec4<f32>(final_world_pos, 1.0);
    out.shadow_pos = vec3<f32>(
        shadow_clip.x * 0.5 + 0.5,
        -shadow_clip.y * 0.5 + 0.5,
        shadow_clip.z
    );

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
fn fs_bark(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_bark_albedo, s_bark, in.uv);
    let to_cam = camera.camera_pos.xyz - in.world_pos;
    let dist = length(to_cam);
    let view_dir = to_cam / max(dist, 0.0001);

    var normal = in.world_normal;
    if (dist < 42.0) {
        let norm_mip = clamp(log2(max(dist * 0.2, 1.0)), 0.0, 4.0);
        let raw_normal = textureSampleLevel(t_bark_normal, s_bark, in.uv, norm_mip).xyz * 2.0 - 1.0;
        let v_up = vec3<f32>(0.0, 1.0, 0.0);
        var t_vec = cross(in.world_normal, v_up);
        if (dot(t_vec, t_vec) < 0.0001) { t_vec = vec3<f32>(1.0, 0.0, 0.0); } else { t_vec = normalize(t_vec); }
        let b_vec = cross(in.world_normal, t_vec);
        let tbn = mat3x3<f32>(t_vec, b_vec, in.world_normal);
        let detail_fade = clamp((42.0 - dist) / 10.0, 0.0, 1.0);
        normal = normalize(tbn * (raw_normal * vec3<f32>(1.8 * detail_fade, 1.8 * detail_fade, 1.0)));
    }

    // Vertical Ground-Moisture & Damp Green Lichen/Moss Gradient on lower 3.5m of tree trunks
    let base_moisture = clamp((3.5 - in.height) / 3.5, 0.0, 1.0);
    let lichen_mask = base_moisture * base_moisture * clamp(normal.y * 0.7 + 0.45 - normal.z * 0.25, 0.0, 1.0);
    let base_albedo = tex_color.rgb * in.color.rgb;
    let damp_bark = base_albedo * mix(1.0, 0.50, base_moisture * 0.85);
    let lichen_color = vec3<f32>(0.13, 0.26, 0.11) * in.color.rgb;
    let albedo = mix(damp_bark, lichen_color, lichen_mask * 0.60);
    let bark_roughness = mix(0.68, 0.28, base_moisture * 0.88);
    let f0: f32 = 0.04;

    let light_dir = normalize(camera.light_dir.xyz);

    // 16-Tap Rotated Poisson PCF Soft Shadow Filtering
    var shadow: f32 = 1.0;
    let shadow_tier = camera.quality_tiers.x;
    if (dist < 72.0 && shadow_tier > 0u && in.shadow_pos.x >= 0.0 && in.shadow_pos.x <= 1.0 && in.shadow_pos.y >= 0.0 && in.shadow_pos.y <= 1.0 && in.shadow_pos.z >= 0.0 && in.shadow_pos.z <= 1.0) {
        let bias = 0.0028;
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

    // Root-to-canopy ambient occlusion & Cook-Torrance GGX Moonlight
    let trunk_ao = mix(0.55, 1.0, clamp(in.height / 1.8, 0.0, 1.0));
    let ambient = vec3<f32>(0.13, 0.17, 0.25) * (0.24 * trunk_ao);

    let ndotl = max(dot(normal, light_dir), 0.0);
    let ndotv = max(dot(normal, view_dir), 0.001);
    let half_dir = normalize(light_dir + view_dir);
    let ndoth = max(dot(normal, half_dir), 0.0);
    let vdoth = max(dot(view_dir, half_dir), 0.0);

    let sun_color = vec3<f32>(0.85, 0.92, 1.0);
    let fm = f_schlick(vdoth, f0);
    let dm = d_ggx(ndoth, bark_roughness);
    let vm = v_smith_ggx(ndotv, ndotl, bark_roughness);
    let bark_spec = min(dm * vm * fm, 4.0) * ndotl * shadow;

    var lit_color = ambient * albedo + ((1.0 - fm) * ndotl * shadow * albedo + bark_spec) * sun_color;

    // Dual-Zone Tactical Flashlight with Fast Algebraic Cone Rejection (Zero-Sqrt Outside Cone)
    if (camera.flashlight_pos.w > 0.5) {
        let to_light = camera.flashlight_pos.xyz - in.world_pos;
        let proj = -dot(to_light, camera.flashlight_dir.xyz);
        let l_dist_sq = dot(to_light, to_light);
        if (proj > 0.0 && l_dist_sq < 2025.0 && proj * proj > 0.7744 * l_dist_sq) {
            let inv_dist = inverseSqrt(max(l_dist_sq, 0.000001));
            let l_dist = l_dist_sq * inv_dist;
            let l_dir = to_light * inv_dist;
            let spot_dot = proj * inv_dist;

            let hotspot = smoothstep(0.94, 0.988, spot_dot);
            let flood = smoothstep(0.82, 0.94, spot_dot);
            let beam_profile = hotspot * 0.90 + flood * 0.35;

            let atten = 1.0 / (1.0 + 0.065 * l_dist + 0.018 * l_dist_sq);
            let dist_cutoff = clamp((54.0 - l_dist) / 10.0, 0.0, 1.0);
            let f_ndotl = max(dot(normal, l_dir), 0.0);
            let half_flash = normalize(l_dir + view_dir);
            let f_ndoth = max(dot(normal, half_flash), 0.0);
            let f_vdoth = max(dot(view_dir, half_flash), 0.0);

            let ff = f_schlick(f_vdoth, f0);
            let fd = d_ggx(f_ndoth, bark_roughness);
            let fv = v_smith_ggx(ndotv, f_ndotl, bark_roughness);
            let flash_spec = min(fd * fv * ff, 0.75) * f_ndotl;

            let flash_color = vec3<f32>(1.0, 0.96, 0.88);
            let flash_power = beam_profile * atten * dist_cutoff * camera.flashlight_dir.w * 1.10;

            lit_color += ((1.0 - ff) * f_ndotl * albedo + flash_spec) * flash_color * flash_power;
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

@vertex
fn vs_leaf(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let model = mat4x4<f32>(inst.model_0, inst.model_1, inst.model_2, inst.model_3);
    let world_pos = (model * vec4<f32>(in.position, 1.0)).xyz;

    // Harmonic wind rustle on leafy branch clusters
    let time = camera.settings.y;
    let height_factor = clamp(in.position.y / 14.0, 0.2, 1.0);
    let wave1 = sin(time * 3.2 + world_pos.x * 0.6 + world_pos.z * 0.5);
    let wave2 = sin(time * 5.4 + world_pos.x * 1.1 + world_pos.z * 1.3) * 0.35;
    let sway = (wave1 + wave2) * 0.12 * height_factor;
    let final_world_pos = world_pos + vec3<f32>(sway, -abs(wave1 * 0.02 * height_factor), sway * 0.7);

    out.world_pos = final_world_pos;
    out.world_normal = normalize((model * vec4<f32>(in.normal, 0.0)).xyz);
    out.clip_position = camera.view_proj * vec4<f32>(final_world_pos, 1.0);
    out.uv = in.uv;
    out.color = inst.color_tint;
    out.height = in.position.y;

    // Transform to shadow map space [-1..1] -> [0..1]
    let shadow_clip = camera.light_view_proj * vec4<f32>(final_world_pos, 1.0);
    out.shadow_pos = vec3<f32>(
        shadow_clip.x * 0.5 + 0.5,
        -shadow_clip.y * 0.5 + 0.5,
        shadow_clip.z
    );

    return out;
}

@fragment
fn fs_leaf(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_leaf_albedo, s_leaf, in.uv);

    // Direct alpha cutout without pre-discard fwidth() quad sync stall (Castano mipmaps preserve distant edge coverage)
    if (tex_color.a < 0.32) {
        discard;
    }

    let to_cam = camera.camera_pos.xyz - in.world_pos;
    let dist = length(to_cam);
    let view_dir = to_cam / max(dist, 0.0001);

    var normal_leaf = in.world_normal;
    if (dist < 32.0) {
        let norm_mip = clamp(log2(max(dist * 0.25, 1.0)), 0.0, 4.0);
        let raw_normal = textureSampleLevel(t_leaf_normal, s_leaf, in.uv, norm_mip).xyz * 2.0 - 1.0;
        let v_up = vec3<f32>(0.0, 1.0, 0.0);
        var t_vec = cross(in.world_normal, v_up);
        if (dot(t_vec, t_vec) < 0.0001) { t_vec = vec3<f32>(1.0, 0.0, 0.0); } else { t_vec = normalize(t_vec); }
        let b_vec = cross(in.world_normal, t_vec);
        let tbn = mat3x3<f32>(t_vec, b_vec, in.world_normal);
        let detail_fade = clamp((32.0 - dist) / 8.0, 0.0, 1.0);
        normal_leaf = normalize(tbn * (raw_normal * vec3<f32>(1.5 * detail_fade, 1.5 * detail_fade, 1.0)));
    }
    let normal_dir = select(-normal_leaf, normal_leaf, is_front);

    let canopy_ao = clamp(in.height / 5.5, 0.48, 1.0);
    let albedo = tex_color.rgb * in.color.rgb * canopy_ao;
    let light_dir = normalize(camera.light_dir.xyz);

    // 16-Tap Rotated Poisson PCF Soft Shadow Filtering on Canopy Leaves
    var shadow: f32 = 1.0;
    let shadow_tier = camera.quality_tiers.x;
    if (dist < 72.0 && shadow_tier > 0u && in.shadow_pos.x >= 0.0 && in.shadow_pos.x <= 1.0 && in.shadow_pos.y >= 0.0 && in.shadow_pos.y <= 1.0 && in.shadow_pos.z >= 0.0 && in.shadow_pos.z <= 1.0) {
        let bias = 0.0032;
        let shadow_depth = in.shadow_pos.z - bias;

        if (shadow_tier == 1u) {
            shadow = textureSampleCompareLevel(t_shadow, s_shadow, in.shadow_pos.xy, shadow_depth);
        } else {
            let texel_size = 1.0 / 2048.0;
            let ang = ign(in.clip_position.xy) * 6.2831853;
            let ca = cos(ang);
            let sa = sin(ang);
            let rot = mat2x2<f32>(vec2<f32>(ca, sa), vec2<f32>(-sa, ca));
            let spread = texel_size * select(1.6, 2.4, shadow_tier >= 3u);

            var shadow_sum: f32 = 0.0;
            for (var p = 0u; p < 16u; p = p + 1u) {
                let offset = rot * get_poisson_16(p) * spread;
                shadow_sum += textureSampleCompareLevel(t_shadow, s_shadow, in.shadow_pos.xy + offset, shadow_depth);
            }
            shadow = shadow_sum * (1.0 / 16.0);
        }
    }

    // Leaf Ambient + Double-Sided Sun Diffuse + Subsurface Scattering (Chlorophyll)
    let ambient = vec3<f32>(0.13, 0.19, 0.26) * 0.28;
    let ndotl = max(dot(normal_dir, light_dir), 0.0);
    let ndotv = max(dot(normal_dir, view_dir), 0.001);
    let diff = max(dot(normal_dir, light_dir) * 0.65 + 0.35, 0.0);
    let sun_color = vec3<f32>(0.85, 0.92, 1.0);

    // Subsurface Scattering (Chlorophyll Backlight Translucency)
    let sun_back = max(dot(view_dir, -light_dir), 0.0);
    let sun_sss = sun_back * sun_back * sun_back * 0.60;
    let sss_tint = vec3<f32>(0.32, 0.74, 0.16);

    // Waxy Cuticle Leaf Specular Sheen (Microfacet Cook-Torrance GGX)
    let leaf_roughness: f32 = 0.38;
    let f0: f32 = 0.04;
    let half_moon = normalize(light_dir + view_dir);
    let ndoth = max(dot(normal_dir, half_moon), 0.0);
    let vdoth = max(dot(view_dir, half_moon), 0.0);
    let fm = f_schlick(vdoth, f0);
    let dm = d_ggx(ndoth, leaf_roughness);
    let vm = v_smith_ggx(ndotv, ndotl, leaf_roughness);
    let leaf_moon_spec = min(dm * vm * fm, 3.5) * ndotl * shadow;

    var lit_color = (ambient + diff * sun_color * shadow) * albedo + sun_sss * sss_tint * sun_color * shadow * albedo + leaf_moon_spec * sun_color;

    // Dual-Zone Tactical Flashlight with Fast Algebraic Cone Rejection (Zero-Sqrt Outside Cone)
    if (camera.flashlight_pos.w > 0.5) {
        let to_light = camera.flashlight_pos.xyz - in.world_pos;
        let proj = -dot(to_light, camera.flashlight_dir.xyz);
        let l_dist_sq = dot(to_light, to_light);
        if (proj > 0.0 && l_dist_sq < 2025.0 && proj * proj > 0.7744 * l_dist_sq) {
            let inv_dist = inverseSqrt(max(l_dist_sq, 0.000001));
            let l_dist = l_dist_sq * inv_dist;
            let l_dir = to_light * inv_dist;
            let spot_dot = proj * inv_dist;

            let hotspot = smoothstep(0.94, 0.988, spot_dot);
            let flood = smoothstep(0.82, 0.94, spot_dot);
            let beam_profile = hotspot * 0.90 + flood * 0.35;

            let atten = 1.0 / (1.0 + 0.065 * l_dist + 0.018 * l_dist_sq);
            let dist_cutoff = clamp((54.0 - l_dist) / 10.0, 0.0, 1.0);
            let flash_color = vec3<f32>(1.0, 0.96, 0.88);
            let flash_power = beam_profile * atten * dist_cutoff * camera.flashlight_dir.w * 1.10;

            let flash_diff = max(dot(normal_dir, l_dir), 0.0);
            let flash_back = max(dot(view_dir, -l_dir), 0.0);
            let flash_sss = flash_back * flash_back * sqrt(flash_back) * 0.35;

            let half_flash = normalize(l_dir + view_dir);
            let f_ndoth = max(dot(normal_dir, half_flash), 0.0);
            let f_vdoth = max(dot(view_dir, half_flash), 0.0);
            let f_ndotl = max(dot(normal_dir, l_dir), 0.0);
            let ff = f_schlick(f_vdoth, f0);
            let fd = d_ggx(f_ndoth, leaf_roughness);
            let fv = v_smith_ggx(ndotv, f_ndotl, leaf_roughness);
            let flash_spec = min(fd * fv * ff, 0.70) * f_ndotl;

            let total_flash = (flash_diff * albedo + flash_sss * albedo * sss_tint) + flash_spec * flash_color;
            lit_color += total_flash * flash_color * flash_power;
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

pub struct ForestPipeline {
    pub bark_pipeline: wgpu::RenderPipeline,
    pub leaf_pipeline: wgpu::RenderPipeline,
    pub shadow_pipeline: wgpu::RenderPipeline,
    pub shadow_leaf_pipeline: wgpu::RenderPipeline,
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub variants: Vec<ForestVariant>,
    pub oak_material_bind_group: wgpu::BindGroup,
    pub bush_material_bind_group: wgpu::BindGroup,
    pub instance_buffers: Vec<wgpu::Buffer>,
    pub shadow_instance_buffers: Vec<wgpu::Buffer>,
    pub max_instances_per_variant: usize,
}

impl ForestPipeline {
    pub const MAX_INSTANCES_PER_VARIANT: usize = 4096;

    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
        binary_path: &Path,
        texture_dir: &Path,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // 1. Read forest_assets.bin
        let mut file = File::open(binary_path)?;
        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)?;
        if &magic != b"FRST" {
            return Err("Invalid forest asset binary magic".into());
        }

        let mut buf4 = [0u8; 4];
        file.read_exact(&mut buf4)?;
        let _version = u32::from_le_bytes(buf4);
        file.read_exact(&mut buf4)?;
        let variant_count = u32::from_le_bytes(buf4) as usize;

        let mut all_vertices: Vec<ForestVertex> = Vec::new();
        let mut all_indices: Vec<u16> = Vec::new();
        let mut variants = Vec::with_capacity(variant_count);

        for _ in 0..variant_count {
            file.read_exact(&mut buf4)?;
            let name_len = u32::from_le_bytes(buf4) as usize;
            file.read_exact(&mut buf4)?;
            let kind = u32::from_le_bytes(buf4);

            let mut name_bytes = vec![0u8; name_len];
            file.read_exact(&mut name_bytes)?;
            let name = String::from_utf8_lossy(&name_bytes).to_string();

            file.read_exact(&mut buf4)?;
            let collider_radius = f32::from_le_bytes(buf4);

            file.read_exact(&mut buf4)?;
            let bark_vcount = u32::from_le_bytes(buf4) as usize;
            file.read_exact(&mut buf4)?;
            let bark_icount = u32::from_le_bytes(buf4) as usize;

            file.read_exact(&mut buf4)?;
            let leaf_vcount = u32::from_le_bytes(buf4) as usize;
            file.read_exact(&mut buf4)?;
            let leaf_icount = u32::from_le_bytes(buf4) as usize;

            // Bark Geometry - full intact geometry preserves complete trunks & branches without amputation
            let bark_v_offset = all_vertices.len() as u32;
            let mut bark_raw_verts = vec![0u8; bark_vcount * 32];
            file.read_exact(&mut bark_raw_verts)?;
            let bark_v_slice: &[ForestVertex] = bytemuck::cast_slice(&bark_raw_verts);
            all_vertices.extend_from_slice(bark_v_slice);

            let mut bark_raw_indices = vec![0u8; bark_icount * 2];
            file.read_exact(&mut bark_raw_indices)?;
            let bark_i_slice: &[u16] = bytemuck::cast_slice(&bark_raw_indices);
            let bark_i_offset = all_indices.len() as u32;
            all_indices.extend_from_slice(bark_i_slice);

            let bark_range = MeshRange {
                vertex_offset: bark_v_offset,
                vertex_count: bark_vcount as u32,
                index_offset: bark_i_offset,
                index_count: bark_icount as u32,
            };
            let bark_lods = [bark_range, bark_range, bark_range];

            // Leaf Geometry - full intact canopy foliage cards without missing holes
            let leaf_v_offset = all_vertices.len() as u32;
            let mut leaf_raw_verts = vec![0u8; leaf_vcount * 32];
            file.read_exact(&mut leaf_raw_verts)?;
            let leaf_v_slice: &[ForestVertex] = bytemuck::cast_slice(&leaf_raw_verts);
            all_vertices.extend_from_slice(leaf_v_slice);

            let mut leaf_raw_indices = vec![0u8; leaf_icount * 2];
            file.read_exact(&mut leaf_raw_indices)?;
            let leaf_i_slice: &[u16] = bytemuck::cast_slice(&leaf_raw_indices);
            let leaf_i_offset = all_indices.len() as u32;
            all_indices.extend_from_slice(leaf_i_slice);

            let leaf_range = MeshRange {
                vertex_offset: leaf_v_offset,
                vertex_count: leaf_vcount as u32,
                index_offset: leaf_i_offset,
                index_count: leaf_icount as u32,
            };
            let leaf_lods = [leaf_range, leaf_range, leaf_range];

            // Compute exact local-space bounding spheres for bark (shadow pass) and full mesh (camera pass)
            let mut bark_min_y = f32::MAX;
            let mut bark_max_y = f32::MIN;
            for v in bark_v_slice {
                bark_min_y = bark_min_y.min(v.position[1]);
                bark_max_y = bark_max_y.max(v.position[1]);
            }
            if bark_min_y > bark_max_y {
                bark_min_y = 0.0;
                bark_max_y = 1.0;
            }
            let bark_y_offset = (bark_min_y + bark_max_y) * 0.5;
            let mut max_bark_dist_sq = 0.0f32;
            for v in bark_v_slice {
                let dy = v.position[1] - bark_y_offset;
                let d_sq = v.position[0] * v.position[0] + dy * dy + v.position[2] * v.position[2];
                max_bark_dist_sq = max_bark_dist_sq.max(d_sq);
            }
            let bark_radius = max_bark_dist_sq.sqrt() + 0.15;

            let mut full_min_y = bark_min_y;
            let mut full_max_y = bark_max_y;
            for v in leaf_v_slice {
                full_min_y = full_min_y.min(v.position[1]);
                full_max_y = full_max_y.max(v.position[1]);
            }
            let bound_y_offset = (full_min_y + full_max_y) * 0.5;
            let mut max_full_dist_sq = 0.0f32;
            for v in bark_v_slice.iter().chain(leaf_v_slice.iter()) {
                let dy = v.position[1] - bound_y_offset;
                let d_sq = v.position[0] * v.position[0] + dy * dy + v.position[2] * v.position[2];
                max_full_dist_sq = max_full_dist_sq.max(d_sq);
            }
            let bound_radius = max_full_dist_sq.sqrt() + 0.25;

            variants.push(ForestVariant {
                name,
                kind,
                collider_radius,
                bound_y_offset,
                bound_radius,
                bark_y_offset,
                bark_radius,
                bark_range: bark_lods[0],
                leaf_range: leaf_lods[0],
                bark_lods,
                leaf_lods,
            });
        }

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Forest Consolidated Vertex Buffer"),
            contents: bytemuck::cast_slice(&all_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Forest Consolidated Index Buffer"),
            contents: bytemuck::cast_slice(&all_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // 2. Load PBR Textures
        let bark_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Forest Bark Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });

        let leaf_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Forest Leaf Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 4,
            ..Default::default()
        });

        let material_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Forest Material Bind Group Layout"),
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
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });

        // Oak material
        let oak_bark_bytes = std::fs::read(texture_dir.join("oak_bark_albedo.png"))?;
        let oak_bark_norm_bytes = std::fs::read(texture_dir.join("oak_bark_normal.png"))?;
        let oak_leaf_bytes = std::fs::read(texture_dir.join("oak_leaf_albedo.png"))?;
        let oak_leaf_norm_bytes = std::fs::read(texture_dir.join("oak_leaf_normal.png"))?;

        let (_, oak_bark_v) = crate::terrain::create_texture_from_bytes(
            device, queue, &oak_bark_bytes, "Oak Bark Albedo", true,
        )?;
        let (_, oak_bark_norm_v) = crate::terrain::create_texture_from_bytes(
            device, queue, &oak_bark_norm_bytes, "Oak Bark Normal", false,
        )?;
        let (_, oak_leaf_v) = crate::terrain::create_texture_from_bytes(
            device, queue, &oak_leaf_bytes, "Oak Leaf Albedo", true,
        )?;
        let (_, oak_leaf_norm_v) = crate::terrain::create_texture_from_bytes(
            device, queue, &oak_leaf_norm_bytes, "Oak Leaf Normal", false,
        )?;

        let oak_material_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Oak Material Bind Group"),
            layout: &material_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&oak_bark_v) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&bark_sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&oak_bark_norm_v) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&oak_leaf_v) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&leaf_sampler) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&oak_leaf_norm_v) },
            ],
        });

        // Bush material
        let bush_bark_bytes = std::fs::read(texture_dir.join("bush_bark_albedo.png"))?;
        let bush_bark_norm_bytes = std::fs::read(texture_dir.join("bush_bark_normal.png"))?;
        let bush_leaf_bytes = std::fs::read(texture_dir.join("bush_leaf_albedo.png"))?;
        let bush_leaf_norm_bytes = std::fs::read(texture_dir.join("bush_leaf_normal.png"))?;

        let (_, bush_bark_v) = crate::terrain::create_texture_from_bytes(
            device, queue, &bush_bark_bytes, "Bush Bark Albedo", true,
        )?;
        let (_, bush_bark_norm_v) = crate::terrain::create_texture_from_bytes(
            device, queue, &bush_bark_norm_bytes, "Bush Bark Normal", false,
        )?;
        let (_, bush_leaf_v) = crate::terrain::create_texture_from_bytes(
            device, queue, &bush_leaf_bytes, "Bush Leaf Albedo", true,
        )?;
        let (_, bush_leaf_norm_v) = crate::terrain::create_texture_from_bytes(
            device, queue, &bush_leaf_norm_bytes, "Bush Leaf Normal", false,
        )?;

        let bush_material_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bush Material Bind Group"),
            layout: &material_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&bush_bark_v) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&bark_sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&bush_bark_norm_v) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&bush_leaf_v) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&leaf_sampler) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&bush_leaf_norm_v) },
            ],
        });

        // 3. Render Pipelines
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Forest Shader Module"),
            source: wgpu::ShaderSource::Wgsl(FOREST_SHADER_WGSL.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Forest Pipeline Layout"),
            bind_group_layouts: &[camera_bind_group_layout, &material_bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_layouts = [
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ForestVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                    wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
                    wgpu::VertexAttribute { offset: 24, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
                ],
            },
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ForestInstanceRaw>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x4 },
                    wgpu::VertexAttribute { offset: 16, shader_location: 4, format: wgpu::VertexFormat::Float32x4 },
                    wgpu::VertexAttribute { offset: 32, shader_location: 5, format: wgpu::VertexFormat::Float32x4 },
                    wgpu::VertexAttribute { offset: 48, shader_location: 6, format: wgpu::VertexFormat::Float32x4 },
                    wgpu::VertexAttribute { offset: 64, shader_location: 7, format: wgpu::VertexFormat::Float32x4 },
                ],
            },
        ];

        // Bark: Opaque backface culled
        let bark_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Forest Bark Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_bark",
                buffers: &vertex_layouts,
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_bark",
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
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Leaf: Double-sided cutout
        let leaf_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Forest Leaf Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_leaf",
                buffers: &vertex_layouts,
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_leaf",
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
                cull_mode: None, // Double-sided leaves
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Shadow Pass Pipeline
        let shadow_camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Forest Shadow Camera Bind Group Layout"),
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

        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Forest Shadow Pipeline Layout"),
                bind_group_layouts: &[&shadow_camera_bind_group_layout],
                push_constant_ranges: &[],
            });

        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Forest Shadow Pipeline"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_shadow",
                buffers: &vertex_layouts,
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back), // Closed manifold oak trunks only
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::mesh3d::Mesh3DPipeline::SHADOW_FORMAT,
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

        let shadow_leaf_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Forest Shadow Leaf Pipeline"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_shadow",
                buffers: &vertex_layouts,
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None, // Double-sided leaf cards for full canopy shadow projection
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::mesh3d::Mesh3DPipeline::SHADOW_FORMAT,
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

        // 4. Per-variant instance buffers (camera frustum culled + light-space shadow culled)
        let mut instance_buffers = Vec::with_capacity(variants.len());
        let mut shadow_instance_buffers = Vec::with_capacity(variants.len());
        let instance_buf_size =
            (Self::MAX_INSTANCES_PER_VARIANT * std::mem::size_of::<ForestInstanceRaw>()) as u64;
        for _ in 0..variants.len() {
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Forest Variant Instance Buffer"),
                size: instance_buf_size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            instance_buffers.push(buf);

            let shadow_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Forest Variant Shadow Instance Buffer"),
                size: instance_buf_size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            shadow_instance_buffers.push(shadow_buf);
        }

        Ok(Self {
            bark_pipeline,
            leaf_pipeline,
            shadow_pipeline,
            shadow_leaf_pipeline,
            vertex_buffer,
            index_buffer,
            variants,
            oak_material_bind_group,
            bush_material_bind_group,
            instance_buffers,
            shadow_instance_buffers,
            max_instances_per_variant: Self::MAX_INSTANCES_PER_VARIANT,
        })
    }

    pub fn update_variant_instances(
        &self,
        queue: &wgpu::Queue,
        variant_idx: usize,
        instances: &[ForestInstanceRaw],
    ) {
        if variant_idx < self.instance_buffers.len() && !instances.is_empty() {
            let count = instances.len().min(self.max_instances_per_variant);
            let bytes: &[u8] = bytemuck::cast_slice(&instances[..count]);
            queue.write_buffer(&self.instance_buffers[variant_idx], 0, bytes);
        }
    }

    pub fn update_shadow_variant_instances(
        &self,
        queue: &wgpu::Queue,
        variant_idx: usize,
        instances: &[ForestInstanceRaw],
    ) {
        if variant_idx < self.shadow_instance_buffers.len() && !instances.is_empty() {
            let count = instances.len().min(self.max_instances_per_variant);
            let bytes: &[u8] = bytemuck::cast_slice(&instances[..count]);
            queue.write_buffer(&self.shadow_instance_buffers[variant_idx], 0, bytes);
        }
    }

    pub fn render_bark<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        camera_bind_group: &'a wgpu::BindGroup,
        variant_counts: &[usize],
    ) {
        rpass.set_pipeline(&self.bark_pipeline);
        rpass.set_bind_group(0, camera_bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        let mut current_mat_kind = None;

        for (variant_idx, variant) in self.variants.iter().enumerate() {
            let count = if variant_idx < variant_counts.len() {
                variant_counts[variant_idx].min(self.max_instances_per_variant) as u32
            } else {
                0
            };
            if count == 0 || variant.bark_range.index_count == 0 {
                continue;
            }

            if current_mat_kind != Some(variant.kind) {
                if variant.kind == 0 {
                    rpass.set_bind_group(1, &self.oak_material_bind_group, &[]);
                } else {
                    rpass.set_bind_group(1, &self.bush_material_bind_group, &[]);
                }
                current_mat_kind = Some(variant.kind);
            }

            rpass.set_vertex_buffer(1, self.instance_buffers[variant_idx].slice(..));
            rpass.draw_indexed(
                variant.bark_range.index_offset..(variant.bark_range.index_offset + variant.bark_range.index_count),
                variant.bark_range.vertex_offset as i32,
                0..count,
            );
        }
    }

    pub fn render_bark_lod<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        camera_bind_group: &'a wgpu::BindGroup,
        variant_lod_ends: &[[u32; 3]],
    ) {
        rpass.set_pipeline(&self.bark_pipeline);
        rpass.set_bind_group(0, camera_bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        let mut current_mat_kind = None;

        for (variant_idx, variant) in self.variants.iter().enumerate() {
            if variant_idx >= variant_lod_ends.len() {
                break;
            }
            let ends = variant_lod_ends[variant_idx];
            let total = ends[2].min(self.max_instances_per_variant as u32);
            if total == 0 || variant.bark_range.index_count == 0 {
                continue;
            }

            if current_mat_kind != Some(variant.kind) {
                if variant.kind == 0 {
                    rpass.set_bind_group(1, &self.oak_material_bind_group, &[]);
                } else {
                    rpass.set_bind_group(1, &self.bush_material_bind_group, &[]);
                }
                current_mat_kind = Some(variant.kind);
            }

            rpass.set_vertex_buffer(1, self.instance_buffers[variant_idx].slice(..));
            let mut start = 0u32;
            for (lod, &end_raw) in ends.iter().enumerate() {
                let end = end_raw.min(total);
                if end > start {
                    let range = variant.bark_lods[lod];
                    if range.index_count > 0 {
                        rpass.draw_indexed(
                            range.index_offset..(range.index_offset + range.index_count),
                            range.vertex_offset as i32,
                            start..end,
                        );
                    }
                    start = end;
                }
            }
        }
    }

    pub fn render_leaf<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        camera_bind_group: &'a wgpu::BindGroup,
        variant_counts: &[usize],
    ) {
        rpass.set_pipeline(&self.leaf_pipeline);
        rpass.set_bind_group(0, camera_bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        let mut current_mat_kind = None;

        for (variant_idx, variant) in self.variants.iter().enumerate() {
            let count = if variant_idx < variant_counts.len() {
                variant_counts[variant_idx].min(self.max_instances_per_variant) as u32
            } else {
                0
            };
            if count == 0 || variant.leaf_range.index_count == 0 {
                continue;
            }

            if current_mat_kind != Some(variant.kind) {
                if variant.kind == 0 {
                    rpass.set_bind_group(1, &self.oak_material_bind_group, &[]);
                } else {
                    rpass.set_bind_group(1, &self.bush_material_bind_group, &[]);
                }
                current_mat_kind = Some(variant.kind);
            }

            rpass.set_vertex_buffer(1, self.instance_buffers[variant_idx].slice(..));
            rpass.draw_indexed(
                variant.leaf_range.index_offset..(variant.leaf_range.index_offset + variant.leaf_range.index_count),
                variant.leaf_range.vertex_offset as i32,
                0..count,
            );
        }
    }

    pub fn render_leaf_lod<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        camera_bind_group: &'a wgpu::BindGroup,
        variant_lod_ends: &[[u32; 3]],
    ) {
        rpass.set_pipeline(&self.leaf_pipeline);
        rpass.set_bind_group(0, camera_bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        let mut current_mat_kind = None;

        for (variant_idx, variant) in self.variants.iter().enumerate() {
            if variant_idx >= variant_lod_ends.len() {
                break;
            }
            let ends = variant_lod_ends[variant_idx];
            let total = ends[2].min(self.max_instances_per_variant as u32);
            if total == 0 || variant.leaf_range.index_count == 0 {
                continue;
            }

            if current_mat_kind != Some(variant.kind) {
                if variant.kind == 0 {
                    rpass.set_bind_group(1, &self.oak_material_bind_group, &[]);
                } else {
                    rpass.set_bind_group(1, &self.bush_material_bind_group, &[]);
                }
                current_mat_kind = Some(variant.kind);
            }

            rpass.set_vertex_buffer(1, self.instance_buffers[variant_idx].slice(..));
            let mut start = 0u32;
            for (lod, &end_raw) in ends.iter().enumerate() {
                let end = end_raw.min(total);
                if end > start {
                    let range = variant.leaf_lods[lod];
                    if range.index_count > 0 {
                        rpass.draw_indexed(
                            range.index_offset..(range.index_offset + range.index_count),
                            range.vertex_offset as i32,
                            start..end,
                        );
                    }
                    start = end;
                }
            }
        }
    }

    /// Render directional shadow map: draws oak tree trunks (`bark_lods[1]`) with Back-face culling
    /// and oak leaf canopies (`leaf_lods[1]`) with double-sided shadow rasterization, casting rich dappled
    /// canopy shadows onto the forest floor and enabling volumetric God rays through canopy gaps.
    pub fn render_shadow<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        shadow_camera_bind_group: &'a wgpu::BindGroup,
        variant_counts: &[usize],
    ) {
        rpass.set_bind_group(0, shadow_camera_bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        // 1. Draw opaque tree trunks (Back-face culled)
        rpass.set_pipeline(&self.shadow_pipeline);
        for (variant_idx, variant) in self.variants.iter().enumerate() {
            let shadow_range = variant.bark_lods[1];
            if variant.kind != 0 || shadow_range.index_count == 0 {
                continue;
            }
            let count = if variant_idx < variant_counts.len() {
                variant_counts[variant_idx].min(self.max_instances_per_variant) as u32
            } else {
                0
            };
            if count == 0 {
                continue;
            }

            rpass.set_vertex_buffer(1, self.shadow_instance_buffers[variant_idx].slice(..));
            rpass.draw_indexed(
                shadow_range.index_offset..(shadow_range.index_offset + shadow_range.index_count),
                shadow_range.vertex_offset as i32,
                0..count,
            );
        }

        // 2. Draw double-sided canopy leaf clusters (No culling)
        rpass.set_pipeline(&self.shadow_leaf_pipeline);
        for (variant_idx, variant) in self.variants.iter().enumerate() {
            let leaf_range = variant.leaf_lods[1];
            if variant.kind != 0 || leaf_range.index_count == 0 {
                continue;
            }
            let count = if variant_idx < variant_counts.len() {
                variant_counts[variant_idx].min(self.max_instances_per_variant) as u32
            } else {
                0
            };
            if count == 0 {
                continue;
            }

            rpass.set_vertex_buffer(1, self.shadow_instance_buffers[variant_idx].slice(..));
            rpass.draw_indexed(
                leaf_range.index_offset..(leaf_range.index_offset + leaf_range.index_count),
                leaf_range.vertex_offset as i32,
                0..count,
            );
        }
    }
}

/// Cached 32m x 32m forest chunk holding scattered oak tree & lilac bush instances and trunk colliders.
pub struct ForestChunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub min_x: f32,
    pub min_z: f32,
    pub instances_by_variant: Vec<Vec<ForestInstanceRaw>>,
    pub colliders: Vec<TreeCollider>,
}

impl ForestChunk {
    pub fn generate(chunk_x: i32, chunk_z: i32, variants: &[ForestVariant]) -> Self {
        let mut instances_by_variant = (0..variants.len())
            .map(|_| Vec::with_capacity(8))
            .collect::<Vec<_>>();
        let mut colliders = Vec::with_capacity(16);
        scatter_chunk_forest(
            chunk_x,
            chunk_z,
            variants,
            &mut instances_by_variant,
            &mut colliders,
        );
        Self {
            chunk_x,
            chunk_z,
            min_x: (chunk_x as f32) * 32.0,
            min_z: (chunk_z as f32) * 32.0,
            instances_by_variant,
            colliders,
        }
    }
}

/// Hierarchical Camera-Frustum & Distance Culling for forest chunks.
/// Level 1 culls 32m x 32m chunks against the 70° FOV camera frustum and view distance.
/// Level 2 culls individual tree and bush bounding spheres (using exact per-variant geometry bounds
/// scaled by each instance's scale factor) within intersecting chunks, and sorts visible instances
/// front-to-back by horizontal distance squared from `cam_pos` for Early-Z rejection and LOD band slicing.
pub fn cull_forest_chunks_camera(
    chunks: &[ForestChunk],
    frustum: &crate::culling::Frustum,
    cam_pos: Vec3,
    max_view_dist: f32,
    variants: &[ForestVariant],
    out_instances_by_variant: &mut [Vec<ForestInstanceRaw>],
) {
    for list in out_instances_by_variant.iter_mut() {
        list.clear();
    }

    let max_chunk_dist_sq = (max_view_dist + 34.0) * (max_view_dist + 34.0);
    let max_tree_dist_sq = (max_view_dist + 16.0) * (max_view_dist + 16.0);
    let bush_dist = (max_view_dist * 0.56).min(104.0);
    let max_bush_dist_sq = bush_dist * bush_dist;

    for chunk in chunks {
        let cx_center = chunk.min_x + 16.0;
        let cz_center = chunk.min_z + 16.0;
        let dx = cx_center - cam_pos.x;
        let dz = cz_center - cam_pos.z;
        if dx * dx + dz * dz > max_chunk_dist_sq {
            continue;
        }

        // Pad chunk bounds by 14.0m horizontally for overhanging Large Oak canopies and [-2.0..36.0] vertically
        if !frustum.intersects_chunk(chunk.min_x - 14.0, chunk.min_z - 14.0, 60.0, -2.0, 36.0) {
            continue;
        }

        for (variant_idx, instances) in chunk.instances_by_variant.iter().enumerate() {
            if instances.is_empty()
                || variant_idx >= out_instances_by_variant.len()
                || variant_idx >= variants.len()
            {
                continue;
            }
            let variant = &variants[variant_idx];
            let dist_limit_sq = if variant.kind == 0 {
                max_tree_dist_sq
            } else {
                max_bush_dist_sq
            };

            let dst = &mut out_instances_by_variant[variant_idx];
            for inst in instances {
                if dst.len() >= ForestPipeline::MAX_INSTANCES_PER_VARIANT {
                    break;
                }
                let wx = inst.model_3[0];
                let wy = inst.model_3[1];
                let wz = inst.model_3[2];

                let idx = wx - cam_pos.x;
                let idz = wz - cam_pos.z;
                if idx * idx + idz * idz > dist_limit_sq {
                    continue;
                }

                let scale = inst.model_1[1].max(0.5);
                let sphere = crate::culling::BoundingSphere {
                    center: Vec3::new(wx, wy + variant.bound_y_offset * scale, wz),
                    radius: variant.bound_radius * scale,
                };
                if frustum.intersects_sphere(&sphere) {
                    dst.push(*inst);
                }
            }
        }
    }

    // Sort each variant's culled instances front-to-back relative to cam_pos.xz
    // This enables hardware Early-Z / Hi-Z occlusion and contiguous [0..end0, end0..end1, end1..end2] LOD slicing.
    for dst in out_instances_by_variant.iter_mut() {
        if dst.len() > 1 {
            dst.sort_unstable_by(|a, b| {
                let adx = a.model_3[0] - cam_pos.x;
                let adz = a.model_3[2] - cam_pos.z;
                let bdx = b.model_3[0] - cam_pos.x;
                let bdz = b.model_3[2] - cam_pos.z;
                (adx * adx + adz * adz)
                    .partial_cmp(&(bdx * bdx + bdz * bdz))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }
}

/// Compute cumulative instance end indices `[lod0_end, lod1_end, lod2_end]` for each front-to-back sorted variant.
pub fn compute_forest_lod_ends(
    instances_by_variant: &[Vec<ForestInstanceRaw>],
    variants: &[ForestVariant],
    cam_pos: Vec3,
    out_lod_ends: &mut [[u32; 3]],
) {
    for (variant_idx, instances) in instances_by_variant.iter().enumerate() {
        if variant_idx >= out_lod_ends.len() {
            break;
        }
        let kind = variants.get(variant_idx).map(|v| v.kind).unwrap_or(0);
        let (d0_sq, d1_sq) = if kind == 0 {
            (26.0 * 26.0, 64.0 * 64.0)
        } else {
            (22.0 * 22.0, 52.0 * 52.0)
        };

        let total = instances.len().min(ForestPipeline::MAX_INSTANCES_PER_VARIANT);
        let mut end0 = 0u32;
        let mut end1 = 0u32;
        for (i, inst) in instances.iter().take(total).enumerate() {
            let dx = inst.model_3[0] - cam_pos.x;
            let dz = inst.model_3[2] - cam_pos.z;
            let dist_sq = dx * dx + dz * dz;
            if dist_sq <= d0_sq {
                end0 = (i + 1) as u32;
                end1 = (i + 1) as u32;
            } else if dist_sq <= d1_sq {
                end1 = (i + 1) as u32;
            }
        }
        out_lod_ends[variant_idx] = [end0, end1, total as u32];
    }
}

/// Light-Space Orthographic Volume Culling for Pass 0 (Directional Shadow Pass).
/// Extracts the 6 orthographic frustum planes from `light_view_proj` and gathers ONLY oak tree trunks
/// (`variant.kind == 0`) whose scaled bark bounding sphere intersects the 130m x 130m directional light box.
pub fn cull_forest_chunks_shadow(
    chunks: &[ForestChunk],
    light_view_proj: Mat4,
    variants: &[ForestVariant],
    out_shadow_by_variant: &mut [Vec<ForestInstanceRaw>],
) {
    for list in out_shadow_by_variant.iter_mut() {
        list.clear();
    }

    let light_frustum = crate::culling::Frustum::from_view_proj(light_view_proj);

    for chunk in chunks {
        // Level 1: Coarse 32m x 32m chunk AABB test (padded for overhanging oak trunks/branches)
        if !light_frustum.intersects_chunk(chunk.min_x - 14.0, chunk.min_z - 14.0, 60.0, -2.0, 30.0)
        {
            continue;
        }

        for (variant_idx, variant) in variants.iter().enumerate() {
            // Skip lilac bushes (kind != 0) in shadow pass
            if variant.kind != 0 || variant_idx >= out_shadow_by_variant.len() {
                continue;
            }
            let instances = &chunk.instances_by_variant[variant_idx];
            if instances.is_empty() {
                continue;
            }

            let dst = &mut out_shadow_by_variant[variant_idx];
            for inst in instances {
                if dst.len() >= ForestPipeline::MAX_INSTANCES_PER_VARIANT {
                    break;
                }
                let wx = inst.model_3[0];
                let wy = inst.model_3[1];
                let wz = inst.model_3[2];
                let scale = inst.model_1[1].max(0.5);

                let sphere = crate::culling::BoundingSphere {
                    center: Vec3::new(wx, wy + variant.bound_y_offset * scale, wz),
                    radius: variant.bound_radius * scale,
                };
                if light_frustum.intersects_sphere(&sphere) {
                    dst.push(*inst);
                }
            }
        }
    }
}

/// Deterministic dense dark forest scattering.
/// Generates oak trees and lilac bushes with physical trunk colliders.
pub fn scatter_chunk_forest(
    chunk_x: i32,
    chunk_z: i32,
    variants: &[ForestVariant],
    instances_by_variant: &mut [Vec<ForestInstanceRaw>],
    colliders_out: &mut Vec<TreeCollider>,
) {
    if variants.is_empty() {
        return;
    }

    let mut seed = ((chunk_x as u64) << 32) ^ (chunk_z as u64 & 0xFFFF_FFFF);
    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);

    // Number of trees in this 32x32m chunk (dense horror forest: 4 to 6 oak trees per chunk)
    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let tree_count = 4 + (((seed >> 32) & 0xFFFF) % 3) as usize;

    // Number of bushes (dense undergrowth: 6 to 9 bushes per chunk)
    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let bush_count = 6 + (((seed >> 32) & 0xFFFF) % 4) as usize;

    // 1. Scatter Oak Trees (variants 0..5)
    for _ in 0..tree_count {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let lx = 2.0 + (((seed >> 32) & 0xFFFF) as f32 / 65535.0) * 28.0;

        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let lz = 2.0 + (((seed >> 32) & 0xFFFF) as f32 / 65535.0) * 28.0;

        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let ryaw = (((seed >> 32) & 0xFFFF) as f32 / 65535.0) * std::f32::consts::TAU;

        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let rscale = 0.85 + (((seed >> 32) & 0xFFFF) as f32 / 65535.0) * 0.40;

        // Oak variants are 0..5
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let variant_idx = ((seed >> 32) as usize) % 5.min(variants.len());

        let wx = (chunk_x as f32) * 32.0 + lx;
        let wz = (chunk_z as f32) * 32.0 + lz;
        let wy = crate::terrain::terrain_height(wx, wz) - 0.15; // Sink slightly to seat roots firmly

        let model = Mat4::from_scale_rotation_translation(
            Vec3::splat(rscale),
            Quat::from_rotation_y(ryaw),
            Vec3::new(wx, wy, wz),
        );

        // Continuous world-space chlorophyll hue & bark darkness variation per tree
        let wave_r = (wx * 0.045 + wz * 0.032).sin() * 0.09;
        let wave_g = (wx * 0.038 - wz * 0.051).cos() * 0.09;
        let rand_tint = (((seed >> 16) & 0xFFFF) as f32 / 65535.0 - 0.5) * 0.12;
        let color_tint = [
            (0.92 + wave_r + rand_tint).clamp(0.75, 1.12),
            (0.94 + wave_g + rand_tint * 0.8).clamp(0.78, 1.12),
            (0.88 - wave_r * 0.5 + rand_tint * 0.5).clamp(0.72, 1.08),
            1.0,
        ];
        if variant_idx < instances_by_variant.len()
            && instances_by_variant[variant_idx].len() < ForestPipeline::MAX_INSTANCES_PER_VARIANT
        {
            instances_by_variant[variant_idx].push(ForestInstanceRaw::new(model, color_tint));
        }

        // Register trunk collider
        let radius = variants[variant_idx].collider_radius * rscale;
        colliders_out.push(TreeCollider {
            center: Vec2::new(wx, wz),
            radius,
        });
    }

    // 2. Scatter Lilac Bushes (variants 5..9)
    let bush_variants_start = 5.min(variants.len());
    let bush_variants_len = variants.len().saturating_sub(bush_variants_start);

    if bush_variants_len > 0 {
        for _ in 0..bush_count {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let lx = 1.0 + (((seed >> 32) & 0xFFFF) as f32 / 65535.0) * 30.0;

            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let lz = 1.0 + (((seed >> 32) & 0xFFFF) as f32 / 65535.0) * 30.0;

            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let ryaw = (((seed >> 32) & 0xFFFF) as f32 / 65535.0) * std::f32::consts::TAU;

            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let rscale = 0.80 + (((seed >> 32) & 0xFFFF) as f32 / 65535.0) * 0.45;

            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let variant_idx = bush_variants_start + (((seed >> 32) as usize) % bush_variants_len);

            let wx = (chunk_x as f32) * 32.0 + lx;
            let wz = (chunk_z as f32) * 32.0 + lz;
            let wy = crate::terrain::terrain_height(wx, wz) - 0.10;

            let model = Mat4::from_scale_rotation_translation(
                Vec3::splat(rscale),
                Quat::from_rotation_y(ryaw),
                Vec3::new(wx, wy, wz),
            );

            // Continuous world-space chlorophyll hue & bark darkness variation per bush
            let wave_r = (wx * 0.052 - wz * 0.041).cos() * 0.08;
            let wave_g = (wx * 0.044 + wz * 0.058).sin() * 0.09;
            let rand_tint = (((seed >> 16) & 0xFFFF) as f32 / 65535.0 - 0.5) * 0.12;
            let color_tint = [
                (0.90 + wave_r + rand_tint).clamp(0.74, 1.10),
                (0.95 + wave_g + rand_tint * 0.8).clamp(0.78, 1.12),
                (0.89 - wave_r * 0.4 + rand_tint * 0.5).clamp(0.72, 1.08),
                1.0,
            ];
            if variant_idx < instances_by_variant.len()
                && instances_by_variant[variant_idx].len() < ForestPipeline::MAX_INSTANCES_PER_VARIANT
            {
                instances_by_variant[variant_idx].push(ForestInstanceRaw::new(model, color_tint));
            }

            // Register bush collider
            let radius = variants[variant_idx].collider_radius * rscale;
            colliders_out.push(TreeCollider {
                center: Vec2::new(wx, wz),
                radius,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera3d::Camera3D;

    fn mock_variants() -> Vec<ForestVariant> {
        let mut variants = Vec::new();
        for i in 0..9 {
            let bark_range = MeshRange {
                vertex_offset: 0,
                vertex_count: 10,
                index_offset: 0,
                index_count: 30,
            };
            let leaf_range = MeshRange {
                vertex_offset: 10,
                vertex_count: 20,
                index_offset: 30,
                index_count: 60,
            };
            variants.push(ForestVariant {
                name: format!("variant_{}", i),
                kind: if i < 5 { 0 } else { 1 }, // 0..5 = Oak, 5..9 = Bush
                collider_radius: if i < 5 { 0.45 } else { 0.30 },
                bound_y_offset: if i == 1 {
                    13.2
                } else if i < 5 {
                    8.0
                } else {
                    2.5
                },
                bound_radius: if i == 1 {
                    16.0
                } else if i < 5 {
                    10.5
                } else {
                    3.5
                },
                bark_y_offset: if i == 1 { 10.9 } else { 6.5 },
                bark_radius: if i == 1 { 13.5 } else { 8.5 },
                bark_range,
                bark_lods: [bark_range, bark_range, bark_range],
                leaf_range,
                leaf_lods: [leaf_range, leaf_range, leaf_range],
            });
        }
        variants
    }

    #[test]
    fn test_forest_camera_and_shadow_culling() {
        let variants = mock_variants();
        let mut chunks = Vec::new();
        for cz in -5..=5 {
            for cx in -5..=5 {
                chunks.push(ForestChunk::generate(cx, cz, &variants));
            }
        }

        let total_all: usize = chunks
            .iter()
            .flat_map(|c| c.instances_by_variant.iter())
            .map(|v| v.len())
            .sum();
        assert!(total_all > 500);

        // 1. Camera frustum culling test
        let cam_pos = Vec3::new(0.0, 2.5, 0.0);
        let mut cam = Camera3D::new(cam_pos, Vec3::new(0.0, 2.5, -1.0), 16.0 / 9.0);
        cam.fov_y_rad = 70.0_f32.to_radians();
        let frustum = cam.frustum();

        let mut camera_culled: Vec<Vec<ForestInstanceRaw>> =
            (0..variants.len()).map(|_| Vec::new()).collect();
        cull_forest_chunks_camera(&chunks, &frustum, cam_pos, 160.0, &variants, &mut camera_culled);

        let total_cam_culled: usize = camera_culled.iter().map(|v| v.len()).sum();
        assert!(total_cam_culled > 0);
        assert!(total_cam_culled < total_all / 2);

        // Verify front-to-back ordering and compute_forest_lod_ends
        let mut lod_ends = vec![[0u32; 3]; variants.len()];
        compute_forest_lod_ends(&camera_culled, &variants, cam_pos, &mut lod_ends);
        for (v_idx, list) in camera_culled.iter().enumerate() {
            for pair in list.windows(2) {
                let d0 = (pair[0].model_3[0] - cam_pos.x).powi(2) + (pair[0].model_3[2] - cam_pos.z).powi(2);
                let d1 = (pair[1].model_3[0] - cam_pos.x).powi(2) + (pair[1].model_3[2] - cam_pos.z).powi(2);
                assert!(d0 <= d1 + 1e-3, "Forest instances must be sorted front-to-back");
            }
            let ends = lod_ends[v_idx];
            assert!(ends[0] <= ends[1] && ends[1] <= ends[2]);
            assert_eq!(ends[2] as usize, list.len());
        }

        // Verify tall Large Oak canopy (33m height at 1.25x scale) is NOT prematurely culled when camera looks slightly below its base
        let tall_oak_chunk = ForestChunk {
            chunk_x: 0,
            chunk_z: -1,
            min_x: 0.0,
            min_z: -32.0,
            instances_by_variant: {
                let mut by_var = (0..9).map(|_| Vec::new()).collect::<Vec<_>>();
                let model = Mat4::from_scale_rotation_translation(
                    Vec3::splat(1.25),
                    Quat::IDENTITY,
                    Vec3::new(0.0, 0.0, -20.0),
                );
                by_var[1].push(ForestInstanceRaw::new(model, [1.0; 4]));
                by_var
            },
            colliders: Vec::new(),
        };
        let mut low_cam = Camera3D::new(
            Vec3::new(0.0, 25.0, 0.0),
            Vec3::new(0.0, 28.0, -20.0),
            16.0 / 9.0,
        );
        low_cam.fov_y_rad = 70.0_f32.to_radians();
        let mut tall_out: Vec<Vec<ForestInstanceRaw>> = (0..9).map(|_| Vec::new()).collect();
        cull_forest_chunks_camera(
            &[tall_oak_chunk],
            &low_cam.frustum(),
            Vec3::new(0.0, 25.0, 0.0),
            160.0,
            &variants,
            &mut tall_out,
        );
        assert_eq!(tall_out[1].len(), 1, "Tall scaled Large Oak canopy must not be prematurely culled");

        // 2. Shadow pass light-space culling test (must exclude all bushes kind==1 and far chunks)
        let sun_dir = Vec3::new(0.5, 0.9, 0.35).normalize();
        let light_view = Mat4::look_at_rh(cam_pos + sun_dir * 80.0, cam_pos, Vec3::Y);
        let light_proj = Mat4::orthographic_rh(-65.0, 65.0, -65.0, 65.0, 0.1, 200.0);
        let light_view_proj = light_proj * light_view;

        let mut shadow_culled: Vec<Vec<ForestInstanceRaw>> =
            (0..variants.len()).map(|_| Vec::new()).collect();
        cull_forest_chunks_shadow(&chunks, light_view_proj, &variants, &mut shadow_culled);

        let oak_shadow_count: usize = shadow_culled[0..5].iter().map(|v| v.len()).sum();
        let bush_shadow_count: usize = shadow_culled[5..9].iter().map(|v| v.len()).sum();
        assert!(oak_shadow_count > 0);
        assert_eq!(bush_shadow_count, 0, "Lilac bushes must be excluded from shadow pass");

        // 3. Verify organic world-space color tint variation on scattered oak trees & bushes
        let first_oak = &camera_culled[0][0];
        let second_oak = &camera_culled[0][1];
        assert_ne!(
            first_oak.color_tint,
            [1.0, 1.0, 1.0, 1.0],
            "Scattered oak trees must have organic world-space color variation"
        );
        assert_ne!(
            first_oak.color_tint, second_oak.color_tint,
            "Distinct trees must have distinct world-space botanical tints"
        );
    }

    #[test]
    fn test_forest_lod_index_decimation() {
        // Construct 32 synthetic 2-triangle leaf cards in a ring around canopy center
        let mut verts = Vec::new();
        let mut leaf_indices = Vec::new();
        for card in 0..32u16 {
            let angle = (card as f32) * std::f32::consts::TAU / 32.0;
            let cx = angle.cos() * 4.0;
            let cz = angle.sin() * 4.0;
            let cy = 6.0 + (card as f32) * 0.2;
            let base = verts.len() as u16;
            verts.push(ForestVertex { position: [cx, cy, cz], normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0] });
            verts.push(ForestVertex { position: [cx + 0.5, cy, cz], normal: [0.0, 1.0, 0.0], uv: [1.0, 0.0] });
            verts.push(ForestVertex { position: [cx + 0.5, cy + 0.5, cz], normal: [0.0, 1.0, 0.0], uv: [1.0, 1.0] });
            verts.push(ForestVertex { position: [cx, cy + 0.5, cz], normal: [0.0, 1.0, 0.0], uv: [0.0, 1.0] });
            leaf_indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }

        let lod1 = build_leaf_lod_indices(&verts, &leaf_indices, 32);
        let lod2 = build_leaf_lod_indices(&verts, &leaf_indices, 16);
        assert_eq!(lod1.len() % 6, 0, "Leaf LOD1 must preserve whole 2-triangle cards");
        assert_eq!(lod2.len() % 6, 0, "Leaf LOD2 must preserve whole 2-triangle cards");
        assert!(lod1.len() < leaf_indices.len() && lod2.len() < lod1.len());

        let bark_lod1 = build_bark_lod_indices(&verts, &leaf_indices, 28);
        let bark_lod2 = build_bark_lod_indices(&verts, &leaf_indices, 14);
        assert_eq!(bark_lod1.len() % 3, 0);
        assert_eq!(bark_lod2.len() % 3, 0);
        assert!(bark_lod1.len() < leaf_indices.len() && bark_lod2.len() < bark_lod1.len());
    }
}


