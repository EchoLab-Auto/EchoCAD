//! Scene data for the wgpu renderer: mesh buffers and the orbit camera.
//!
//! P1 scope: one `GpuMesh` per feature (positions/normals/indices), a shared
//! render pipeline, and an orbit camera driven by native window events.
//! Edge/line rendering and GPU picking land in P2.

use std::collections::HashMap;

use wgpu::util::DeviceExt;

/// Geometry for a single feature, ready to be uploaded to the GPU.
#[derive(Debug, Clone, Default)]
pub struct SceneMesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    pub label: String,
    pub suppressed: bool,
    pub error: Option<String>,
}

/// Per-mesh uniform (16 bytes, std140): base color + highlight flag.
/// During the picking pass `base_color` carries the packed 24-bit feature id.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshParams {
    pub base_color: [f32; 3],
    pub highlight: u32,
}

impl MeshParams {
    pub const DEFAULT_COLOR: [f32; 3] = [0.62, 0.66, 0.72];
    pub const HIGHLIGHT_COLOR: [f32; 3] = [0.95, 0.72, 0.25];

    /// Pack a 24-bit feature id into the RGB channels.
    pub fn from_feature_id(id: u32) -> Self {
        Self {
            base_color: [
                ((id >> 16) & 0xFF) as f32 / 255.0,
                ((id >> 8) & 0xFF) as f32 / 255.0,
                (id & 0xFF) as f32 / 255.0,
            ],
            highlight: 0,
        }
    }

    /// Decode a 24-bit feature id from an Rgba8Unorm readback pixel.
    pub fn decode_id(r: u8, g: u8, b: u8) -> u32 {
        ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }
}

/// GPU-side buffers for one feature.
pub struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    #[allow(dead_code)]
    vertex_count: u32,
    /// Per-mesh parameters (base color / highlight), bound to group 1.
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// CPU-side copy of the params, rewritten each frame.
    pub params: MeshParams,
}

impl GpuMesh {
    pub fn vertex_buffer(&self) -> &wgpu::Buffer {
        &self.vertex_buffer
    }

    pub fn index_buffer(&self) -> &wgpu::Buffer {
        &self.index_buffer
    }

    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    /// Upload the per-mesh params (color/highlight) to the GPU.
    pub fn write_params(&self, queue: &wgpu::Queue, params: MeshParams) {
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[params]),
        );
    }
}

/// All feature meshes, keyed by feature id.
pub struct MeshBuffers {
    meshes: HashMap<u32, GpuMesh>,
    /// Feature ids that were deleted since the last frame.
    removed: Vec<u32>,
}

impl Default for MeshBuffers {
    fn default() -> Self {
        Self {
            meshes: HashMap::new(),
            removed: Vec::new(),
        }
    }
}

impl MeshBuffers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Upload or replace the GPU buffers for `feature_id`. Call on the main
    /// thread only (wgpu resources are not Send).
    pub fn upsert(
        &mut self,
        device: &wgpu::Device,
        params_layout: &wgpu::BindGroupLayout,
        feature_id: u32,
        mesh: &SceneMesh,
    ) {
        let vertex_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("mesh-{feature_id} vertices")),
                contents: bytemuck::cast_slice(&mesh.positions),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("mesh-{feature_id} indices")),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("mesh-{feature_id} params")),
            size: std::mem::size_of::<MeshParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("mesh-{feature_id} params")),
            layout: params_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        self.meshes.insert(
            feature_id,
            GpuMesh {
                vertex_buffer,
                index_buffer,
                index_count: mesh.indices.len() as u32,
                vertex_count: (mesh.positions.len() / 3) as u32,
                uniform_buffer,
                bind_group,
                params: MeshParams {
                    base_color: MeshParams::DEFAULT_COLOR,
                    highlight: 0,
                },
            },
        );
        self.removed.retain(|id| *id != feature_id);
    }

    /// Queue a feature for removal at the next frame.
    pub fn remove(&mut self, feature_id: u32) {
        self.meshes.remove(&feature_id);
    }

    /// Clear everything (new document).
    pub fn clear(&mut self) {
        self.meshes.clear();
    }

    /// Number of meshes currently resident on the GPU.
    pub fn len(&self) -> usize {
        self.meshes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u32, &GpuMesh)> {
        self.meshes.iter()
    }
}

