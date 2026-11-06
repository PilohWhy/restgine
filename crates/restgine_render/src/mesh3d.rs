use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

/// 3D Mesh Vertex layout for instanced rendering.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Mesh3DVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

impl Mesh3DVertex {
    pub const fn new(pos: [f32; 3], norm: [f32; 3], uv: [f32; 2]) -> Self {
        Self {
            position: pos,
            normal: norm,
            uv,
        }
    }
}

/// Instance data sent to the GPU for each 3D entity (80 bytes per instance).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Mesh3DInstanceRaw {
    pub model_matrix: [[f32; 4]; 4],
    pub color: [f32; 4],
}

impl Mesh3DInstanceRaw {
    pub fn new(model: Mat4, color: [f32; 4]) -> Self {
        Self {
            model_matrix: model.to_cols_array_2d(),
            color,
        }
    }
}

/// Uniform for 3D Camera view-projection and lighting parameters.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Camera3DUniform {
    pub view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub light_dir: [f32; 4],
}

pub const MESH_3D_SHADER_WGSL: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    light_dir: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

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
    @location(7) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    let model = mat4x4<f32>(
        inst.model_0,
        inst.model_1,
        inst.model_2,
        inst.model_3,
    );
    let world_pos = model * vec4<f32>(in.position, 1.0);
    let world_normal = (model * vec4<f32>(in.normal, 0.0)).xyz;

    var out: VertexOutput;
    out.clip_pos = camera.view_proj * world_pos;
    out.world_pos = world_pos.xyz;
    out.world_normal = normalize(world_normal);
    out.color = inst.color;
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let light_dir = normalize(camera.light_dir.xyz);
    let normal = normalize(in.world_normal);
    
    // Ambient light
    let ambient = vec3<f32>(0.20, 0.24, 0.32);
    
    // Diffuse directional light
    let diff = max(dot(normal, light_dir), 0.0);
    let diffuse = diff * vec3<f32>(1.0, 0.98, 0.92);
    
    // Specular highlight
    let view_dir = normalize(camera.camera_pos.xyz - in.world_pos);
    let reflect_dir = reflect(-light_dir, normal);
    let spec = pow(max(dot(view_dir, reflect_dir), 0.0), 32.0);
    let specular = spec * vec3<f32>(0.4, 0.4, 0.4);

    let lit_color = (ambient + diffuse) * in.color.rgb + specular;
    return vec4<f32>(lit_color, in.color.a);
}
"#;

pub struct Mesh3DPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub camera_buffer: wgpu::Buffer,
    pub camera_bind_group: wgpu::BindGroup,
    pub cube_vertex_buffer: wgpu::Buffer,
    pub cube_index_buffer: wgpu::Buffer,
    pub cube_index_count: u32,
    pub instance_buffer: wgpu::Buffer,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
}

impl Mesh3DPipeline {
    pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let camera_uniform = Camera3DUniform {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            camera_pos: [0.0, 10.0, 20.0, 1.0],
            light_dir: [0.5, 0.8, 0.3, 0.0],
        };

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mesh3D Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Mesh3D Camera Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Mesh3D Camera Bind Group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        // 3D Unit Cube Vertices (24 vertices with normals)
        let (cube_verts, cube_indices) = create_cube_geometry();

        let cube_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mesh3D Cube Vertex Buffer"),
            contents: bytemuck::cast_slice(&cube_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let cube_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mesh3D Cube Index Buffer"),
            contents: bytemuck::cast_slice(&cube_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let max_instances = 2048;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Mesh3D Instance Buffer"),
            size: (max_instances * std::mem::size_of::<Mesh3DInstanceRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let depth_texture = create_depth_texture(device, width, height);
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Mesh3D Shader"),
            source: wgpu::ShaderSource::Wgsl(MESH_3D_SHADER_WGSL.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Mesh3D Pipeline Layout"),
            bind_group_layouts: &[&camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Mesh3D Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[
                    // Vertex buffer layout
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Mesh3DVertex>() as wgpu::BufferAddress,
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
                    // Instance buffer layout
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Mesh3DInstanceRaw>()
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
                                format: wgpu::VertexFormat::Float32x4, // color
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
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: Self::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::GreaterEqual, // Reversed-Z
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            camera_buffer,
            camera_bind_group,
            cube_vertex_buffer,
            cube_index_buffer,
            cube_index_count: cube_indices.len() as u32,
            instance_buffer,
            depth_texture,
            depth_view,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.depth_texture = create_depth_texture(device, width, height);
        self.depth_view = self
            .depth_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
    }

    pub fn update_camera(
        &self,
        queue: &wgpu::Queue,
        view_proj: Mat4,
        cam_pos: glam::Vec3,
        light_dir: glam::Vec3,
    ) {
        let uniform = Camera3DUniform {
            view_proj: view_proj.to_cols_array_2d(),
            camera_pos: [cam_pos.x, cam_pos.y, cam_pos.z, 1.0],
            light_dir: [light_dir.x, light_dir.y, light_dir.z, 0.0],
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[uniform]));
    }
}

fn create_depth_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Mesh3D Depth Texture"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: Mesh3DPipeline::DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn create_cube_geometry() -> (Vec<Mesh3DVertex>, Vec<u16>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // 6 faces of a unit cube [-0.5, 0.5]
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        // Front (+Z)
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        // Back (-Z)
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        // Top (+Y)
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        // Bottom (-Y)
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        // Right (+X)
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        // Left (-X)
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ];

    for (normal, right, up) in faces {
        let base_idx = vertices.len() as u16;
        let c = [normal[0] * 0.5, normal[1] * 0.5, normal[2] * 0.5];
        let r = [right[0] * 0.5, right[1] * 0.5, right[2] * 0.5];
        let u = [up[0] * 0.5, up[1] * 0.5, up[2] * 0.5];

        // 4 corners of face
        let p0 = [c[0] - r[0] - u[0], c[1] - r[1] - u[1], c[2] - r[2] - u[2]];
        let p1 = [c[0] + r[0] - u[0], c[1] + r[1] - u[1], c[2] + r[2] - u[2]];
        let p2 = [c[0] + r[0] + u[0], c[1] + r[1] + u[1], c[2] + r[2] + u[2]];
        let p3 = [c[0] - r[0] + u[0], c[1] - r[1] + u[1], c[2] - r[2] + u[2]];

        vertices.push(Mesh3DVertex::new(p0, normal, [0.0, 1.0]));
        vertices.push(Mesh3DVertex::new(p1, normal, [1.0, 1.0]));
        vertices.push(Mesh3DVertex::new(p2, normal, [1.0, 0.0]));
        vertices.push(Mesh3DVertex::new(p3, normal, [0.0, 0.0]));

        indices.extend_from_slice(&[
            base_idx,
            base_idx + 1,
            base_idx + 2,
            base_idx,
            base_idx + 2,
            base_idx + 3,
        ]);
    }

    (vertices, indices)
}
