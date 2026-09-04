// Lens Distort TOP — barrel and pincushion, and the correction for them.
// p0 = (k1, k2, scale, extend)
// p1 = (centre x, centre y, 0, 0)
//
// Positive k1 is pincushion, negative is barrel — the fisheye look, and the
// one a GoPro needs undone. k2 is the higher-order term that matters only at
// the very edge of a wide lens. Scale zooms afterwards, which is how you get
// rid of the black corners a barrel correction leaves.

fn extend_uv(uv: vec2<f32>, mode: i32) -> vec2<f32> {
  if (mode == 1) { return clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)); }
  if (mode == 2) { return fract(uv); }
  if (mode == 3) { return 1.0 - abs(fract(uv * 0.5) * 2.0 - 1.0); }
  return uv;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let aspect = U.res.x / max(U.res.y, 1.0);
  let centre = vec2<f32>(U.p1.x, 1.0 - U.p1.y);
  var p = (in.uv - centre) * vec2<f32>(aspect, 1.0) / max(U.p0.z, 1e-4);

  let r2 = dot(p, p);
  p = p * (1.0 + U.p0.x * r2 + U.p0.y * r2 * r2);

  let uv = p / vec2<f32>(aspect, 1.0) + centre;
  let mode = i32(U.p0.w + 0.5);
  if (mode == 0 && (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0)) {
    return vec4<f32>(0.0);
  }
  return sample0(extend_uv(uv, mode));
}
