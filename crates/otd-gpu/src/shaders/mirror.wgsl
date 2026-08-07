// Mirror TOP — fold the image onto itself, the cheap kaleidoscope.
// p0 = (mode, segments, angle, centre-x)
// p1 = (centre-y, 0, 0, 0)

const PI: f32 = 3.14159265;

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let centre = vec2<f32>(U.p0.w, U.p1.x);
  var uv = in.uv;
  let mode = i32(U.p0.x + 0.5);

  if (mode == 0) {
    // Fold left onto right about the centre.
    uv.x = centre.x + abs(uv.x - centre.x);
  } else if (mode == 1) {
    uv.y = centre.y + abs(uv.y - centre.y);
  } else if (mode == 2) {
    uv = centre + abs(uv - centre);
  } else {
    // Radial: wrap the angle into one wedge and mirror alternate wedges, so
    // the seams line up instead of showing a hard cut.
    let d = uv - centre;
    let r = length(d);
    let segs = max(U.p0.y, 1.0);
    let wedge = 2.0 * PI / segs;
    var a = atan2(d.y, d.x) - U.p0.z;
    a = a - floor(a / wedge) * wedge;
    a = abs(a - wedge * 0.5);
    a = a + U.p0.z;
    uv = centre + vec2<f32>(cos(a), sin(a)) * r;
  }
  return sample0(clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
}
