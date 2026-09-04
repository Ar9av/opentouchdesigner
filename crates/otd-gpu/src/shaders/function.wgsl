// Function TOP — one unary function per pixel, with a scale either side.
// p0 = (function, pre multiply, pre add, post multiply)
// p1 = (post add, include alpha, 0, 0)
//
// functions: 0 none, 1 log, 2 exp, 3 sqrt, 4 square, 5 reciprocal, 6 invert,
//            7 sin, 8 cos, 9 atan, 10 abs, 11 sign

fn apply(x: f32, which: i32) -> f32 {
  if (which == 1) { return log(max(x, 1e-6)); }
  if (which == 2) { return exp(x); }
  if (which == 3) { return sqrt(max(x, 0.0)); }
  if (which == 4) { return x * x; }
  if (which == 5) { return 1.0 / select(x, 1e-6, abs(x) < 1e-6); }
  if (which == 6) { return 1.0 - x; }
  if (which == 7) { return sin(x); }
  if (which == 8) { return cos(x); }
  if (which == 9) { return atan(x); }
  if (which == 10) { return abs(x); }
  if (which == 11) { return sign(x); }
  return x;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = sample0(in.uv);
  let n = i32(U.p0.x + 0.5);

  var v = c * U.p0.y + U.p0.z;
  v = vec4<f32>(apply(v.x, n), apply(v.y, n), apply(v.z, n), apply(v.w, n));
  v = v * U.p0.w + U.p1.x;

  return vec4<f32>(v.rgb, select(c.a, v.a, U.p1.y > 0.5));
}
