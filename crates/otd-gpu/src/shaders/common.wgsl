// Prelude prepended to every TOP shader.
//
// One bind group layout serves every operator: a uniform block, a sampler and
// two input textures. Operators with fewer inputs get a 1x1 transparent dummy
// bound, which keeps pipeline creation uniform and means adding an operator is
// a single .wgsl file plus a table entry.

struct Uniforms {
  // x, y = resolution in pixels; z, w = 1/resolution
  res: vec4<f32>,
  // x = absolute time (s), y = local time (s), z = frame, w = fps
  time: vec4<f32>,
  // Operator parameters, packed by the Rust side. See ops.rs for the layout
  // each operator expects. Built-in operators fill p0..p3 and leave the rest
  // zero; the GLSL TOP uses all twelve, because imported ISF shaders declare
  // as many as forty-eight components of inputs. Keep the count in step with
  // ops::PARAM_VECS and with the GLSL block in shader.rs — a test checks.
  p0: vec4<f32>,
  p1: vec4<f32>,
  p2: vec4<f32>,
  p3: vec4<f32>,
  p4: vec4<f32>,
  p5: vec4<f32>,
  p6: vec4<f32>,
  p7: vec4<f32>,
  p8: vec4<f32>,
  p9: vec4<f32>,
  p10: vec4<f32>,
  p11: vec4<f32>,
};

@group(0) @binding(0) var<uniform> U: Uniforms;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex0: texture_2d<f32>;
@group(0) @binding(3) var tex1: texture_2d<f32>;

struct VOut {
  @builtin(position) pos: vec4<f32>,
  // (0,0) top-left, (1,1) bottom-right
  @location(0) uv: vec2<f32>,
};

// Fullscreen triangle — no vertex buffer, no index buffer.
@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VOut {
  var out: VOut;
  let x = f32((idx << 1u) & 2u);
  let y = f32(idx & 2u);
  out.uv = vec2<f32>(x, y);
  out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
  return out;
}

fn sample0(uv: vec2<f32>) -> vec4<f32> {
  return textureSample(tex0, samp, uv);
}

fn sample1(uv: vec2<f32>) -> vec4<f32> {
  return textureSample(tex1, samp, uv);
}
