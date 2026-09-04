// Monochrome TOP.
// p0 = (mode, opacity, 0, 0)
//
// modes: 0 luminance, 1 average, 2 maximum, 3 minimum, 4 red, 5 green,
//        6 blue, 7 alpha

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = sample0(in.uv);
  let mode = i32(U.p0.x + 0.5);

  // Rec.709 by default, because an unweighted average reads as the wrong
  // brightness on anything with saturated colour in it.
  var g = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
  if (mode == 1) { g = (c.r + c.g + c.b) / 3.0; }
  if (mode == 2) { g = max(max(c.r, c.g), c.b); }
  if (mode == 3) { g = min(min(c.r, c.g), c.b); }
  if (mode == 4) { g = c.r; }
  if (mode == 5) { g = c.g; }
  if (mode == 6) { g = c.b; }
  if (mode == 7) { g = c.a; }

  return vec4<f32>(mix(c.rgb, vec3<f32>(g), clamp(U.p0.y, 0.0, 1.0)), c.a);
}
