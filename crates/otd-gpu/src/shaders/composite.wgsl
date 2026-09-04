// Composite TOP — input 1 (tex1) composited over input 0 (tex0).
// p0 = (operation, opacity, 0, 0)
//
// operations: 0 over, 1 add, 2 multiply, 3 screen, 4 difference, 5 subtract,
//             6 maximum, 7 minimum, 8 under, 9 inside, 10 outside, 11 cross

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
  } else if (op == 8) {
    // Under: the same as Over with the inputs swapped, which saves a wire.
    rgb = base.rgb * base.a + top.rgb * (1.0 - base.a);
    a = base.a + top.a * (1.0 - base.a);
  } else if (op == 9) {
    // Inside: input 2 kept only where input 1 is opaque.
    rgb = top.rgb;
    a = top.a * base.a;
  } else if (op == 10) {
    rgb = top.rgb;
    a = top.a * (1.0 - base.a);
  } else if (op == 11) {
    // Cross (xor): each input survives where the other is not.
    rgb = top.rgb * top.a * (1.0 - base.a) + base.rgb * base.a * (1.0 - top.a);
    a = top.a * (1.0 - base.a) + base.a * (1.0 - top.a);
  } else {
    // Source-over, premultiplied by the incoming alpha.
    rgb = top.rgb * top.a + base.rgb * (1.0 - top.a);
    a = top.a + base.a * (1.0 - top.a);
  }

  return vec4<f32>(rgb, a);
}
