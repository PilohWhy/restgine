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
@group(0) @binding(5)
var t_shadow: texture_depth_2d;
@group(0) @binding(6)
var s_shadow: sampler_comparison;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) screen_pos: vec2<f32>,
    @location(2) near_ray: vec3<f32>,
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

    // Precompute exact world-space near-plane vector at the 3 fullscreen vertices.
    // Because z_ndc = 1.0 lies on the flat camera near plane (constant view Z = z_near),
    // hardware linear interpolation across the fullscreen triangle is mathematically exact.
    let near_h = camera.inv_view_proj * vec4<f32>(x, y, 1.0, 1.0);
    out.near_ray = near_h.xyz / max(near_h.w, 0.0001) - camera.camera_pos.xyz;
    return out;
}

// Jimenez Interleaved Gradient Noise (IGN)
fn ign(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
}

fn hash2d(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

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

fn extract_bloom(c: vec3<f32>, thresh: f32, knee: f32) -> vec3<f32> {
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    let soft = clamp((luma - thresh + knee) / (2.0 * knee + 0.0001), 0.0, 1.0);
    let w = max(luma - thresh, 0.0) + knee * soft * soft;
    return c * (w / max(luma, 0.001));
}

fn get_ssao_offset(idx: u32) -> vec2<f32> {
    switch (idx) {
        case 0u: { return vec2<f32>(-0.326, -0.406); }
        case 1u: { return vec2<f32>(-0.840, -0.074); }
        case 2u: { return vec2<f32>(-0.696,  0.457); }
        case 3u: { return vec2<f32>(-0.203,  0.621); }
        case 4u: { return vec2<f32>( 0.962, -0.195); }
        case 5u: { return vec2<f32>( 0.473, -0.480); }
        case 6u: { return vec2<f32>( 0.519,  0.767); }
        case 7u: { return vec2<f32>( 0.185, -0.893); }
        case 8u: { return vec2<f32>( 0.507,  0.064); }
        case 9u: { return vec2<f32>( 0.896,  0.412); }
        case 10u: { return vec2<f32>(-0.322, -0.933); }
        default: { return vec2<f32>(-0.792, -0.598); }
    }
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let post_tier = camera.quality_tiers.w;
    let screen_ign = ign(in.position.xy);
    let raw_depth = textureSample(t_depth, s_nearest, in.uv);
    let is_sky = raw_depth <= 0.00002;

    // Unproject world-space ray & metric distance
    let near_len_sq = dot(in.near_ray, in.near_ray);
    let inv_near_len = inverseSqrt(max(near_len_sq, 0.000001));
    let near_len = near_len_sq * inv_near_len;
    let ray_dir = in.near_ray * inv_near_len;
    let safe_depth = max(raw_depth, 0.00005);
    let linear_dist = near_len / safe_depth;
    let total_dist = select(linear_dist, camera.settings.w, is_sky);

    // ------------------------------------------------------------------------
    // 1. BASE RADIANCE & OPTICAL CHROMATIC DISPERSION
    // ------------------------------------------------------------------------
    let ca_dir = in.uv - vec2<f32>(0.5, 0.5);
    let ca_dist = dot(ca_dir, ca_dir);
    let ca_strength = select(0.0, select(0.0035, 0.0065, post_tier >= 3u), post_tier >= 2u);
    let ca_offset = ca_dir * (ca_dist * ca_strength);

    let col_r = textureSample(t_scene, s_linear, in.uv + ca_offset).r;
    let col_g = textureSample(t_scene, s_linear, in.uv).g;
    let col_b = textureSample(t_scene, s_linear, in.uv - ca_offset).b;
    var scene_color = vec3<f32>(col_r, col_g, col_b);

    // ------------------------------------------------------------------------
    // 2. ULTRA HIGH-FIDELITY SCREEN-SPACE AMBIENT OCCLUSION (SSAO)
    //    Aspect-Ratio Corrected, Normal-Oriented Hemisphere Occlusion
    //    (Zero False Darkening on Flat Terrain, Deep Contact Shadows in Crevices & Roots)
    // ------------------------------------------------------------------------
    let gi_tier = camera.quality_tiers.y;
    if (gi_tier >= 1u && !is_sky && total_dist < 36.0) {
        let depth_dims = vec2<f32>(textureDimensions(t_depth));
        let aspect = max(depth_dims.x / max(depth_dims.y, 1.0), 0.1);

        let p_curr = camera.camera_pos.xyz + ray_dir * total_dist;
        let dp_dx = dpdx(p_curr);
        let dp_dy = dpdy(p_curr);
        var surf_normal = normalize(cross(dp_dx, dp_dy));
        if (dot(surf_normal, -ray_dir) < 0.0) {
            surf_normal = -surf_normal;
        }

        let angle = screen_ign * 6.2831853;
        let c = cos(angle);
        let s = sin(angle);
        let rot = mat2x2<f32>(vec2<f32>(c, s), vec2<f32>(-s, c));

        let base_rad = select(0.014, select(0.020, 0.028, gi_tier >= 3u), gi_tier >= 2u);
        let persp_scale = clamp(6.2 / max(total_dist, 1.2), 0.12, 1.4);
        let rad = base_rad * persp_scale;
        let rad_vec = vec2<f32>(rad / aspect, rad);

        var occ: f32 = 0.0;
        let r_max = select(0.65, 0.95, gi_tier >= 2u);

        for (var i = 0u; i < 12u; i = i + 1u) {
            let offset_uv = in.uv + (rot * get_ssao_offset(i)) * rad_vec;
            let s_raw = textureSample(t_depth, s_nearest, offset_uv);
            let s_dist = near_len / max(s_raw, 0.00005);
            let p_sample = camera.camera_pos.xyz + ray_dir * s_dist;
            let v = p_sample - p_curr;
            let d_v = length(v);
            if (d_v > 0.015 && d_v < r_max) {
                let ndotv = dot(surf_normal, v / d_v);
                if (ndotv > 0.10) {
                    occ += (ndotv - 0.10) * (1.0 - d_v / r_max);
                }
            }
        }

        let dist_fade = clamp((36.0 - total_dist) * 0.1, 0.0, 1.0);
        let ao_strength = select(1.10, select(1.45, 1.85, gi_tier >= 3u), gi_tier >= 2u) * dist_fade;
        let ao_factor = clamp(1.0 - (occ * (1.0 / 12.0)) * ao_strength, 0.18, 1.0);
        scene_color *= ao_factor;
    }

    // ------------------------------------------------------------------------
    // 3. CINEMATIC ATMOSPHERIC VOLUMETRIC GOD RAYS & MULTI-OCTAVE SWIRLING MIST
    // ------------------------------------------------------------------------
    let effects_tier = camera.quality_extra.x;
    if (effects_tier > 0u) {
        let max_march_dist = min(total_dist, camera.settings.w * 0.92);
        let num_steps = select(10u, select(14u, 18u, effects_tier >= 3u), effects_tier >= 2u);

        let moon_dir = normalize(camera.light_dir.xyz);
        let cos_moon = dot(ray_dir, moon_dir);

        // Double-lobe Mie forward scattering with wide-angle presence
        let g1 = select(0.74, 0.79, effects_tier >= 2u);
        let g2 = 0.35;
        let mie1 = (1.0 - g1 * g1) / (12.56637 * pow(max(1.0 + g1 * g1 - 2.0 * g1 * cos_moon, 0.02), 1.5));
        let mie2 = (1.0 - g2 * g2) / (12.56637 * pow(max(1.0 + g2 * g2 - 2.0 * g2 * cos_moon, 0.05), 1.5));
        let phase_moon = mie1 * 0.65 + mie2 * 0.35;

        let moon_scatter_color = vec3<f32>(0.84, 0.92, 1.0);
        let mist_ambient_color = vec3<f32>(0.038, 0.072, 0.14);
        let time = camera.settings.y;

        var ray_transmittance: f32 = 1.0;
        var god_ray_accum = vec3<f32>(0.0);

        for (var step = 0u; step < num_steps; step = step + 1u) {
            // Non-linear quadratic distance march concentrates samples in near-field forest
            let u_step = (f32(step) + screen_ign) / f32(num_steps);
            let t_march = max_march_dist * (u_step * u_step);
            let step_len = 2.0 * max_march_dist * max(u_step, 0.06) / f32(num_steps);
            let p = camera.camera_pos.xyz + ray_dir * t_march;

            // Height falloff + 2-octave animated turbulent ground mist
            let h_ground = exp(-0.30 * max(p.y - 0.2, -1.0));
            let mist_uv1 = p.xz * 0.048 + vec2<f32>(time * 0.065, time * 0.038);
            let n1 = noise2d(mist_uv1);
            let mist_uv2 = p.xz * 0.115 - vec2<f32>(time * 0.085, n1 * 0.35);
            let n2 = noise2d(mist_uv2);
            let swirl = smoothstep(0.22, 0.78, n1 * 0.62 + n2 * 0.38);
            let ground_mist = h_ground * (0.32 + 1.35 * swirl);

            // Forest canopy haze layer enables beams cutting between tree crowns
            let canopy_haze = clamp((22.0 - p.y) * 0.045, 0.0, 1.0) * 0.42;
            let mist_density = (ground_mist + canopy_haze) * select(0.020, select(0.032, 0.046, effects_tier >= 3u), effects_tier >= 2u);

            // Project step sample into shadow map to detect canopy gaps
            let s_clip = camera.light_view_proj * vec4<f32>(p, 1.0);
            let s_uv = vec2<f32>(s_clip.x * 0.5 + 0.5, -s_clip.y * 0.5 + 0.5);
            var in_light: f32 = 1.0;
            if (s_uv.x >= 0.0 && s_uv.x <= 1.0 && s_uv.y >= 0.0 && s_uv.y <= 1.0 && s_clip.z >= 0.0 && s_clip.z <= 1.0) {
                in_light = textureSampleCompareLevel(t_shadow, s_shadow, s_uv, s_clip.z - 0.0030);
            }

            let step_opt_depth = mist_density * step_len;
            let step_transmittance = exp(-step_opt_depth);
            let in_scatter = (in_light * moon_scatter_color * (phase_moon * 6.5) + mist_ambient_color) * step_opt_depth;

            god_ray_accum += in_scatter * ray_transmittance;
            ray_transmittance *= step_transmittance;
        }

        scene_color = scene_color * ray_transmittance + god_ray_accum;

        // --------------------------------------------------------------------
        // 3B. VOLUMETRIC FLASHLIGHT BEAM WITH GLOWING FOREST SPORES
        // --------------------------------------------------------------------
        if (camera.flashlight_pos.w > 0.5) {
            let f_pos = camera.flashlight_pos.xyz;
            let f_dir = normalize(camera.flashlight_dir.xyz);
            let v = camera.camera_pos.xyz - f_pos;

            let cos_cone = 0.89;
            let c2 = cos_cone * cos_cone;
            let da = dot(ray_dir, f_dir);
            let va = dot(v, f_dir);
            let dv = dot(ray_dir, v);
            let vv = dot(v, v);

            let qa = da * da - c2;
            let qb = da * va - c2 * dv;
            let qc = va * va - c2 * vv;
            let disc = qb * qb - qa * qc;

            var t_start: f32 = 0.18;
            var t_end: f32 = 0.0;
            let max_beam_dist = min(total_dist, 44.0);

            if (disc >= 0.0) {
                let sqrt_d = sqrt(disc);
                let safe_qa = select(max(qa, 0.00001), min(qa, -0.00001), qa < 0.0);
                let inv_qa = 1.0 / safe_qa;
                let r0 = (-qb - sqrt_d) * inv_qa;
                let r1 = (-qb + sqrt_d) * inv_qa;
                let t0 = min(r0, r1);
                let t1 = max(r0, r1);

                if (qa < 0.0) {
                    let s0 = max(t0, 0.18);
                    let s1 = min(t1, max_beam_dist);
                    let t_mid = 0.5 * (s0 + s1);
                    if (s1 > s0 && (va + t_mid * da) > 0.0) {
                        t_start = s0;
                        t_end = s1;
                    }
                } else if (da > 0.0) {
                    let s0 = max(max(t1, -va / max(da, 0.001)), 0.18);
                    if (max_beam_dist > s0) {
                        t_start = s0;
                        t_end = max_beam_dist;
                    }
                }
            } else if (qa > 0.0 && da > 0.0) {
                t_start = 0.18;
                t_end = max_beam_dist;
            }

            if (t_end > t_start) {
                let span = min(t_end - t_start, 32.0);
                let f_steps = select(7u, 11u, effects_tier >= 2u);
                let f_step_len = span / f32(f_steps);
                var f_t = t_start + f_step_len * screen_ign;
                var beam_accum = vec3<f32>(0.0);

                let flash_color = vec3<f32>(1.0, 0.97, 0.88);

                for (var s = 0u; s < f_steps; s = s + 1u) {
                    let p = camera.camera_pos.xyz + ray_dir * f_t;
                    let to_p = p - f_pos;
                    let l_dist_sq = dot(to_p, to_p);
                    let inv_l_dist = inverseSqrt(max(l_dist_sq, 0.000001));
                    let l_dist = l_dist_sq * inv_l_dist;
                    let l_dir = to_p * inv_l_dist;
                    let cos_ang = dot(l_dir, f_dir);

                    let core = smoothstep(0.97, 0.994, cos_ang);
                    let flood = smoothstep(0.91, 0.97, cos_ang);
                    let profile = core * 0.60 + flood * 0.12;

                    let atten = 1.0 / (1.0 + 0.18 * l_dist + 0.05 * l_dist_sq);
                    let dist_cutoff = clamp((38.0 - l_dist) / 8.0, 0.0, 1.0);
                    let cos_view = dot(-ray_dir, l_dir);

                    // Dual-lobe aerosol scattering with forward peak and subtle backscatter
                    let p_fwd = (1.0 - 0.40 * 0.40) / (12.56637 * pow(max(1.0 + 0.40 * 0.40 - 0.80 * cos_view, 0.05), 1.5));
                    let p_back = pow(max(-cos_view, 0.0), 2.5) * 0.20;
                    let phase_flash = 0.05 + p_fwd * 0.15 + p_back * 0.10;

                    // Glowing floating forest spores / organic dust motes
                    var spore_glow: f32 = 0.0;
                    if (l_dist < 18.0) {
                        let drift = vec3<f32>(time * 0.32, sin(time * 0.8 + p.x) * 0.22, time * 0.22);
                        let sp = (p + drift) * 3.6;
                        let cell_id = floor(sp);
                        let p3 = fract(cell_id * vec3<f32>(0.1031, 0.1030, 0.0973));
                        let cell_hash = fract((p3.x + p3.y) * (dot(p3, p3.yzx + 33.33)));
                        if (cell_hash > 0.88) {
                            let center_offset = vec3<f32>(
                                fract(cell_hash * 13.7),
                                fract(cell_hash * 29.3),
                                fract(cell_hash * 47.1)
                            ) * 0.6 + 0.2;
                            let local_d = fract(sp) - center_offset;
                            let r2 = dot(local_d, local_d);
                            let sparkle = 0.5 + 0.5 * sin(time * 4.2 + cell_hash * 37.0);
                            spore_glow = exp(-r2 * 24.0) * sparkle * clamp((18.0 - l_dist) * 0.1, 0.0, 1.0) * 0.75;
                        }
                    }

                    let density = clamp(1.0 - max(p.y, 0.0) * 0.15, 0.15, 1.0);
                    let step_light = profile * atten * dist_cutoff * (phase_flash * density * 0.010 + spore_glow * 0.003) * f_step_len;
                    beam_accum += flash_color * step_light;
                    f_t += f_step_len;
                }

                beam_accum = min(beam_accum, vec3<f32>(0.035));
                scene_color += beam_accum * camera.flashlight_dir.w;
            }
        }
    }

    // ------------------------------------------------------------------------
    // 4. BALANCED MULTI-TAP CINEMATIC BLOOM (Natural Light Bleed without Glare)
    // ------------------------------------------------------------------------
    if (post_tier >= 1u) {
        let thresh = select(2.5, select(2.0, 1.6, post_tier >= 3u), post_tier >= 2u);
        let knee = 0.35;
        let bloom_intensity = select(0.06, select(0.09, 0.12, post_tier >= 3u), post_tier >= 2u);

        // Center tap
        let c_center = extract_bloom(textureSample(t_scene, s_linear, in.uv).rgb, thresh, knee);

        // Tier 1: Inner glow ring (radius ~3-4px)
        let r1 = 0.0035;
        let i_t = extract_bloom(textureSample(t_scene, s_linear, in.uv + vec2<f32>( 0.0,  r1)).rgb, thresh, knee);
        let i_b = extract_bloom(textureSample(t_scene, s_linear, in.uv + vec2<f32>( 0.0, -r1)).rgb, thresh, knee);
        let i_l = extract_bloom(textureSample(t_scene, s_linear, in.uv + vec2<f32>(-r1,  0.0)).rgb, thresh, knee);
        let i_r = extract_bloom(textureSample(t_scene, s_linear, in.uv + vec2<f32>( r1,  0.0)).rgb, thresh, knee);

        // Tier 2: Middle halo ring (radius ~12px)
        let r2 = 0.012;
        let m_t = extract_bloom(textureSample(t_scene, s_linear, in.uv + vec2<f32>( 0.0,  r2)).rgb, thresh * 1.15, knee);
        let m_b = extract_bloom(textureSample(t_scene, s_linear, in.uv + vec2<f32>( 0.0, -r2)).rgb, thresh * 1.15, knee);
        let m_l = extract_bloom(textureSample(t_scene, s_linear, in.uv + vec2<f32>(-r2,  0.0)).rgb, thresh * 1.15, knee);
        let m_r = extract_bloom(textureSample(t_scene, s_linear, in.uv + vec2<f32>( r2,  0.0)).rgb, thresh * 1.15, knee);

        let multi_bloom = c_center * 0.45 +
            (i_t + i_b + i_l + i_r) * 0.10 +
            (m_t + m_b + m_l + m_r) * 0.038;

        let bloom_tint = select(vec3<f32>(0.84, 0.92, 1.06), vec3<f32>(1.02, 0.98, 0.92), camera.flashlight_pos.w > 0.5);
        scene_color += multi_bloom * bloom_tint * bloom_intensity;
    }

    // ------------------------------------------------------------------------
    // 5. NATURAL COS^4 OPTICAL VIGNETTE & PERIPHERY COLOR SHIFT
    // ------------------------------------------------------------------------
    if (post_tier >= 1u) {
        let v_pos = in.screen_pos * vec2<f32>(0.50, 0.62);
        let dist_center_sq = dot(v_pos, v_pos);
        let vig_power = select(0.38, select(0.62, 0.85, post_tier >= 3u), post_tier >= 2u);
        let vig = clamp(1.0 - dist_center_sq * vig_power, 0.0, 1.0);
        let vignette = vig * vig;
        let edge_tint = vec3<f32>(0.75, 0.82, 0.98);
        scene_color *= mix(edge_tint, vec3<f32>(1.0), vignette);
    }

    // ------------------------------------------------------------------------
    // 6. SCOTOPIC PURKINJE SHIFT & FINE 35MM CINEMATIC FILM GRAIN
    // ------------------------------------------------------------------------
    let luma = dot(scene_color, vec3<f32>(0.2126, 0.7152, 0.0722));
    let scotopic = exp(-luma * 6.5);
    let rod_color = vec3<f32>(0.68, 0.88, 1.25) * luma;
    scene_color = mix(scene_color, rod_color, scotopic * 0.32);

    if (post_tier >= 2u) {
        // High-frequency temporal noise with zero periodic dither lattice
        let frame_time = fract(camera.settings.y * 24.0);
        let t_seed = fract(frame_time * 0.6180339887);
        let p_a = in.position.xy + vec2<f32>(t_seed * 47.11, t_seed * 83.19);
        let p_b = in.position.xy + vec2<f32>(t_seed * 113.7, t_seed * 29.3);
        let n1 = ign(p_a);
        let n2 = ign(p_b);
        // Triangular PDF distribution (sum of two uniform noises - 1.0) for natural analog grain
        let tri_noise = n1 + n2 - 1.0;

        let grain_scale = 4.0 * clamp(luma, 0.0, 1.0) * (1.0 - clamp(luma, 0.0, 1.0));
        let grain_strength = select(0.006, 0.010, post_tier >= 3u);
        let grain = tri_noise * grain_scale * grain_strength;
        scene_color = max(scene_color + vec3<f32>(grain), vec3<f32>(0.0));
    }

    // ------------------------------------------------------------------------
    // 7. FILM-GRADE ACEScc TONEMAPPING & GAMMA CORRECTION
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
                // Binding 0: CameraUniform (VERTEX | FRAGMENT so vs_main precomputes near_ray)
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
                // Binding 5: Directional Shadow Map Texture
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Binding 6: Shadow Comparison Sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
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
        shadow_view: &wgpu::TextureView,
        shadow_sampler: &wgpu::Sampler,
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
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(shadow_sampler),
                },
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    fn solve_flashlight_cone_interval(
        cam_pos: Vec3,
        ray_dir: Vec3,
        f_pos: Vec3,
        f_dir: Vec3,
        total_dist: f32,
    ) -> (f32, f32) {
        let v = cam_pos - f_pos;
        let cos_cone = 0.89_f32;
        let c2 = cos_cone * cos_cone;
        let da = ray_dir.dot(f_dir);
        let va = v.dot(f_dir);
        let dv = ray_dir.dot(v);
        let vv = v.dot(v);

        let qa = da * da - c2;
        let qb = da * va - c2 * dv;
        let qc = va * va - c2 * vv;
        let disc = qb * qb - qa * qc;

        let mut t_start = 0.18_f32;
        let mut t_end = 0.0_f32;
        let max_beam_dist = total_dist.min(44.0);

        if disc >= 0.0 {
            let sqrt_d = disc.sqrt();
            let safe_qa = if qa < 0.0 {
                qa.min(-0.00001)
            } else {
                qa.max(0.00001)
            };
            let inv_qa = 1.0 / safe_qa;
            let r0 = (-qb - sqrt_d) * inv_qa;
            let r1 = (-qb + sqrt_d) * inv_qa;
            let t0 = r0.min(r1);
            let t1 = r0.max(r1);

            if qa < 0.0 {
                let s0 = t0.max(0.18);
                let s1 = t1.min(max_beam_dist);
                let t_mid = 0.5 * (s0 + s1);
                if s1 > s0 && (va + t_mid * da) > 0.0 {
                    t_start = s0;
                    t_end = s1;
                }
            } else if da > 0.0 {
                let s0 = t1.max(-va / da.max(0.001)).max(0.18);
                if max_beam_dist > s0 {
                    t_start = s0;
                    t_end = max_beam_dist;
                }
            }
        } else if qa > 0.0 && da > 0.0 {
            t_start = 0.18;
            t_end = max_beam_dist;
        }

        (t_start, t_end)
    }

    #[test]
    fn test_ray_cone_intersection_extremes_and_grazing_boundary() {
        let cam_pos = Vec3::new(0.0, 2.75, 0.0);
        // Flashlight held right (+0.20), down (-0.16), forward (-0.12)
        let f_pos = cam_pos + Vec3::new(0.20, -0.16, -0.12);
        let f_dir = Vec3::new(0.0, 0.0, -1.0);

        // 1. Center screen ray looking straight down the beam
        let (s, e) = solve_flashlight_cone_interval(cam_pos, f_dir, f_pos, f_dir, 100.0);
        assert!(e > s && s >= 0.18 && (e - 44.0).abs() < 1e-4);

        // 2. Ray looking opposite to the flashlight (0 steps executed)
        let (s_back, e_back) =
            solve_flashlight_cone_interval(cam_pos, Vec3::new(0.0, 0.0, 1.0), f_pos, f_dir, 100.0);
        assert!(e_back <= s_back, "Opposite ray must execute 0 volumetric steps");

        // 3. Grazing cone boundary ray where qa is negative and |qa| < 1e-5
        let cos_cone = 0.89_f32;
        let grazing_da = cos_cone - 1e-6;
        let grazing_sin = (1.0 - grazing_da * grazing_da).sqrt();
        // Angle toward +X (across the offset flashlight beam)
        let grazing_ray = Vec3::new(grazing_sin, 0.0, -grazing_da).normalize();
        let (s_gr, e_gr) = solve_flashlight_cone_interval(cam_pos, grazing_ray, f_pos, f_dir, 100.0);
        assert!(
            e_gr > s_gr,
            "Grazing ray across offset cone must produce valid positive interval ({}, {})",
            s_gr,
            e_gr
        );
        let mid_p = cam_pos + grazing_ray * (0.5 * (s_gr + e_gr)) - f_pos;
        assert!(
            mid_p.normalize().dot(f_dir) >= cos_cone - 1e-3,
            "Midpoint of grazing interval must lie inside the tactical cone"
        );

        // 4. Pitch straight up (+89 deg) and straight down (-89 deg)
        for pitch_deg in [89.0_f32, -89.0_f32] {
            let pitch = pitch_deg.to_radians();
            let dir = Vec3::new(0.0, pitch.sin(), -pitch.cos()).normalize();
            let f_up = Vec3::new(dir.x, -dir.z, dir.y).normalize();
            let f_p = cam_pos + Vec3::X * 0.20 + f_up * -0.16 + dir * 0.12;
            let (sp, ep) = solve_flashlight_cone_interval(cam_pos, dir, f_p, dir, 60.0);
            assert!(
                ep > sp && sp >= 0.18 && (ep - 44.0).abs() < 1e-4,
                "Extreme pitch {} deg failed cone intersection: ({}, {})",
                pitch_deg,
                sp,
                ep
            );
        }

        // 5. Sprinting forward where camera moves ahead of lagging flashlight apex (va > 0, qc > 0)
        let lagging_f_pos = cam_pos + Vec3::new(0.10, -0.08, 0.35);
        let (s_sprint, e_sprint) =
            solve_flashlight_cone_interval(cam_pos, f_dir, lagging_f_pos, f_dir, 80.0);
        assert!(
            (s_sprint - 0.18).abs() < 1e-4 && (e_sprint - 44.0).abs() < 1e-4,
            "Camera already inside forward cone must start march at 0.18m"
        );
    }

    #[test]
    fn test_analytical_height_mist_taylor_continuity() {
        let lambda = 0.26_f32;
        let dist = 120.0_f32;
        // Compare exact formula just above threshold (|u| = 0.00105) with Taylor just below (|u| = 0.00095)
        let dy_above = 0.00105 / (lambda * dist);
        let u_above = lambda * dy_above * dist;
        let exact = (1.0 - (-u_above).exp()) / (lambda * dy_above);

        let dy_below = 0.00095 / (lambda * dist);
        let u_below = lambda * dy_below * dist;
        let taylor = dist * (1.0 - 0.5 * u_below);

        assert!(
            (exact - taylor).abs() < 0.02,
            "Analytical mist integral must transition seamlessly across horizontal Taylor boundary: {} vs {}",
            exact,
            taylor
        );
    }

    #[test]
    fn test_fullscreen_near_ray_interpolation_exactness() {
        let cam_pos = Vec3::new(14.5, 3.2, -22.0);
        let cam = crate::camera3d::Camera3D::new(
            cam_pos,
            Vec3::new(18.0, 2.1, -35.0),
            16.0 / 9.0,
        );
        let inv_vp = cam.view_proj_matrix().inverse();

        // Fullscreen triangle vertices: v0=(-1, -1), v1=(3, -1), v2=(-1, 3)
        let near_ray_at = |x: f32, y: f32| -> Vec3 {
            let h = inv_vp * glam::Vec4::new(x, y, 1.0, 1.0);
            h.truncate() / h.w - cam_pos
        };
        let nr0 = near_ray_at(-1.0, -1.0);
        let nr1 = near_ray_at(3.0, -1.0);
        let nr2 = near_ray_at(-1.0, 3.0);

        for &(sx, sy) in &[(-0.85f32, -0.7f32), (0.0, 0.0), (0.65, 0.42), (-0.3, 0.9)] {
            // Barycentric weights on [-1,-1], [3,-1], [-1,3]
            let u = (sx + 1.0) * 0.25;
            let v = (sy + 1.0) * 0.25;
            let w = 1.0 - u - v;
            let interp_near_ray = nr0 * w + nr1 * u + nr2 * v;

            for &raw_depth in &[0.5f32, 0.05, 0.005, 0.0005] {
                let near_len = interp_near_ray.length();
                let fast_ray_dir = interp_near_ray / near_len;
                let fast_dist = near_len / raw_depth;

                let full_h = inv_vp * glam::Vec4::new(sx, sy, raw_depth, 1.0);
                let full_vec = (full_h.truncate() / full_h.w) - cam_pos;
                let full_dist = full_vec.length();
                let full_dir = full_vec / full_dist;

                assert!(
                    (fast_ray_dir - full_dir).length() < 1e-4,
                    "Interpolated near_ray direction must match full mat4x4 unprojection"
                );
                assert!(
                    ((fast_dist - full_dist) / full_dist).abs() < 1e-3,
                    "Analytical Reversed-Z distance {} must match full unprojection {}",
                    fast_dist,
                    full_dist
                );
            }
        }
    }
}


