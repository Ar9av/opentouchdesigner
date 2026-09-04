// Math TOP — arithmetic on two inputs, or on one and a constant.
// p0 = (operation, 0, 0, 0)
// p1 = pre-multiply for input 1
// p2 = pre-multiply for input 2
// p3 = post-add

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let a = sample0(in.uv) * U.p1;
  let b = sample1(in.uv) * U.p2;
  let op = i32(U.p0.x + 0.5);

  var r = a + b;
  if (op == 1) { r = a - b; }
  if (op == 2) { r = a * b; }
  if (op == 3) { r = a / max(abs(b), vec4<f32>(1e-5)) * sign(b + vec4<f32>(1e-9)); }
  if (op == 4) { r = min(a, b); }
  if (op == 5) { r = max(a, b); }
  if (op == 6) { r = abs(a - b); }
  if (op == 7) { r = pow(max(a, vec4<f32>(0.0)), max(b, vec4<f32>(1e-5))); }
  return r + U.p3;
}