/// Orbit camera (target + yaw/pitch + distance), the CAD-viewport classic.
#[derive(Debug, Clone)]
pub struct OrbitCamera {
    pub target: glam::Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_y_rad: f32,
    pub near: f32,
    pub far: f32,
    pub up: glam::Vec3,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: glam::Vec3::ZERO,
            distance: 8.0,
            yaw: 0.7,
            pitch: 0.45,
            fov_y_rad: 50f32.to_radians(),
            near: 0.01,
            far: 10000.0,
            up: glam::Vec3::Y,
        }
    }
}

impl OrbitCamera {
    /// Camera position for the current orbit parameters.
    pub fn eye(&self) -> glam::Vec3 {
        let cp = self.pitch.cos();
        let dir = glam::Vec3::new(
            self.distance * cp * self.yaw.sin(),
            self.distance * self.pitch.sin(),
            self.distance * cp * self.yaw.cos(),
        );
        self.target + dir
    }

    pub fn view_matrix(&self) -> glam::Mat4 {
        glam::Mat4::look_at_rh(self.eye(), self.target, self.up)
    }

    pub fn projection(&self, aspect: f32) -> glam::Mat4 {
        glam::Mat4::perspective_rh(self.fov_y_rad, aspect.max(0.001), self.near, self.far)
    }

    /// Rotate by pixel deltas (right-drag). `viewport_h` in physical pixels.
    pub fn orbit(&mut self, dx_px: f32, dy_px: f32, viewport_h: f32) {
        let scale = 0.008 * (viewport_h / 900.0).clamp(0.5, 2.0);
        self.yaw -= dx_px * scale;
        self.pitch = (self.pitch + dy_px * scale).clamp(-1.55, 1.55);
    }

    /// Pan perpendicular to the view direction (middle-drag or shift+drag).
    /// `viewport_h` in physical pixels.
    pub fn pan(&mut self, dx_px: f32, dy_px: f32, viewport_h: f32) {
        let world_per_px = 2.0 * self.distance * (self.fov_y_rad / 2.0).tan()
            / viewport_h.max(1.0);
        let right = self.view_matrix().x_axis.truncate().normalize();
        let up = self.view_matrix().y_axis.truncate().normalize();
        self.target -= right * dx_px * world_per_px;
        self.target += up * dy_px * world_per_px;
    }

    /// Zoom by a multiplicative factor (< 1 zooms in).
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(0.001, 1.0e6);
    }

    /// Fit the camera to a bounding box (min, max) in world coordinates.
    pub fn fit_to(&mut self, bbox: (glam::Vec3, glam::Vec3), aspect: f32) {
        let (min, max) = bbox;
        let center = (min + max) * 0.5;
        let radius = (max - min).length() * 0.5;
        if radius <= 1e-9 {
            return;
        }
        // Keep the current direction, just re-target and re-distance.
        let fov_scale = 1.0 / (self.fov_y_rad / 2.0).tan();
        let distance = radius * fov_scale * 1.1 / aspect.max(0.1).min(3.0);
        self.target = center;
        self.distance = distance.max(radius * 0.1);
    }

}

/// Axis-aligned bounds across scene meshes (for fitView). `None` if empty.
pub fn scene_bounds(meshes: &HashMap<u32, SceneMesh>) -> Option<(glam::Vec3, glam::Vec3)> {
    let mut min = glam::Vec3::splat(f32::INFINITY);
    let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for mesh in meshes.values() {
        for chunk in mesh.positions.chunks_exact(3) {
            let p = glam::Vec3::new(chunk[0], chunk[1], chunk[2]);
            min = min.min(p);
            max = max.max(p);
            any = true;
        }
    }
    any.then_some((min, max))
}

