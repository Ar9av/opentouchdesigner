// Transform TOP.
// p0 = (translate.x, translate.y, rotate radians, 0)
// p1 = (scale.x, scale.y, extend: 0 zero / 1 hold / 2 repeat / 3 mirror, 0)

fn extend_uv(uv: vec2<f32>, mode: i32) -> vec2<f32> {
  if (mode == 1) {
    return clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
  } else if (mode == 2) {
    return fract(uv);
  } else if (mode == 3) {
    let t = abs(fract(uv * 0.5) * 2.0 - 1.0);
    return 1.0 - t;
  }
  return uv;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let aspect = U.res.x / max(U.res.y, 1.0);
  // Work in centred, aspect-corrected space so rotation isn't sheared.
  var p = vec2<f32>((in.uv.x - 0.5) * aspect, in.uv.y - 0.5);

  p = p - vec2<f32>(U.p0.x * aspect, -U.p0.y);

  let c = cos(-U.p0.z);
  let s = sin(-U.p0.z);
  p = vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);

  p = p / vec2<f32>(max(abs(U.p1.x), 1e-5), max(abs(U.p1.y), 1e-5));

  var uv = vec2<f32>(p.x / aspect + 0.5, p.y + 0.5);
  let mode = i32(U.p1.z + 0.5);
  let outside = uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0;
  if (mode == 0 && outside) {
    return vec4<f32>(0.0);
  }
  return sample0(extend_uv(uv, mode));
}
