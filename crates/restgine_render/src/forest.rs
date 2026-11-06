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
    pub bark_range: MeshRange,
    pub leaf_range: MeshRange,
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

@fragment
fn fs_bark(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_bark_albedo, s_bark, in.uv);
    let raw_normal = textureSample(t_bark_normal, s_bark, in.uv).xyz * 2.0 - 1.0;

    let v_up = vec3<f32>(0.0, 1.0, 0.0);
    var t_vec = cross(in.world_normal, v_up);
    if (length(t_vec) < 0.01) { t_vec = vec3<f32>(1.0, 0.0, 0.0); } else { t_vec = normalize(t_vec); }
    let b_vec = cross(in.world_normal, t_vec);
    let tbn = mat3x3<f32>(t_vec, b_vec, in.world_normal);
    let normal = normalize(tbn * (raw_normal * vec3<f32>(1.6, 1.6, 1.0)));

    let albedo = tex_color.rgb * in.color.rgb;
    let light_dir = normalize(camera.light_dir.xyz);
    let view_dir = normalize(camera.camera_pos.xyz - in.world_pos);

    // Directional Shadow PCF sampling
    var shadow: f32 = 1.0;
    let shadow_tier = camera.quality_tiers.x;
    if (shadow_tier > 0u && in.shadow_pos.x >= 0.0 && in.shadow_pos.x <= 1.0 && in.shadow_pos.y >= 0.0 && in.shadow_pos.y <= 1.0 && in.shadow_pos.z >= 0.0 && in.shadow_pos.z <= 1.0) {
        let texel_size = 1.0 / 2048.0;
        let bias = 0.003;
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

    // Ambient + Moonlight Diffuse with Shadow
    let ambient = vec3<f32>(0.15, 0.18, 0.25) * 0.24;
    let diff = max(dot(normal, light_dir), 0.0);
    let sun_color = vec3<f32>(0.85, 0.92, 1.0); // Cold moonlight
    var lit_color = (ambient + diff * sun_color * shadow) * albedo;

    // Wet Bark Specular Sheen (Rain / Dew)
    let half_dir = normalize(light_dir + view_dir);
    let ndoth = max(dot(normal, half_dir), 0.0);
    let bark_spec = pow(ndoth, 32.0) * 0.22 * shadow;
    lit_color += bark_spec * sun_color;

    // Flashlight Spot
    if (camera.flashlight_pos.w > 0.5) {
        let to_light = camera.flashlight_pos.xyz - in.world_pos;
        let dist = length(to_light);
        let l_dir = to_light / max(dist, 0.001);

        let spot_dot = dot(-l_dir, camera.flashlight_dir.xyz);
        let cos_outer = 0.86;
        let cos_inner = 0.94;
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
            let flash_spec = pow(flash_ndoth, 36.0) * 0.35;

            lit_color += (flash_diff * albedo + flash_spec * flash_color) * flash_color * (smooth_spot * atten * dist_cutoff * flash_intensity);
        }
    }

    // Atmospheric Midnight Horizon Fog
    let dist = length(camera.camera_pos.xyz - in.world_pos);
    let view_dist = camera.settings.w;
    let fog_start = view_dist * 0.40;
    let fog_end = view_dist * 0.88;
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
    if (tex_color.a < 0.32) {
        discard;
    }

    let raw_normal = textureSample(t_leaf_normal, s_leaf, in.uv).xyz * 2.0 - 1.0;
    let v_up = vec3<f32>(0.0, 1.0, 0.0);
    var t_vec = cross(in.world_normal, v_up);
    if (length(t_vec) < 0.01) { t_vec = vec3<f32>(1.0, 0.0, 0.0); } else { t_vec = normalize(t_vec); }
    let b_vec = cross(in.world_normal, t_vec);
    let tbn = mat3x3<f32>(t_vec, b_vec, in.world_normal);
    let normal_leaf = normalize(tbn * (raw_normal * vec3<f32>(1.5, 1.5, 1.0)));
    let normal_dir = select(-normal_leaf, normal_leaf, is_front);

    let albedo = tex_color.rgb * in.color.rgb;
    let light_dir = normalize(camera.light_dir.xyz);
    let view_dir = normalize(camera.camera_pos.xyz - in.world_pos);

    // Directional Shadow PCF sampling
    var shadow: f32 = 1.0;
    let shadow_tier = camera.quality_tiers.x;
    if (shadow_tier > 0u && in.shadow_pos.x >= 0.0 && in.shadow_pos.x <= 1.0 && in.shadow_pos.y >= 0.0 && in.shadow_pos.y <= 1.0 && in.shadow_pos.z >= 0.0 && in.shadow_pos.z <= 1.0) {
        let texel_size = 1.0 / 2048.0;
        let bias = 0.003;
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

    // Leaf Ambient + Double-Sided Sun Diffuse + Subsurface Scattering
    let ambient = vec3<f32>(0.14, 0.20, 0.26) * 0.28;
    let diff = max(dot(normal_dir, light_dir) * 0.65 + 0.35, 0.0);
    let sun_color = vec3<f32>(0.85, 0.92, 1.0);

    // Subsurface Scattering (Backlight leaf glow)
    let sun_back = max(dot(view_dir, -light_dir), 0.0);
    let sun_sss = pow(sun_back, 3.0) * 0.45;
    let sss_tint = vec3<f32>(0.35, 0.65, 0.15);

    var lit_color = (ambient + (diff + sun_sss * sss_tint) * sun_color * shadow) * albedo;

    // Wet Leaf Specular Sheen from Moon
    let half_moon = normalize(light_dir + view_dir);
    let moon_ndoth = max(dot(normal_dir, half_moon), 0.0);
    let moon_spec = pow(moon_ndoth, 24.0) * 0.20 * shadow;
    lit_color += moon_spec * sun_color;

    // Realistic Flashlight (with Subsurface Translucency & Waxy Sheen)
    if (camera.flashlight_pos.w > 0.5) {
        let to_light = camera.flashlight_pos.xyz - in.world_pos;
        let dist = length(to_light);
        let l_dir = to_light / max(dist, 0.001);

        let spot_dot = dot(-l_dir, camera.flashlight_dir.xyz);
        let cos_outer = 0.86;
        let cos_inner = 0.94;
        let spot_factor = clamp((spot_dot - cos_outer) / (cos_inner - cos_outer), 0.0, 1.0);
        let smooth_spot = spot_factor * spot_factor * (3.0 - 2.0 * spot_factor);

        if (smooth_spot > 0.0) {
            let atten = 1.0 / (1.0 + 0.06 * dist + 0.015 * dist * dist);
            let dist_cutoff = clamp((55.0 - dist) / 12.0, 0.0, 1.0);
            let flash_color = vec3<f32>(1.0, 0.97, 0.88);
            let flash_intensity = camera.flashlight_dir.w * 4.2;

            let flash_diff = max(dot(normal_dir, l_dir), 0.0);
            let flash_back = max(dot(view_dir, -l_dir), 0.0);
            let flash_sss = pow(flash_back, 2.5) * 0.55;

            let half_flash = normalize(l_dir + view_dir);
            let ndoth = max(dot(normal_dir, half_flash), 0.0);
            let leaf_spec = pow(ndoth, 36.0) * 0.35;

            let total_flash = (flash_diff * albedo + flash_sss * albedo * sss_tint) + leaf_spec * flash_color;
            lit_color += total_flash * flash_color * (smooth_spot * atten * dist_cutoff * flash_intensity);
        }
    }

    // Atmospheric Midnight Horizon Fog
    let dist = length(camera.camera_pos.xyz - in.world_pos);
    let view_dist = camera.settings.w;
    let fog_start = view_dist * 0.40;
    let fog_end = view_dist * 0.88;
    let fog_factor = clamp((dist - fog_start) / (fog_end - fog_start + 0.001), 0.0, 1.0);
    let smooth_fog = fog_factor * fog_factor * (3.0 - 2.0 * fog_factor);
    let horizon_fog_color = vec3<f32>(0.04, 0.07, 0.13);
    lit_color = mix(lit_color, horizon_fog_color, smooth_fog);

    return vec4<f32>(lit_color, tex_color.a);
}
"#;

