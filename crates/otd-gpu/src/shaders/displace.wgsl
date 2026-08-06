// Displace TOP — input 2 pushes input 1's lookup around.
//
// p0 = (amount, source channel X, source channel Y, offset)
// p1 = (extend: 0 zero / 1 hold / 2 repeat / 3 mirror, 0, 0, 0)
//
// channel codes: 0 r, 1 g, 2 b, 3 a, 4 luminance

fn channel(c: vec4<f32>, which: i32) -> f32 {
  if (which == 1) { return c.g; }
  if (which == 2) { return c.b; }
  if (which == 3) { return c.a; }
  if (which == 4) { return dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)); }
  return c.r;
}

fn extend_uv(uv: vec2<f32>, mode: i32) -> vec2<f32> {
  if (mode == 1) {
    return clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
  } else if (mode == 2) {
    return fract(uv);
  } else if (mode == 3) {
    return 1.0 - abs(fract(uv * 0.5) * 2.0 - 1.0);
  }
  return uv;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let d = sample1(in.uv);
  // The offset re-centres the displacement map: a 0..1 map with offset -0.5
  // pushes in both directions instead of only one.
  let dx = channel(d, i32(U.p0.y + 0.5)) + U.p0.w;
  let dy = channel(d, i32(U.p0.z + 0.5)) + U.p0.w;

  let uv = in.uv + vec2<f32>(dx, dy) * U.p0.x;
  let mode = i32(U.p1.x + 0.5);
  if (mode == 0 && (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0)) {
    return vec4<f32>(0.0);
  }
  return sample0(extend_uv(uv, mode));
}
