// Sketch overlay: unlit 2D linework in the sketch plane, drawn in 3D.

struct Camera {
  view_proj: mat4x4<f32>,
  clip_plane: vec4f,
  clip_enabled: u32,
  _pad: u32,
};

@group(0) @binding(0) var<uniform> cam: Camera;

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
  // Sketch linework: bright green, slightly transparent.
  return vec4f(0.45, 0.85, 0.45, 0.9);
}
