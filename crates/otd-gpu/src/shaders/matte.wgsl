// Matte TOP — colour from input 1, alpha from input 2.
// p0 = (source: 0 alpha / 1 luminance / 2 red, invert, premultiply, 0)

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = sample0(in.uv);
  let m = sample1(in.uv);

  let mode = i32(U.p0.x + 0.5);
  var a = m.a;
  if (mode == 1) { a = dot(m.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)); }
  if (mode == 2) { a = m.r; }
  a = clamp(mix(a, 1.0 - a, clamp(U.p0.y, 0.0, 1.0)), 0.0, 1.0);

  // Premultiplying is off by default: it is what you want when the result is
  // about to be composited, and wrong when it is about to be looked at.
  let rgb = select(c.rgb, c.rgb * a, U.p0.z > 0.5);
  return vec4<f32>(rgb, c.a * a);
}
