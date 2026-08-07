// Chroma Key TOP.
// p0 = key colour rgb, p0.w = tolerance
// p1 = (softness, despill, replace mode, 0)
// p2 = replacement colour

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let src = sample0(in.uv);

  // Chrominance distance, not RGB distance: a green screen is unevenly lit,
  // and matching on brightness as well would key the bright half and keep
  // the shadowed half. Dropping luma is what makes a lit screen keyable.
  let l_src = dot(src.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
  let l_key = dot(U.p0.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
  let d = length((src.rgb - l_src) - (U.p0.rgb - l_key));

  let tol = max(U.p0.w, 1e-5);
  let soft = max(U.p1.x, 1e-5);
  // 0 where the pixel matches the key, 1 where it does not.
  let alpha = smoothstep(tol, tol + soft, d);

  var rgb = src.rgb;
  // Despill: pull the keyed hue out of what survives, so a subject lit by
  // bounce off the screen does not keep a green rim.
  if (U.p1.y > 0.0) {
    let spill = max(dot(normalize(max(U.p0.rgb, vec3<f32>(1e-5))), src.rgb - l_src), 0.0);
    rgb = rgb - normalize(max(U.p0.rgb, vec3<f32>(1e-5))) * spill * U.p1.y;
  }

  if (U.p1.z > 0.5) {
    // Replace rather than cut out, for when there is no compositor after.
    return vec4<f32>(mix(U.p2.rgb, rgb, alpha), max(src.a, U.p2.a));
  }
  return vec4<f32>(rgb, src.a * alpha);
}
