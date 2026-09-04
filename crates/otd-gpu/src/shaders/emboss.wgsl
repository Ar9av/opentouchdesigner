// Emboss TOP — a directional difference, sat at mid grey.
// p0 = (direction in radians, width in pixels, strength, mix with source)

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = sample0(in.uv);
  let d = vec2<f32>(cos(U.p0.x), sin(U.p0.x)) * U.p0.y * U.res.zw;

  let a = sample0(in.uv + d).rgb;
  let b = sample0(in.uv - d).rgb;
  let relief = (a - b) * U.p0.z + 0.5;

  return vec4<f32>(mix(relief, c.rgb, clamp(U.p0.w, 0.0, 1.0)), c.a);
}
