// Text TOP.
// p0 = text colour, p1 = background colour
//
// The glyph coverage arrives in the uploaded texture's alpha; this tints it and
// puts it over the background. Splitting it this way means the colour is a
// live parameter — changing it costs a uniform write, not a re-rasterise.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let coverage = sample0(in.uv).a;
  let ink = vec4<f32>(U.p0.rgb, U.p0.a) * coverage;
  // Over, premultiplied: the glyph is already multiplied by its own coverage.
  return ink + U.p1 * (1.0 - ink.a);
}
