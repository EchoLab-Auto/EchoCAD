// Picking pass: renders each feature's id (encoded in `params.base_color`
// RGB, low 24 bits) to a small texture; a 1-pixel readback decodes the
// feature under the cursor.

struct Camera {
  view_proj: mat4x4<f32>,
  clip_plane: vec4f,
  clip_enabled: u32,
  _pad: u32,
};

@group(0) @binding(0) var<uniform> cam: Camera;

struct MeshParams {
  base_color: vec3f,
  highlight: u32,
};

@group(1) @binding(0) var<uniform> params: MeshParams;

struct VsOut {
  @builtin(position) pos: vec4f,
};

@vertex
fn vs_main(@location(0) pos: vec3f, @location(1) normal: vec3f) -> VsOut {
  var out: VsOut;
  out.pos = cam.view_proj * vec4f(pos, 1.0);
  return out;
}

@fragment
fn fs_main() -> @location(0) vec4f {
  // base_color holds the packed 24-bit feature id.
  return vec4f(params.base_color, 1.0);
}
