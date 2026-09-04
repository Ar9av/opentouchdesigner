// Constant TOP — a flat colour.
// p0 = rgba

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  return U.p0;
}
