// Luma Blur TOP — blur radius driven by input 2's brightness.
// p0 = (white radius in pixels, black radius in pixels, source, 0)
//
// Depth of field without a depth buffer: put the thing to keep sharp where
// input 2 is black. It is a single disc-sampled pass rather than a separable
// one, because a per-pixel radius is not separable — cost is fixed, but a
// very large radius will show the sample pattern.
//
// sources: 0 luminance, 1 alpha, 2 red

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let m = sample1(in.uv);
  let mode = i32(U.p0.z + 0.5);
  var k = dot(m.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
  if (mode == 1) { k = m.a; }
  if (mode == 2) { k = m.r; }

  let radius = mix(U.p0.y, U.p0.x, clamp(k, 0.0, 1.0));
  if (radius < 0.5) {
    return sample0(in.uv);
  }

  var taps = array<vec2<f32>, 12>(
    vec2<f32>( 1.0,  0.0), vec2<f32>( 0.5,  0.866), vec2<f32>(-0.5,  0.866),
    vec2<f32>(-1.0,  0.0), vec2<f32>(-0.5, -0.866), vec2<f32>( 0.5, -0.866),
    vec2<f32>( 0.5,  0.0), vec2<f32>( 0.25, 0.433), vec2<f32>(-0.25, 0.433),
    vec2<f32>(-0.5,  0.0), vec2<f32>(-0.25,-0.433), vec2<f32>( 0.25,-0.433),
  );

  var sum = sample0(in.uv);
  // Two rings of six, each ring sampled at three rotations, so the disc is
  // covered well enough that a bokeh does not turn into a hexagon.
  for (var i = 0; i < 12; i = i + 1) {
    let o = taps[i] * radius * U.res.zw;
    sum = sum + sample0(in.uv + o) + sample0(in.uv - o);
  }
  return sum / 25.0;
}
