// Fit TOP — resample the input into a different resolution and aspect.
// p0 = (mode: 0 fill / 1 fit / 2 stretch / 3 native, 0, 0, 0)
// p1 = the background colour, seen wherever `fit` leaves a gap
//
// `fill` crops the overflowing axis, `fit` letterboxes, `stretch` distorts and
// `native` keeps the source's pixel size centred. The source's size comes from
// the texture itself rather than from a parameter, so the node keeps working
// when whatever is upstream changes resolution.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let mode = i32(U.p0.x + 0.5);
  if (mode == 2) {
    return sample0(in.uv);
  }

  let size = vec2<f32>(textureDimensions(tex0));
  let dst = U.res.x / max(U.res.y, 1.0);
  let src = size.x / max(size.y, 1.0);

  var scale = vec2<f32>(1.0);
  if (mode == 3) {
    scale = U.res.xy / max(size, vec2<f32>(1.0));
  } else {
    // uv is scaled about the centre, so a factor above 1 samples beyond the
    // edge of the source and shows it smaller. `fit` shrinks until the long
    // axis is inside; `fill` grows until the short axis covers, and crops.
    let wider = src > dst;
    let fill = (mode == 0);
    if (wider == fill) {
      scale.x = dst / src;
    } else {
      scale.y = src / dst;
    }
  }

  let uv = (in.uv - 0.5) * scale + 0.5;
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
    return U.p1;
  }
  return sample0(uv);
}
