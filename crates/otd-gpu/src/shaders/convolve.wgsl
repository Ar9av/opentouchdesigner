// Convolve TOP — an arbitrary 3x3 kernel.
// p0 = (k00, k01, k02, spread in pixels)
// p1 = (k10, k11, k12, scale)
// p2 = (k20, k21, k22, offset)
// p3 = (normalise, 0, 0, 0)
//
// Sharpen, edge detect, box blur and every embossing variant are one kernel
// each. Normalise divides by the sum of the weights, which is what keeps a
// blur kernel from also brightening the picture.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let s = U.res.zw * max(U.p0.w, 0.0);
  var k = array<f32, 9>(
    U.p0.x, U.p0.y, U.p0.z,
    U.p1.x, U.p1.y, U.p1.z,
    U.p2.x, U.p2.y, U.p2.z,
  );

  var sum = vec4<f32>(0.0);
  var total = 0.0;
  for (var j = 0; j < 3; j = j + 1) {
    for (var i = 0; i < 3; i = i + 1) {
      let w = k[j * 3 + i];
      let o = vec2<f32>(f32(i - 1), f32(j - 1)) * s;
      sum = sum + w * sample0(in.uv + o);
      total = total + w;
    }
  }

  if (U.p3.x > 0.5 && abs(total) > 1e-5) {
    sum = sum / total;
  }
  let out = sum * U.p1.w + U.p2.w;
  return vec4<f32>(out.rgb, sample0(in.uv).a);
}
