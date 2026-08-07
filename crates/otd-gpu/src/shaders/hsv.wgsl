// HSV Adjust TOP.
// p0 = (hue offset in turns, saturation, value, contrast about mid)
// p1 = (hue range centre, hue range width, 0, 0)

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
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
  return vec3<f32>(h, s, mx);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
  let h = fract(c.x) * 6.0;
  let i = floor(h);
  let f = h - i;
  let p = c.z * (1.0 - c.y);
  let q = c.z * (1.0 - c.y * f);
  let t = c.z * (1.0 - c.y * (1.0 - f));
  let n = i32(i);
  if (n == 0) { return vec3<f32>(c.z, t, p); }
  if (n == 1) { return vec3<f32>(q, c.z, p); }
  if (n == 2) { return vec3<f32>(p, c.z, t); }
  if (n == 3) { return vec3<f32>(p, q, c.z); }
  if (n == 4) { return vec3<f32>(t, p, c.z); }
  return vec3<f32>(c.z, p, q);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let src = sample0(in.uv);
  var hsv = rgb_to_hsv(max(src.rgb, vec3<f32>(0.0)));

  // A hue range limits the adjustment to one band of the wheel — a selective
  // colour correction, which is most of what this operator gets used for.
  // A width of 1 (the default) covers the whole wheel and costs nothing.
  var mask = 1.0;
  if (U.p1.y < 0.999) {
    let d = abs(fract(hsv.x - U.p1.x + 0.5) - 0.5);   // shortest way round
    mask = 1.0 - smoothstep(U.p1.y * 0.5, U.p1.y * 0.5 + 0.05, d);
  }

  hsv.x = fract(hsv.x + U.p0.x * mask);
  hsv.y = clamp(hsv.y * mix(1.0, U.p0.y, mask), 0.0, 1.0);
  hsv.z = hsv.z * mix(1.0, U.p0.z, mask);
  var rgb = hsv_to_rgb(hsv);
  rgb = (rgb - 0.5) * U.p0.w + 0.5;
  return vec4<f32>(rgb, src.a);
}
