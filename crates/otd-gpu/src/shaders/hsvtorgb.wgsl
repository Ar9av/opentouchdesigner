// HSV to RGB TOP — the inverse of the RGB to HSV TOP.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let c = sample0(in.uv);
  let h = fract(c.r) * 6.0;
  let i = floor(h);
  let f = h - i;
  let v = c.b;
  let s = clamp(c.g, 0.0, 1.0);
  let p = v * (1.0 - s);
  let q = v * (1.0 - s * f);
  let t = v * (1.0 - s * (1.0 - f));
  let n = i32(i);
  var rgb = vec3<f32>(v, p, q);
  if (n == 0) { rgb = vec3<f32>(v, t, p); }
  else if (n == 1) { rgb = vec3<f32>(q, v, p); }
  else if (n == 2) { rgb = vec3<f32>(p, v, t); }
  else if (n == 3) { rgb = vec3<f32>(p, q, v); }
  else if (n == 4) { rgb = vec3<f32>(t, p, v); }
  return vec4<f32>(rgb, c.a);
}
