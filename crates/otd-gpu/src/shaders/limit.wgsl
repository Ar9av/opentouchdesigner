// Limit TOP — clamp and/or quantise.
// p0 = (mode: 0 clamp / 1 quantise / 2 both, minimum, maximum, step)
// p1 = (include alpha, 0, 0, 0)
//
// Quantising is what makes posterised and colour-indexed looks; a Dither TOP
// does the same reduction but hides the banding, which is usually what you
// actually want. Clamping matters upstream of anything that will exponentiate.

fn limit(v: vec4<f32>, mode: i32, lo: f32, hi: f32, step: f32) -> vec4<f32> {
  var x = v;
  if (mode != 1) { x = clamp(x, vec4<f32>(lo), vec4<f32>(hi)); }
  if (mode != 0) {
    let s = max(step, 1e-5);
    x = floor(x / s + 0.5) * s;
  }
  return x;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = sample0(in.uv);
  let out = limit(c, i32(U.p0.x + 0.5), U.p0.y, U.p0.z, U.p0.w);
  return vec4<f32>(out.rgb, select(c.a, out.a, U.p1.x > 0.5));
}
