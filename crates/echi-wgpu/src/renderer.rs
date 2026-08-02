//! Core wgpu renderer: device setup, surface management, scene rendering.
//!
//! The scene (`MeshBuffers` + `OrbitCamera`) is drawn each frame with a
//! shared solid pipeline and an optional wireframe pass. Input arrives from
//! native window events (see `plugin.rs`); mesh updates arrive through a
//! pending queue decoupled from the main thread.

use wgpu::util::DeviceExt;

use crate::scene::{CameraUniform, MeshBuffers, MeshParams, OrbitCamera, SceneMesh};

/// Panel layout constants (logical pixels). The Rust side owns these values;
/// the CSS inside each panel webview mirrors them.
#[derive(Debug, Clone, Copy)]
pub struct PanelLayout {
    /// Left feature-tree panel width.
    pub left_width: f64,
    /// Right properties panel width.
    pub right_width: f64,
    /// Top toolbar height.
    pub top_height: f64,
}

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            left_width: 260.0,
            right_width: 300.0,
            top_height: 48.0,
        }
    }
}

impl PanelLayout {
    /// Viewport rect in physical pixels for a window of `window_width` ×
    /// `window_height` logical px at scale factor `scale`.
    pub fn viewport_rect_physical(
        &self,
        window_width: f64,
        window_height: f64,
        scale: f64,
    ) -> (f64, f64, f64, f64) {
        let x = self.left_width * scale;
        let y = self.top_height * scale;
        let w = (window_width - self.left_width - self.right_width) * scale;
        let h = (window_height - self.top_height) * scale;
        (x, y, w.max(1.0), h.max(1.0))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("wgpu surface creation failed: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    #[error("wgpu adapter request failed: {0}")]
    Adapter(#[from] wgpu::RequestAdapterError),
    #[error("wgpu device request failed: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("no compatible surface format found")]
    NoSurfaceFormat,
    #[error("shader compilation failed")]
    Shader,
}

/// Rendering mode for the main pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Solid,
    Wireframe,
}

/// Viewport arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Single perspective viewport.
    Perspective,
    /// Four viewports: front / top / right (ortho) + perspective.
    Four,
}

/// Owns the GPU device/queue and the window surface.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,

    pub meshes: MeshBuffers,
    pub camera: OrbitCamera,
    pub mode: RenderMode,
    pub view_mode: ViewMode,
    /// Orthographic cameras for the auxiliary viewports (front/top/right).
    pub aux: crate::scene::AuxCameras,
    /// World-space clip plane (a, b, c, d).
    pub clip_plane: [f32; 4],
    pub clip_enabled: bool,
    /// Currently highlighted feature (selection).
    pub highlighted: Option<u32>,

    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    params_layout: wgpu::BindGroupLayout,
    solid_pipeline: wgpu::RenderPipeline,
    wire_pipeline: wgpu::RenderPipeline,
    id_pipeline: wgpu::RenderPipeline,
    sketch_pipeline: wgpu::RenderPipeline,
    sketch_mesh: Option<wgpu::Buffer>,
    sketch_vertex_count: u32,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,

    // Picking (P2): id pass renders to a small texture, then 1 px is copied
    // to a readback buffer and mapped asynchronously.
    pick_texture: wgpu::Texture,
    pick_view: wgpu::TextureView,
    readback_buffer: wgpu::Buffer,
    pending_pick: Option<PendingPick>,
}

