// Circle TOP.
// p0 = (centre x, centre y, radius x, radius y)
// p1 = fill colour
// p2 = background colour
// p3 = (softness, aspect corrected, 0, 0)

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  var d = in.uv - vec2<f32>(U.p0.x, U.p0.y);
  // Aspect correction makes a circle a circle rather than an ellipse on a
  // 16:9 canvas. Off by default would surprise; on by default is the shape
  // the parameter is named after.
  if (U.p3.y > 0.5) { d.x = d.x * (U.res.x / max(U.res.y, 1.0)); }
  let r = vec2<f32>(max(U.p0.z, 1e-5), max(U.p0.w, 1e-5));
  let dist = length(d / r);

  // One pixel of softness by default, in units of the distance field, so the
  // edge is antialiased at whatever resolution the node is running at.
  let soft = max(U.p3.x, U.res.z / max(r.x, 1e-5));
  let t = 1.0 - smoothstep(1.0 - soft, 1.0 + soft, dist);
  return mix(U.p2, U.p1, t);
}
