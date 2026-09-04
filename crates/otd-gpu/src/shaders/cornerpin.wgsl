// Corner Pin TOP — drag the four corners of the image anywhere.
// p0 = (bottom-left x, y, bottom-right x, y)
// p1 = (top-left x, y, top-right x, y)
// p2 = (extend, 0, 0, 0)
//
// This is the projection-mapping operator: the four corners are where the
// image's corners should land, in 0..1 of the output, and the perspective in
// between follows. A plain Transform cannot do it — the mapping is a
// homography, not an affine, which is why straight lines stay straight but
// evenly spaced ones stop being evenly spaced.

fn extend_uv(uv: vec2<f32>, mode: i32) -> vec2<f32> {
  if (mode == 1) { return clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)); }
  if (mode == 2) { return fract(uv); }
  if (mode == 3) { return 1.0 - abs(fract(uv * 0.5) * 2.0 - 1.0); }
  return uv;
}

fn inverse3(m: mat3x3<f32>) -> mat3x3<f32> {
  let a = m[0]; let b = m[1]; let c = m[2];
  let r0 = cross(b, c);
  let r1 = cross(c, a);
  let r2 = cross(a, b);
  let det = dot(a, r0);
  let inv = 1.0 / select(det, 1e-8, abs(det) < 1e-8);
  // rows of the adjugate become columns of the inverse
  return mat3x3<f32>(
    vec3<f32>(r0.x, r1.x, r2.x),
    vec3<f32>(r0.y, r1.y, r2.y),
    vec3<f32>(r0.z, r1.z, r2.z),
  ) * inv;
}

// The homography taking the unit square's corners to four arbitrary points.
fn square_to_quad(p00: vec2<f32>, p10: vec2<f32>, p01: vec2<f32>, p11: vec2<f32>) -> mat3x3<f32> {
  let d1 = p10 - p11;
  let d2 = p01 - p11;
  let s  = p00 - p10 + p11 - p01;
  let den = d1.x * d2.y - d2.x * d1.y;
  let inv = 1.0 / select(den, 1e-8, abs(den) < 1e-8);
  let g = (s.x * d2.y - d2.x * s.y) * inv;
  let h = (d1.x * s.y - s.x * d1.y) * inv;
  return mat3x3<f32>(
    vec3<f32>(p10.x - p00.x + g * p10.x, p10.y - p00.y + g * p10.y, g),
    vec3<f32>(p01.x - p00.x + h * p01.x, p01.y - p00.y + h * p01.y, h),
    vec3<f32>(p00.x, p00.y, 1.0),
  );
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  // Corner parameters are given bottom-up, the way the editor draws them.
  let bl = vec2<f32>(U.p0.x, 1.0 - U.p0.y);
  let br = vec2<f32>(U.p0.z, 1.0 - U.p0.w);
  let tl = vec2<f32>(U.p1.x, 1.0 - U.p1.y);
  let tr = vec2<f32>(U.p1.z, 1.0 - U.p1.w);

  // Source uv (0,0) is top-left, so the square's (0,0) corner is `tl`.
  let h = inverse3(square_to_quad(tl, tr, bl, br));
  let q = h * vec3<f32>(in.uv, 1.0);
  if (abs(q.z) < 1e-6) { return vec4<f32>(0.0); }
  let uv = q.xy / q.z;

  let mode = i32(U.p2.x + 0.5);
  if (mode == 0 && (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0)) {
    return vec4<f32>(0.0);
  }
  return sample0(extend_uv(uv, mode));
}
