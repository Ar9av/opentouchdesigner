// Dither TOP — quantise to few levels, and hide the banding with a pattern.
// p0 = (levels, pattern, strength, monochrome)
// p1 = (scale in px, 0, 0, 0)
//
// Quantising alone gives flat bands. The point of dithering is that a pattern
// added *before* the quantiser makes the rounding error alternate between
// neighbouring pixels, so the eye averages it back into the tone that was
// there. Which pattern you use is the whole look: Bayer is the ordered
// crosshatch of early games, blue-ish noise is film grain, and a plain
// threshold is the hard posterisation you get with no dither at all.

fn luma(c: vec3<f32>) -> f32 {
  return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The 4x4 recursive Bayer matrix, returned in 0..1 and centred on zero.
fn bayer4(p: vec2<i32>) -> f32 {
  let x = p.x & 3;
  let y = p.y & 3;
  // Generated rather than tabled: the recursive definition interleaves the
  // bits of x and (x ^ y), which is shorter than a 16-entry array and is the
  // same numbers.
  var v = 0;
  var xx = x;
  var yy = x ^ y;
  for (var i = 0; i < 2; i = i + 1) {
    v = (v << 2) | ((yy & 2) | ((xx & 2) >> 1));
    xx = xx << 1;
    yy = yy << 1;
  }
  return f32(v) / 16.0 - 0.5;
}

// 8x8 Bayer, built from the 4x4 the same way the 4x4 is built from the 2x2.
fn bayer8(p: vec2<i32>) -> f32 {
  let coarse = bayer4(p / 2) + 0.5;
  let fine = bayer4(p) + 0.5;
  return (coarse + fine / 16.0) / 1.0625 - 0.5;
}

fn hash21(p: vec2<f32>) -> f32 {
  var h = fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
  return h;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let src = sample0(in.uv);

  // The pattern is measured in output pixels, then optionally enlarged, so
  // "chunky dither" is a parameter rather than a resolution accident.
  let scale = max(U.p1.x, 1.0);
  let cell = vec2<i32>(floor(in.uv * U.res.xy / scale));

  let pattern = i32(U.p0.y + 0.5);
  var threshold = 0.0;
  if (pattern == 0) {
    threshold = bayer4(cell);
  } else if (pattern == 1) {
    threshold = bayer8(cell);
  } else if (pattern == 2) {
    threshold = hash21(vec2<f32>(cell)) - 0.5;
  }
  // pattern == 3 leaves threshold at zero: straight posterisation.

  let levels = max(U.p0.x, 2.0);
  let strength = clamp(U.p0.z, 0.0, 1.0);

  var rgb = src.rgb;
  if (U.p0.w > 0.5) {
    rgb = vec3<f32>(luma(rgb));
  }

  // The dither is scaled by the step between levels — bigger than that and
  // it stops being a dither and starts being noise laid over the picture.
  let step = 1.0 / (levels - 1.0);
  let dithered = rgb + threshold * step * strength;
  let out = floor(dithered * (levels - 1.0) + 0.5) / (levels - 1.0);

  return vec4<f32>(clamp(out, vec3<f32>(0.0), vec3<f32>(1.0)), src.a);
}
