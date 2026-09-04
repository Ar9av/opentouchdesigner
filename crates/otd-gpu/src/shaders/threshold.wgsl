// Threshold TOP.
// p0 = (threshold, softness, mode, invert)
// p1 = below colour
// p2 = above colour

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = sample0(in.uv);

  // Which scalar the comparison is against. Luminance is the useful default;
  // alpha matters when the thing being thresholded is a mask.
  let mode = i32(U.p0.z + 0.5);
  var key = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
  if (mode == 1) { key = max(max(c.r, c.g), c.b); }
  if (mode == 2) { key = c.a; }

  // A hard step aliases badly at any resolution; softness widens the edge
  // into a smoothstep so it can be antialiased or used as a soft matte.
  let soft = max(U.p0.y, 1e-5);
  var t = smoothstep(U.p0.x - soft * 0.5, U.p0.x + soft * 0.5, key);
  t = mix(t, 1.0 - t, clamp(U.p0.w, 0.0, 1.0));

  return mix(U.p1, U.p2, t);
}
