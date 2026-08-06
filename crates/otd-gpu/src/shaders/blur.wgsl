// Blur TOP — separable Gaussian. The engine runs this pipeline twice, once
// horizontally into a scratch texture and once vertically into the output.
//
// p0 = (radius in pixels, direction: 0 = horizontal / 1 = vertical, 0, 0)

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let radius = max(U.p0.x, 0.0);
  if (radius < 0.5) {
    return sample0(in.uv);
  }
  var dir = vec2<f32>(U.res.z, 0.0);
  if (U.p0.y > 0.5) {
    dir = vec2<f32>(0.0, U.res.w);
  }

  // Fixed tap count with a widening step: constant cost, and a large radius
  // degrades to a soft box rather than to a visible ring.
  let taps = 16;
  let sigma = radius * 0.5;
  let step = radius / f32(taps);

  var sum = sample0(in.uv);
  var weight = 1.0;
  for (var i = 1; i <= taps; i = i + 1) {
    let o = f32(i) * step;
    let w = exp(-(o * o) / (2.0 * sigma * sigma + 1e-6));
    sum = sum + w * (sample0(in.uv + dir * o) + sample0(in.uv - dir * o));
    weight = weight + 2.0 * w;
  }
  return sum / weight;
}
