// Anti Alias TOP — FXAA, near enough.
// p0 = (strength, 0, 0, 0)
//
// A post-process pass: it finds luminance edges and blends along them. It
// cannot recover detail the render never had, so on a Render TOP it is a
// cheaper alternative to more samples, not a better one.

fn luma(uv: vec2<f32>) -> f32 {
  return dot(sample0(uv).rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let e = U.res.zw;
  let m  = luma(in.uv);
  let nw = luma(in.uv + vec2<f32>(-e.x, -e.y));
  let ne = luma(in.uv + vec2<f32>( e.x, -e.y));
  let sw = luma(in.uv + vec2<f32>(-e.x,  e.y));
  let se = luma(in.uv + vec2<f32>( e.x,  e.y));

  let lo = min(m, min(min(nw, ne), min(sw, se)));
  let hi = max(m, max(max(nw, ne), max(sw, se)));
  if (hi - lo < 0.02) {
    return sample0(in.uv);
  }

  var dir = vec2<f32>(-((nw + ne) - (sw + se)), (nw + sw) - (ne + se));
  let scale = 1.0 / (min(abs(dir.x), abs(dir.y)) + 0.03125);
  // Clamped to two texels: beyond that FXAA smears texture it should keep.
  dir = clamp(dir * scale, vec2<f32>(-2.0), vec2<f32>(2.0)) * e * max(U.p0.x, 0.0);

  let a = 0.5 * (sample0(in.uv + dir * (1.0 / 3.0 - 0.5))
               + sample0(in.uv + dir * (2.0 / 3.0 - 0.5)));
  let b = a * 0.5 + 0.25 * (sample0(in.uv - dir * 0.5) + sample0(in.uv + dir * 0.5));

  let bl = dot(b.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
  return select(b, a, bl < lo || bl > hi);
}