pub struct ForestPipeline {
    pub bark_pipeline: wgpu::RenderPipeline,
    pub leaf_pipeline: wgpu::RenderPipeline,
    pub shadow_pipeline: wgpu::RenderPipeline,
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub variants: Vec<ForestVariant>,
    pub oak_material_bind_group: wgpu::BindGroup,
    pub bush_material_bind_group: wgpu::BindGroup,
    pub instance_buffers: Vec<wgpu::Buffer>,
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

            // Bark Geometry
            let bark_v_offset = all_vertices.len() as u32;
            let bark_i_offset = all_indices.len() as u32;
            let mut bark_raw_verts = vec![0u8; bark_vcount * 32];
            file.read_exact(&mut bark_raw_verts)?;
            let bark_v_slice: &[ForestVertex] = bytemuck::cast_slice(&bark_raw_verts);
            all_vertices.extend_from_slice(bark_v_slice);

            let mut bark_raw_indices = vec![0u8; bark_icount * 2];
            file.read_exact(&mut bark_raw_indices)?;
            let bark_i_slice: &[u16] = bytemuck::cast_slice(&bark_raw_indices);
            all_indices.extend_from_slice(bark_i_slice);

            // Leaf Geometry
            let leaf_v_offset = all_vertices.len() as u32;
            let leaf_i_offset = all_indices.len() as u32;
            let mut leaf_raw_verts = vec![0u8; leaf_vcount * 32];
            file.read_exact(&mut leaf_raw_verts)?;
            let leaf_v_slice: &[ForestVertex] = bytemuck::cast_slice(&leaf_raw_verts);
            all_vertices.extend_from_slice(leaf_v_slice);

