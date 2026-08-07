// Flip TOP.
// p0 = (flip x, flip y, transpose, 0)

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  var uv = in.uv;
  // Transpose first: flipping then transposing is not the same as
  // transposing then flipping, and this order is the one that reads as
  // "swap the axes, then mirror".
  if (U.p0.z > 0.5) { uv = uv.yx; }
  if (U.p0.x > 0.5) { uv.x = 1.0 - uv.x; }
  if (U.p0.y > 0.5) { uv.y = 1.0 - uv.y; }
  return sample0(uv);
}
