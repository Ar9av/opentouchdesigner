// Tile TOP — repeat the image across the frame.
// p0 = (repeat x, repeat y, offset x, offset y)
// p1 = (mirror, row shift, column shift, 0)
//
// Mirror flips alternate copies, which makes any image tile seamlessly at the
// cost of a visible axis of symmetry. Row Shift offsets every other row — a
// half shift is a brick bond, and is what stops a tiling from reading as a
// grid.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let n = max(vec2<f32>(U.p0.x, U.p0.y), vec2<f32>(1e-3));
  var p = in.uv * n + vec2<f32>(U.p0.z, U.p0.w) * n;

  let cell = floor(p);
  p.x = p.x + U.p1.y * (fract(cell.y * 0.5) * 2.0);
  p.y = p.y + U.p1.z * (fract(cell.x * 0.5) * 2.0);

  var uv = fract(p);
  if (U.p1.x > 0.5) {
    let odd = fract(floor(p) * 0.5) * 2.0;
    uv = mix(uv, 1.0 - uv, odd);
  }
  return sample0(uv);
}
