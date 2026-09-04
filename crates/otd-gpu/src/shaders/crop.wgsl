// Crop TOP — keep a rectangle of the source, at an explicit output size.
// p0 = (left, right, bottom, top) as fractions of the source
//
// The kept rectangle is resampled to Resolution W/H. That is a crop *and* a
// resize in one node; for a crop that keeps the source's pixel scale, set the
// resolution to the region's size in pixels.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let l = min(U.p0.x, U.p0.y);
  let r = max(U.p0.x, U.p0.y);
  let b = min(U.p0.z, U.p0.w);
  let t = max(U.p0.z, U.p0.w);

  // v is measured from the bottom in the parameters and from the top in uv.
  let u = mix(l, r, in.uv.x);
  let v = 1.0 - mix(b, t, 1.0 - in.uv.y);
  return sample0(vec2<f32>(u, v));
}
