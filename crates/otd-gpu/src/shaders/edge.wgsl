// Edge TOP — a Sobel gradient magnitude.
// p0 = (strength, width in px, mode, keep colour)
// p1 = edge colour

fn luma(c: vec3<f32>) -> f32 {
  return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  // The step is in pixels, so the same Width looks the same at 720p and 4K.
  let step = U.res.zw * max(U.p0.y, 0.0);

  // Sobel: the 3x3 taps in one go. Nine samples is cheap next to the
  // separable trick, and keeping it in one pass keeps the operator a single
  // table entry.
  var gx = 0.0;
  var gy = 0.0;
  for (var j = -1; j <= 1; j = j + 1) {
    for (var i = -1; i <= 1; i = i + 1) {
      let l = luma(sample0(in.uv + vec2<f32>(f32(i), f32(j)) * step).rgb);
      let wx = f32(i) * select(1.0, 2.0, j == 0);
      let wy = f32(j) * select(1.0, 2.0, i == 0);
      gx = gx + l * wx;
      gy = gy + l * wy;
    }
  }

  var e = sqrt(gx * gx + gy * gy) * U.p0.x;
  let mode = i32(U.p0.z + 0.5);
  if (mode == 1) { e = abs(gx) * U.p0.x; }
  if (mode == 2) { e = abs(gy) * U.p0.x; }
  e = clamp(e, 0.0, 1.0);

  let src = sample0(in.uv);
  // Keep Colour tints the edges with what was underneath them, which is how
  // an edge pass stays usable as a look rather than only as a matte.
  let tint = mix(U.p1.rgb, src.rgb, clamp(U.p0.w, 0.0, 1.0));
  return vec4<f32>(tint * e, e * U.p1.a);
}
