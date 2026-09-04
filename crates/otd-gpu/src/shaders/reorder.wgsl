// Reorder TOP — build each output channel from a channel of either input.
// p0 = (source for R, G, B, A)
//
// sources: 0..3 input 1 rgba, 4..7 input 2 rgba, 8 input 1 luma,
//          9 input 2 luma, 10 one, 11 zero

fn pick(a: vec4<f32>, b: vec4<f32>, which: f32) -> f32 {
  let n = i32(which + 0.5);
  if (n == 0) { return a.r; }
  if (n == 1) { return a.g; }
  if (n == 2) { return a.b; }
  if (n == 3) { return a.a; }
  if (n == 4) { return b.r; }
  if (n == 5) { return b.g; }
  if (n == 6) { return b.b; }
  if (n == 7) { return b.a; }
  if (n == 8) { return dot(a.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)); }
  if (n == 9) { return dot(b.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)); }
  if (n == 10) { return 1.0; }
  return 0.0;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let a = sample0(in.uv);
  let b = sample1(in.uv);
  return vec4<f32>(
    pick(a, b, U.p0.x),
    pick(a, b, U.p0.y),
    pick(a, b, U.p0.z),
    pick(a, b, U.p0.w),
  );
}
