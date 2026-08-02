struct Camera {
  view_proj: mat4x4<f32>,
  clip_plane: vec4f,
  clip_enabled: u32,
  _pad: u32,
};

@group(0) @binding(0) var<uniform> cam: Camera;

// Per-mesh parameters (16 bytes, std140: vec3 + u32).
struct MeshParams {
  base_color: vec3f,
  highlight: u32,
};

@group(1) @binding(0) var<uniform> params: MeshParams;

struct VsOut {
  @builtin(position) pos: vec4f,
  @location(0) normal: vec3f,
  @location(1) world: vec3f,
};

@vertex
fn vs_main(@location(0) pos: vec3f, @location(1) normal: vec3f) -> VsOut {
  var out: VsOut;
  out.pos = cam.view_proj * vec4f(pos, 1.0);
  out.normal = normal;
  out.world = pos;
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
  if (cam.clip_enabled == 1u) {
    let d = dot(in.world, cam.clip_plane.xyz) + cam.clip_plane.w;
    if (d < 0.0) {
      discard;
    }
  }
  let n = normalize(in.normal);
  let light = normalize(vec3f(0.45, 0.8, 0.55));
  let diff = max(dot(n, light), 0.0);
  var color = params.base_color * (0.3 + 0.7 * diff);
  if (params.highlight == 1u) {
    color = color * 1.4 + vec3f(0.25, 0.12, 0.0);
  }
  return vec4f(color, 1.0);
}