            let mut leaf_raw_indices = vec![0u8; leaf_icount * 2];
            file.read_exact(&mut leaf_raw_indices)?;
            let leaf_i_slice: &[u16] = bytemuck::cast_slice(&leaf_raw_indices);
            all_indices.extend_from_slice(leaf_i_slice);

            variants.push(ForestVariant {
                name,
                kind,
                collider_radius,
                bark_range: MeshRange {
                    vertex_offset: bark_v_offset,
                    vertex_count: bark_vcount as u32,
                    index_offset: bark_i_offset,
                    index_count: bark_icount as u32,
                },
                leaf_range: MeshRange {
                    vertex_offset: leaf_v_offset,
                    vertex_count: leaf_vcount as u32,
                    index_offset: leaf_i_offset,
                    index_count: leaf_icount as u32,
                },
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
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Forest Linear Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 16,
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
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&linear_sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&oak_bark_norm_v) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&oak_leaf_v) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&linear_sampler) },
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
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&linear_sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&bush_bark_norm_v) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&bush_leaf_v) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&linear_sampler) },
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
                cull_mode: None, // Shadow from all faces
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

        // 4. Per-variant instance buffers
        let mut instance_buffers = Vec::with_capacity(variants.len());
        for _ in 0..variants.len() {
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Forest Variant Instance Buffer"),
                size: (Self::MAX_INSTANCES_PER_VARIANT * std::mem::size_of::<ForestInstanceRaw>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            instance_buffers.push(buf);
        }

        Ok(Self {
            bark_pipeline,
            leaf_pipeline,
            shadow_pipeline,
            vertex_buffer,
            index_buffer,
            variants,
            oak_material_bind_group,
            bush_material_bind_group,
            instance_buffers,
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

    pub fn render_shadow<'a>(
        &'a self,
        rpass: &mut wgpu::RenderPass<'a>,
        shadow_camera_bind_group: &'a wgpu::BindGroup,
        variant_counts: &[usize],
    ) {
        rpass.set_pipeline(&self.shadow_pipeline);
        rpass.set_bind_group(0, shadow_camera_bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        for (variant_idx, variant) in self.variants.iter().enumerate() {
            let count = if variant_idx < variant_counts.len() {
                variant_counts[variant_idx].min(self.max_instances_per_variant) as u32
            } else {
                0
            };
            if count == 0 {
                continue;
            }

            rpass.set_vertex_buffer(1, self.instance_buffers[variant_idx].slice(..));

            if variant.bark_range.index_count > 0 {
                rpass.draw_indexed(
                    variant.bark_range.index_offset..(variant.bark_range.index_offset + variant.bark_range.index_count),
                    variant.bark_range.vertex_offset as i32,
                    0..count,
                );
            }

            if variant.leaf_range.index_count > 0 {
                rpass.draw_indexed(
                    variant.leaf_range.index_offset..(variant.leaf_range.index_offset + variant.leaf_range.index_count),
                    variant.leaf_range.vertex_offset as i32,
                    0..count,
                );
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

        let color_tint = [1.0, 1.0, 1.0, 1.0];
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

            let color_tint = [1.0, 1.0, 1.0, 1.0];
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
