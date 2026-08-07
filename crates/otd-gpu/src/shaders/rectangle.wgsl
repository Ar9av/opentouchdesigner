// Rectangle TOP.
// p0 = (centre x, centre y, half width, half height)
// p1 = fill colour
// p2 = background colour
// p3 = (softness, corner radius, border width, border colour index)
// The border colour shares p1's alpha slot rather than taking a fifth vec4;
// four vec4s is the whole uniform budget (see common.wgsl).

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let p = in.uv - vec2<f32>(U.p0.x, U.p0.y);
  let half = vec2<f32>(max(U.p0.z, 0.0), max(U.p0.w, 0.0));
  let radius = min(U.p3.y, min(half.x, half.y));

  // Rounded-box signed distance: inset by the corner radius, measure to the
  // inset box, then subtract it back.
  let q = abs(p) - (half - vec2<f32>(radius));
  let dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;

  let soft = max(U.p3.x, U.res.z);
  let inside = 1.0 - smoothstep(-soft, soft, dist);
  var col = mix(U.p2, U.p1, inside);

  // A border is the band just inside the edge, which falls out of the same
  // distance field for free.
  if (U.p3.z > 0.0) {
    let band = smoothstep(-U.p3.z - soft, -U.p3.z + soft, dist) * inside;
    col = mix(col, vec4<f32>(U.p3.w, U.p3.w, U.p3.w, U.p1.a), band);
  }
  return col;
}
