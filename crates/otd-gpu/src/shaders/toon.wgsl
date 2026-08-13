// Toon TOP — flatten the shading into bands and ink the edges.
// p0 = (bands, edge strength, edge width in px, saturation)
// p1 = ink colour
//
// Cel shading is two separate ideas that only look like one. Posterising the
// *luminance* — and only the luminance — collapses a smooth gradient into a
// few flat steps while leaving hue alone, which is what keeps the result
// looking painted rather than colour-crushed. The ink is a Sobel edge
// multiplied over the top. Doing both in one operator is worth it because the
// two want to share a threshold: ink drawn where the bands already change is
// invisible, and ink drawn everywhere else is a mess.

fn luma(c: vec3<f32>) -> f32 {
  return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let src = sample0(in.uv);

  // ---- bands, on luminance only
  let bands = max(U.p0.x, 2.0);
  let l = luma(src.rgb);
  let stepped = floor(l * bands) / (bands - 1.0);
  // Scale the original colour to the new luminance rather than replacing it,
  // so a red stays red instead of turning grey at the band edges.
  let ratio = select(1.0, stepped / max(l, 0.0001), l > 0.0001);
  var rgb = src.rgb * ratio;

  // ---- saturation, because flattening luminance tends to wash colour out
  let grey = vec3<f32>(luma(rgb));
  rgb = mix(grey, rgb, max(U.p0.w, 0.0));

  // ---- ink
  let step_px = U.res.zw * max(U.p0.z, 0.0);
  var gx = 0.0;
  var gy = 0.0;
  for (var j = -1; j <= 1; j = j + 1) {
    for (var i = -1; i <= 1; i = i + 1) {
      let s = luma(sample0(in.uv + vec2<f32>(f32(i), f32(j)) * step_px).rgb);
      let wx = f32(i) * select(1.0, 2.0, j == 0);
      let wy = f32(j) * select(1.0, 2.0, i == 0);
      gx = gx + s * wx;
      gy = gy + s * wy;
    }
  }
  let e = clamp(sqrt(gx * gx + gy * gy) * U.p0.y, 0.0, 1.0);

  // Multiplied, not added: ink darkens what is under it. Adding an edge
  // colour makes the lines glow, which is the opposite of a drawn line.
  let inked = mix(rgb, U.p1.rgb, e * U.p1.a);

  return vec4<f32>(clamp(inked, vec3<f32>(0.0), vec3<f32>(1.0)), src.a);
}
