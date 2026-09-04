// RGB Key TOP — key on distance in RGB, rather than on hue.
// p0 = (tolerance, softness, output: 0 keyed / 1 matte / 2 inverted matte, 0)
// p1 = the key colour
//
// Chroma Key is the right tool for a lit greenscreen. This one is for flat
// synthetic images — a solid background out of a render, a logo on white —
// where the colour to remove is exact and hue is a worse discriminator than
// plain distance.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = sample0(in.uv);
  let d = distance(c.rgb, U.p1.rgb);
  let soft = max(U.p0.y, 1e-5);
  let keep = smoothstep(U.p0.x, U.p0.x + soft, d);

  let out = i32(U.p0.z + 0.5);
  if (out == 1) { return vec4<f32>(vec3<f32>(keep), 1.0); }
  if (out == 2) { return vec4<f32>(vec3<f32>(1.0 - keep), 1.0); }
  return vec4<f32>(c.rgb, c.a * keep);
}
