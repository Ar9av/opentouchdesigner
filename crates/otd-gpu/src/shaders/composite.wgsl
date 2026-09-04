// Composite TOP — input 1 (tex1) composited over input 0 (tex0).
// p0 = (operation, opacity, 0, 0)
//
// operations: 0 over, 1 add, 2 multiply, 3 screen, 4 difference, 5 subtract,
//             6 maximum, 7 minimum

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let base = sample0(in.uv);
  var top = sample1(in.uv);
  top = top * U.p0.y;

  let op = i32(U.p0.x + 0.5);
  var rgb: vec3<f32>;
  var a: f32;

  if (op == 1) {
    rgb = base.rgb + top.rgb;
    a = max(base.a, top.a);
  } else if (op == 2) {
    rgb = base.rgb * top.rgb;
    a = max(base.a, top.a);
  } else if (op == 3) {
    rgb = 1.0 - (1.0 - base.rgb) * (1.0 - top.rgb);
    a = max(base.a, top.a);
  } else if (op == 4) {
    rgb = abs(base.rgb - top.rgb);
    a = max(base.a, top.a);
  } else if (op == 5) {
    rgb = base.rgb - top.rgb;
    a = max(base.a, top.a);
  } else if (op == 6) {
    rgb = max(base.rgb, top.rgb);
    a = max(base.a, top.a);
  } else if (op == 7) {
    rgb = min(base.rgb, top.rgb);
    a = min(base.a, top.a);
  } else {
    // Source-over, premultiplied by the incoming alpha.
    rgb = top.rgb * top.a + base.rgb * (1.0 - top.a);
    a = top.a + base.a * (1.0 - top.a);
  }

  return vec4<f32>(rgb, a);
}