/// Camera uniform block shared by all pipelines.
/// 96 bytes, no padding: mat4 (64) + clip plane (16) + flags (8) + pad (8).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: glam::Mat4,
    /// World-space clipping plane (a, b, c, d): dot(pos, abc) + d < 0 discards.
    pub clip_plane: [f32; 4],
    /// 1 = clipping enabled, 0 = disabled.
    pub clip_enabled: u32,
    /// Padding to 16-byte alignment.
    pub _pad: u32,
    /// More padding to reach 96 bytes with zero internal padding.
    pub _pad2: [f32; 2],
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self {
            view_proj: glam::Mat4::IDENTITY,
            clip_plane: [0.0, -1.0, 0.0, 0.0], // Y=0 plane, keep positive Y
            clip_enabled: 0,
            _pad: 0,
            _pad2: [0.0; 2],
        }
    }
}

impl CameraUniform {
    pub fn from_camera(camera: &OrbitCamera, aspect: f32) -> Self {
        let mut u = Self::default();
        u.view_proj = camera.projection(aspect) * camera.view_matrix();
        u
    }

    /// Enable clipping against a world-space plane (a, b, c, d).
    pub fn with_clip(mut self, plane: [f32; 4], enabled: bool) -> Self {
        self.clip_plane = plane;
        self.clip_enabled = u32::from(enabled);
        self
    }
}

/// Orthographic camera for the auxiliary viewports (front/top/right).
#[derive(Debug, Clone)]
pub struct OrthoCamera {
    pub eye: glam::Vec3,
    pub target: glam::Vec3,
    pub up: glam::Vec3,
    pub size: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for OrthoCamera {
    fn default() -> Self {
        Self {
            eye: glam::Vec3::new(0.0, 0.0, 10.0),
            target: glam::Vec3::ZERO,
            up: glam::Vec3::Y,
            size: 10.0,
            near: -1000.0,
            far: 1000.0,
        }
    }
}

impl OrthoCamera {
    pub fn view_matrix(&self) -> glam::Mat4 {
        glam::Mat4::look_at_rh(self.eye, self.target, self.up)
    }

    pub fn projection(&self, aspect: f32) -> glam::Mat4 {
        let half = self.size * 0.5;
        if aspect >= 1.0 {
            glam::Mat4::orthographic_rh(
                -half * aspect,
                half * aspect,
                -half,
                half,
                self.near,
                self.far,
            )
        } else {
            glam::Mat4::orthographic_rh(
                -half,
                half,
                -half / aspect,
                half / aspect,
                self.near,
                self.far,
            )
        }
    }

    /// Fit the ortho framing to a bounding box (min, max).
    pub fn fit_to(&mut self, bbox: (glam::Vec3, glam::Vec3)) {
        let (min, max) = bbox;
        self.target = (min + max) * 0.5;
        self.size = (max - min).length().max(1.0) * 1.2;
    }

    /// Position the camera to look down the given axis.
    pub fn align_axis(&mut self, axis: glam::Vec3, up: glam::Vec3) {
        self.eye = self.target + axis.normalize() * self.size.max(1.0);
        self.up = up;
    }
}

/// Fixed auxiliary viewport cameras, all sharing one ortho size.
pub struct AuxCameras {
    pub front: OrthoCamera,
    pub top: OrthoCamera,
    pub right: OrthoCamera,
}

impl Default for AuxCameras {
    fn default() -> Self {
        let mut c = Self {
            front: OrthoCamera::default(),
            top: OrthoCamera::default(),
            right: OrthoCamera::default(),
        };
        c.front.align_axis(glam::Vec3::Z, glam::Vec3::Y); // looking -Z from +Z
        c.top.align_axis(glam::Vec3::Y, glam::Vec3::Z); // looking -Y from +Y
        c.right.align_axis(glam::Vec3::X, glam::Vec3::Y); // looking -X from +X
        c
    }
}

impl AuxCameras {
    pub fn fit_to(&mut self, bbox: (glam::Vec3, glam::Vec3)) {
        let (min, max) = bbox;
        let center = (min + max) * 0.5;
        let size = (max - min).length().max(1.0) * 1.2;
        for cam in [&mut self.front, &mut self.top, &mut self.right] {
            cam.target = center;
            cam.size = size;
            cam.eye = cam.target + (cam.eye - cam.target).normalize() * size;
        }
    }
}
