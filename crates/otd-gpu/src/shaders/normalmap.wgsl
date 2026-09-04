// Normal Map TOP — a height field becomes a tangent-space normal.
// p0 = (source: 0 luminance / 1 alpha / 2 red, strength, flip green, 0)
//
// Feed the result to the Render TOP's material, or to anything that wants a
// direction per pixel. Flip Green is the OpenGL/DirectX convention switch and
// is the reason a normal map ever looks lit from the wrong side.

fn height(uv: vec2<f32>, mode: i32) -> f32 {
  let c = sample0(uv);
  if (mode == 1) { return c.a; }
  if (mode == 2) { return c.r; }
  return dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let mode = i32(U.p0.x + 0.5);
  let e = U.res.zw;
  let dx = height(in.uv + vec2<f32>(e.x, 0.0), mode) - height(in.uv - vec2<f32>(e.x, 0.0), mode);
  let dy = height(in.uv + vec2<f32>(0.0, e.y), mode) - height(in.uv - vec2<f32>(0.0, e.y), mode);

  var n = normalize(vec3<f32>(-dx * U.p0.y, -dy * U.p0.y, 1.0));
  if (U.p0.z > 0.5) { n.y = -n.y; }
  return vec4<f32>(n * 0.5 + 0.5, 1.0);
}