/// An in-flight GPU pick. `mapped` is set by the map_async callback; the
/// plugin polls it each frame and resolves the id from the buffer contents.
struct PendingPick {
    buffer: wgpu::Buffer,
    mapped: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Pick resolution: (feature_id, ok). `None` when the pick is still pending.
pub struct PickResult {
    pub feature_id: Option<u32>,
}

impl Renderer {
    /// Initialize the GPU device and the window surface. Blocks on adapter
    /// enumeration; call from the main thread at first redraw.
    ///
    /// `initial_size` is the window size in physical pixels; the surface is
    /// re-configured on `Resized` events afterwards.
    ///
    /// The surface takes ownership of `window`'s handle (`Box<dyn WindowHandle>`),
    /// which keeps it alive for the lifetime of the surface.
    ///
    /// `display` is required by wgpu 30: the instance must know the platform
    /// display connection before a surface can be created from a window handle.
    pub fn new<H>(
        window: H,
        display: Box<dyn wgpu::wgt::WgpuHasDisplayHandle>,
        initial_size: (u32, u32),
    ) -> Result<Self, RendererError>
    where
        H: wgpu::WindowHandle + 'static,
    {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            display: Some(display),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(wgpu::SurfaceTarget::Window(Box::new(window)))?;
        let (device, queue, config) = pollster::block_on(async {
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    ..Default::default()
                })
                .await?;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("echi-wgpu"),
                    // POLYGON_MODE_LINE for the wireframe pass.
                    required_features: wgpu::Features::POLYGON_MODE_LINE,
                    required_limits: wgpu::Limits::default(),
                    experimental_features: wgpu::ExperimentalFeatures::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                    trace: wgpu::Trace::Off,
                })
                .await?;
            let caps = surface.get_capabilities(&adapter);
            let format = caps
                .formats
                .iter()
                .copied()
                .find(|f| f.is_srgb())
                .ok_or(RendererError::NoSurfaceFormat)?;
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                color_space: wgpu::SurfaceColorSpace::Auto,
                width: initial_size.0.max(1),
                height: initial_size.1.max(1),
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: 2,
                alpha_mode: caps.alpha_modes[0],
                view_formats: vec![],
            };
            surface.configure(&device, &config);
            Ok::<_, RendererError>((device, queue, config))
        })?;

        let (depth_texture, depth_view) = create_depth(&device, config.width, config.height);

        // Camera uniform (one per frame, shared by all pipelines).
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera uniforms"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let solid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("echi-wgpu solid shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("solid.wgsl").into()),
        });
        let id_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("echi-wgpu id shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("id.wgsl").into()),
        });

        // Group 0: camera; Group 1: per-mesh params.
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
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
        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh params layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera uniforms"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("solid pipeline layout"),
            bind_group_layouts: &[Some(&camera_layout), Some(&params_layout)],
            immediate_size: 0,
        });

        let make_pipeline = |label: &str,
                             module: &wgpu::ShaderModule,
                             polygon_mode: wgpu::PolygonMode,
                             cull: bool,
                             target_format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(SceneVertex::LAYOUT)],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: if cull {
                        Some(wgpu::Face::Back)
                    } else {
                        None
                    },
                    polygon_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let solid_pipeline = make_pipeline(
            "solid",
            &solid_shader,
            wgpu::PolygonMode::Fill,
            true,
            config.format,
        );
        let wire_pipeline = make_pipeline(
            "wireframe",
            &solid_shader,
            wgpu::PolygonMode::Line,
            false,
            config.format,
        );
        let id_pipeline = make_pipeline(
            "id pass",
            &id_shader,
            wgpu::PolygonMode::Fill,
            false,
            wgpu::TextureFormat::Rgba8Unorm,
        );

        // Sketch overlay: unlit line list, camera group only.
        let sketch_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("echi-wgpu sketch shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sketch.wgsl").into()),
        });
        let sketch_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sketch pipeline layout"),
            bind_group_layouts: &[Some(&camera_layout)],
            immediate_size: 0,
        });
        let sketch_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sketch lines"),
                layout: Some(&sketch_layout),
                vertex: wgpu::VertexState {
                    module: &sketch_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(SceneVertex::LAYOUT)],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineList,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &sketch_shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });

        // Picking target: full-surface Rgba8Unorm texture + 4-byte readback.
        let pick_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("echi-wgpu pick"),
            size: wgpu::Extent3d {
                width: config.width.max(1),
                height: config.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let pick_view = pick_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("echi-wgpu pick readback"),
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Ok(Self {
            device,
            queue,
            surface,
            config,
            meshes: MeshBuffers::new(),
            camera: OrbitCamera::default(),
            mode: RenderMode::Solid,
            view_mode: ViewMode::Perspective,
            aux: crate::scene::AuxCameras::default(),
            clip_plane: [0.0, -1.0, 0.0, 0.0],
            clip_enabled: false,
            highlighted: None,
            camera_buffer,
            camera_bind_group,
            params_layout,
            solid_pipeline,
            wire_pipeline,
            id_pipeline,
            sketch_pipeline,
            sketch_mesh: None,
            sketch_vertex_count: 0,
            depth_texture,
            depth_view,
            pick_texture,
            pick_view,
            readback_buffer,
            pending_pick: None,
        })
    }

    /// Surface size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Handle a native resize event (physical pixel size).
    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if self.config.width == width && self.config.height == height {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        let (depth_texture, depth_view) = create_depth(&self.device, width, height);
        self.depth_texture = depth_texture;
        self.depth_view = depth_view;
    }

    /// Upload or replace one feature's mesh (main thread).
    pub fn upsert_mesh(&mut self, feature_id: u32, mesh: &SceneMesh) {
        self.meshes
            .upsert(&self.device, &self.params_layout, feature_id, mesh);
    }

    /// Remove one feature's mesh (main thread).
    pub fn remove_mesh(&mut self, feature_id: u32) {
        self.meshes.remove(feature_id);
    }

    /// Clear all meshes (new document).
    pub fn clear_meshes(&mut self) {
        self.meshes.clear();
    }

    /// Fit the camera to the given world-space bounding box.
    pub fn fit_view(&mut self, bbox: (glam::Vec3, glam::Vec3)) {
        let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
        self.camera.fit_to(bbox, aspect);
        self.aux.fit_to(bbox);
    }

    /// Render one frame. Called from the event loop on MainEventsCleared.
    pub fn render(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                t
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                // Surface was invalidated (resize raced, minimize/restore).
                // Reconfigure; the next frame will succeed. Avoids the
                // classic black-screen trap.
                self.surface.configure(&self.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                // Window hidden/minimized — skip this frame.
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                // Surface configuration was invalidated; reconfigure and retry.
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("echi-wgpu frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("echi-wgpu main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.075,
                            g: 0.085,
                            b: 0.105,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            let (w, h) = (self.config.width as f32, self.config.height.max(1) as f32);
            match self.view_mode {
                ViewMode::Perspective => {
                    let aspect = w / h;
                    self.set_camera_uniform(
                        CameraUniform::from_camera(&self.camera, aspect),
                    );
                    self.draw_scene(&mut pass, true);
                }
                ViewMode::Four => {
                    // Front (top-left), Top (top-right), Right (bottom-left),
                    // Perspective (bottom-right).
                    let quads = [
                        (0.0, 0.0, w / 2.0, h / 2.0),
                        (w / 2.0, 0.0, w / 2.0, h / 2.0),
                        (0.0, h / 2.0, w / 2.0, h / 2.0),
                        (w / 2.0, h / 2.0, w / 2.0, h / 2.0),
                    ];
                    for (i, (x, y, vw, vh)) in quads.iter().enumerate() {
                        pass.set_viewport(*x, *y, *vw, *vh, 0.0, 1.0);
                        let aspect = vw / vh.max(1.0);
                        let uniform = match i {
                            0 => {
                                let c = &self.aux.front;
                                CameraUniform {
                                    view_proj: c.projection(aspect) * c.view_matrix(),
                                    ..Default::default()
                                }
                            }
                            1 => {
                                let c = &self.aux.top;
                                CameraUniform {
                                    view_proj: c.projection(aspect) * c.view_matrix(),
                                    ..Default::default()
                                }
                            }
                            2 => {
                                let c = &self.aux.right;
                                CameraUniform {
                                    view_proj: c.projection(aspect) * c.view_matrix(),
                                    ..Default::default()
                                }
                            }
                            _ => CameraUniform::from_camera(&self.camera, aspect),
                        };
                        self.set_camera_uniform(uniform);
                        self.draw_scene(&mut pass, i == 3);
                    }
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
    }

    /// Write the current camera uniform (with clip state) to the GPU.
    fn set_camera_uniform(&mut self, mut uniform: CameraUniform) {
        uniform = uniform.with_clip(self.clip_plane, self.clip_enabled);
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[uniform]),
        );
    }

    /// Draw all meshes (+ sketch overlay) with the current camera uniform.
    fn draw_scene(&mut self, pass: &mut wgpu::RenderPass<'_>, include_sketch: bool) {
        let pipeline = match self.mode {
            RenderMode::Solid => &self.solid_pipeline,
            RenderMode::Wireframe => &self.wire_pipeline,
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        for (feature_id, mesh) in self.meshes.iter() {
            if mesh.index_count() == 0 {
                continue;
            }
            let mut params = mesh.params;
            params.highlight = u32::from(self.highlighted == Some(*feature_id));
            mesh.write_params(&self.queue, params);
            pass.set_bind_group(1, mesh.bind_group(), &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer().slice(..));
            pass.set_index_buffer(mesh.index_buffer().slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count(), 0, 0..1);
        }
        // Sketch overlay on top of the solids.
        if include_sketch {
            if let Some(buffer) = &self.sketch_mesh {
                pass.set_pipeline(&self.sketch_pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..self.sketch_vertex_count, 0..1);
            }
        }
    }

    /// Upload the sketch overlay linework (positions of a LineList).
    pub fn set_sketch(&mut self, positions: &[f32]) {
        if positions.is_empty() {
            self.sketch_mesh = None;
            self.sketch_vertex_count = 0;
            return;
        }
        let vertices: Vec<SceneVertex> = positions
            .chunks_exact(3)
            .map(|p| SceneVertex {
                pos: [p[0], p[1], p[2]],
                normal: [0.0, 0.0, 0.0],
            })
            .collect();
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("sketch linework"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.sketch_mesh = Some(buffer);
        self.sketch_vertex_count = vertices.len() as u32;
    }

    /// Clear the sketch overlay.
    pub fn clear_sketch(&mut self) {
        self.sketch_mesh = None;
        self.sketch_vertex_count = 0;
    }

    // ── Picking (P2) ───────────────────────────────────────────────────

    /// Set the highlighted (selected) feature, or `None` to clear.
    pub fn set_highlight(&mut self, feature_id: Option<u32>) {
        self.highlighted = feature_id;
    }

    /// Render an id pass at `(x, y)` (physical pixels) and stage an async
    /// readback. Resolve the result with `poll_pick` in subsequent frames.
    pub fn request_pick(&mut self, x: u32, y: u32) {
        if self.meshes.is_empty() || self.pending_pick.is_some() {
            return;
        }
        let x = x.min(self.config.width.saturating_sub(1));
        let y = y.min(self.config.height.saturating_sub(1));

        let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
        let uniform = CameraUniform::from_camera(&self.camera, aspect);
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[uniform]),
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("echi-wgpu pick pass"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("echi-wgpu id pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.pick_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.id_pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            for (feature_id, mesh) in self.meshes.iter() {
                if mesh.index_count() == 0 {
                    continue;
                }
                // Encode the 24-bit feature id into the params color.
                let params = MeshParams::from_feature_id(*feature_id);
                mesh.write_params(&self.queue, params);
                pass.set_bind_group(1, mesh.bind_group(), &[]);
                pass.set_vertex_buffer(0, mesh.vertex_buffer().slice(..));
                pass.set_index_buffer(mesh.index_buffer().slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count(), 0, 0..1);
            }
        }
        // Copy the single pixel under the cursor to the readback buffer.
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.pick_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let mapped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let m2 = mapped.clone();
        self.readback_buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            m2.store(r.is_ok(), std::sync::atomic::Ordering::Relaxed);
        });
        self.pending_pick = Some(PendingPick {
            buffer: self.readback_buffer.clone(),
            mapped,
        });
    }

    /// Poll the pending pick; returns `Some(result)` once the GPU readback
    /// is available. Call once per frame.
    pub fn poll_pick(&mut self) -> Option<PickResult> {
        let Some(pending) = self.pending_pick.take() else {
            return None;
        };
        // Force the map_async callback to run (blocks until the pick pass
        // completes; pick is user-triggered so a brief wait is fine).
        let _ = self
            .device
            .poll(wgpu::wgt::PollType::wait_indefinitely());
        if !pending.mapped.load(std::sync::atomic::Ordering::Relaxed) {
            // Not ready yet — try again next frame.
            self.pending_pick = Some(pending);
            return None;
        }
        let feature_id = {
            let Ok(data) = pending.buffer.slice(..).get_mapped_range() else {
                return None;
            };
            let (r, g, b) = (data[0], data[1], data[2]);
            pending.buffer.unmap();
            let decoded = MeshParams::decode_id(r, g, b);
            if decoded == 0 || decoded == u32::MAX {
                None
            } else {
                Some(decoded)
            }
        };
        Some(PickResult { feature_id })
    }
}

/// Depth attachment for the current surface size.
fn create_depth(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let size = wgpu::Extent3d {
        width: width.max(1),
        height: height.max(1),
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("echi-wgpu depth"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SceneVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

impl SceneVertex {
    pub const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<SceneVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
    };
}
