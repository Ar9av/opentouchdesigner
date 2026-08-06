// Level TOP.
// p0 = (brightness, contrast, gamma, opacity)
// p1 = (black level, white level, invert, 0)

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  var c = sample0(in.uv);
  var rgb = c.rgb;

  // Black/white level remap first, so gamma acts on the remapped range.
  let black = U.p1.x;
  let white = U.p1.y;
  rgb = (rgb - black) / max(white - black, 1e-5);

  rgb = (rgb - 0.5) * U.p0.y + 0.5;   // contrast about mid grey
  rgb = rgb * U.p0.x;                  // brightness
  rgb = pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / max(U.p0.z, 1e-3)));
  rgb = mix(rgb, 1.0 - rgb, clamp(U.p1.z, 0.0, 1.0));

  return vec4<f32>(rgb, c.a) * U.p0.w;
}
