// Slope TOP — the image's own gradient.
// p0 = (mode: 0 xy / 1 magnitude / 2 direction, strength, 0, 0)
//
// The `xy` output is the derivative packed as red = d/dx and green = d/dy,
// centred on 0.5 — which is exactly what a Displace TOP wants in its second
// input, so Noise -> Slope -> Displace warps a picture along the noise.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = sample0(in.uv);
  let e = U.res.zw;
  let l = dot(sample0(in.uv - vec2<f32>(e.x, 0.0)).rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
  let r = dot(sample0(in.uv + vec2<f32>(e.x, 0.0)).rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
  let d = dot(sample0(in.uv - vec2<f32>(0.0, e.y)).rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
  let u = dot(sample0(in.uv + vec2<f32>(0.0, e.y)).rgb, vec3<f32>(0.2126, 0.7152, 0.0722));

  let g = vec2<f32>(r - l, u - d) * 0.5 * U.p0.y;
  let mode = i32(U.p0.x + 0.5);
  if (mode == 1) { return vec4<f32>(vec3<f32>(length(g)), c.a); }
  if (mode == 2) { return vec4<f32>(vec3<f32>(fract(atan2(g.y, g.x) / 6.2831853 + 1.0)), c.a); }
  return vec4<f32>(g.x + 0.5, g.y + 0.5, 0.0, c.a);
}
