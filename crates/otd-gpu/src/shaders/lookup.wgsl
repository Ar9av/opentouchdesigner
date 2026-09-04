// Lookup TOP — input 1 indexes input 2.
// p0 = (source channel, 0, 0, 0)
//
// The classic use is a gradient map: a greyscale image on input 1, a Ramp TOP
// on input 2, and every luminance becomes the colour at that position along
// the ramp. It is also how a curve becomes an operator without a curve
// widget — the ramp *is* the curve.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let src = sample0(in.uv);
  let mode = i32(U.p0.x + 0.5);
  var key = dot(src.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
  if (mode == 1) { key = src.r; }
  if (mode == 2) { key = src.g; }
  if (mode == 3) { key = src.b; }
  if (mode == 4) { key = src.a; }
  // Sample the middle row: a lookup table is a horizontal gradient, and
  // reading the centre avoids the edge clamp at v=0 or v=1.
  return sample1(vec2<f32>(clamp(key, 0.0, 1.0), 0.5));
}
