// Channel Mix TOP — a 4x4 matrix over RGBA.
// p0 = the row that makes red, p1 = green, p2 = blue, p3 = alpha.
//
// Each row is (from R, from G, from B, from A). The identity is the default,
// so this operator does nothing until a number is moved.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = sample0(in.uv);
  return vec4<f32>(dot(U.p0, c), dot(U.p1, c), dot(U.p2, c), dot(U.p3, c));
}
