// Luma Level TOP — brightness, contrast and gamma on luminance alone.
// p0 = (brightness, contrast, gamma, opacity)
// p1 = (black level, white level, 0, 0)
//
// A Level TOP works per channel, which shifts hue as soon as the channels are
// unequal. This one moves the luminance and rescales the colour to match, so
// a saturated red gets brighter instead of getting pinker.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = sample0(in.uv);
  let y = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));

  var v = clamp((y - U.p1.x) / max(U.p1.y - U.p1.x, 1e-5), 0.0, 1.0);
  v = pow(v, 1.0 / max(U.p0.z, 1e-3));
  v = (v - 0.5) * U.p0.y + 0.5;
  v = v * U.p0.x;

  // Scale rather than offset: an offset washes saturation out, a scale keeps
  // the ratios between the channels and so keeps the hue.
  let rgb = c.rgb * (v / max(y, 1e-4));
  return vec4<f32>(mix(c.rgb, rgb, clamp(U.p0.w, 0.0, 1.0)), c.a);
}
