// Ramp TOP.
// p0 = (type: 0 horizontal / 1 vertical / 2 radial, phase, 0, 0)
// p1 = colour 1 rgba
// p2 = colour 2 rgba

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let kind = i32(U.p0.x + 0.5);
  var t: f32;
  if (kind == 1) {
    t = in.uv.y;
  } else if (kind == 2) {
    let aspect = U.res.x / max(U.res.y, 1.0);
    let d = vec2<f32>((in.uv.x - 0.5) * aspect, in.uv.y - 0.5);
    t = clamp(length(d) * 2.0, 0.0, 1.0);
  } else {
    t = in.uv.x;
  }
  t = fract(t + U.p0.y);
  return mix(U.p1, U.p2, t);
}
