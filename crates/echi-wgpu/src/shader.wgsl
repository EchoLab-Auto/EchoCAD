struct Uniforms {
  mvp: mat4x4<f32>,
  model: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
  @builtin(position) pos: vec4f,
  @location(0) normal: vec3f,
};

@vertex
fn vs_main(@location(0) pos: vec3f, @location(1) normal: vec3f) -> VsOut {
  var out: VsOut;
  out.pos = u.mvp * vec4f(pos, 1.0);
  out.normal = mat3x3(u.model[0].xyz, u.model[1].xyz, u.model[2].xyz) * normal;
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
  let n = normalize(in.normal);
  let light = normalize(vec3f(0.45, 0.8, 0.55));
  let diff = max(dot(n, light), 0.0);
  let color = vec3f(0.62, 0.66, 0.72);
  return vec4f(color * (0.3 + 0.7 * diff), 1.0);
}
