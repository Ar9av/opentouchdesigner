// RGB to HSV TOP — hue, saturation and value land in r, g and b.
//
// The point is to get at those channels with the ordinary maths operators: a
// Level or a Lookup on the red channel is now a hue operation. Convert back
// with an HSV to RGB TOP.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = max(sample0(in.uv), vec4<f32>(0.0));
  let mx = max(max(c.r, c.g), c.b);
  let mn = min(min(c.r, c.g), c.b);
  let d = mx - mn;
  var h = 0.0;
  if (d > 1e-6) {
    if (mx == c.r)      { h = (c.g - c.b) / d; }
    else if (mx == c.g) { h = 2.0 + (c.b - c.r) / d; }
    else                { h = 4.0 + (c.r - c.g) / d; }
    h = fract(h / 6.0 + 1.0);
  }
  let s = select(0.0, d / mx, mx > 1e-6);
  return vec4<f32>(h, s, mx, c.a);
}
