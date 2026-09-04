// Remap TOP — input 2's red and green are the coordinates to read input 1 at.
// p0 = (amount, extend, flip v, 0)
//
// Displace offsets the lookup; this replaces it. A Ramp in both axes into the
// second input is the identity, and every warp is a modification of that —
// which makes it the operator to reach for when the mapping comes from
// somewhere else (a calibration pass, a UV render, a baked distortion).

fn extend_uv(uv: vec2<f32>, mode: i32) -> vec2<f32> {
  if (mode == 1) { return clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)); }
  if (mode == 2) { return fract(uv); }
  if (mode == 3) { return 1.0 - abs(fract(uv * 0.5) * 2.0 - 1.0); }
  return uv;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let m = sample1(in.uv);
  var uv = m.rg;
  if (U.p0.z > 0.5) { uv.y = 1.0 - uv.y; }
  uv = mix(in.uv, uv, clamp(U.p0.x, 0.0, 1.0));

  let mode = i32(U.p0.y + 0.5);
  if (mode == 0 && (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0)) {
    return vec4<f32>(0.0);
  }
  return sample0(extend_uv(uv, mode));
}
