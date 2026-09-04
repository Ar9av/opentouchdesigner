// Noise TOP — fractal value noise.
//
// p0 = (period, harmonics, roughness, exponent)
// p1 = (translate.x, translate.y, translate.z, monochrome)
// p2 = (amplitude, offset, 0, 0)

// Integer bit-mixing hash. The usual `fract(sin(dot(...)))` trick correlates
// badly on a lattice and shows up as square blocking in the fbm, which is
// exactly what a noise generator must not do.
fn hash_u(x: u32) -> u32 {
  var h = x;
  h ^= h >> 16u;
  h *= 0x7feb352du;
  h ^= h >> 15u;
  h *= 0x846ca68bu;
  h ^= h >> 16u;
  return h;
}

fn hash3(p: vec3<f32>) -> f32 {
  let i = vec3<i32>(p);
  let h = hash_u(bitcast<u32>(i.x) ^ hash_u(bitcast<u32>(i.y) ^ hash_u(bitcast<u32>(i.z) ^ 0x9e3779b9u)));
  return f32(h) * (1.0 / 4294967296.0);
}

fn vnoise(p: vec3<f32>) -> f32 {
  let i = floor(p);
  let f = fract(p);
  // Quintic smoothstep — continuous second derivative, so fbm doesn't band.
  let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);

  let n000 = hash3(i + vec3<f32>(0.0, 0.0, 0.0));
  let n100 = hash3(i + vec3<f32>(1.0, 0.0, 0.0));
  let n010 = hash3(i + vec3<f32>(0.0, 1.0, 0.0));
  let n110 = hash3(i + vec3<f32>(1.0, 1.0, 0.0));
  let n001 = hash3(i + vec3<f32>(0.0, 0.0, 1.0));
  let n101 = hash3(i + vec3<f32>(1.0, 0.0, 1.0));
  let n011 = hash3(i + vec3<f32>(0.0, 1.0, 1.0));
  let n111 = hash3(i + vec3<f32>(1.0, 1.0, 1.0));

  let x00 = mix(n000, n100, u.x);
  let x10 = mix(n010, n110, u.x);
  let x01 = mix(n001, n101, u.x);
  let x11 = mix(n011, n111, u.x);
  return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

fn fbm(p: vec3<f32>, harmonics: i32, roughness: f32) -> f32 {
  var sum = 0.0;
  var amp = 1.0;
  var norm = 0.0;
  var q = p;
  for (var i = 0; i < harmonics; i = i + 1) {
    sum = sum + amp * vnoise(q);
    norm = norm + amp;
    amp = amp * roughness;
    q = q * 2.0;
  }
  return sum / max(norm, 1e-6);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let period = max(U.p0.x, 1e-4);
  let harmonics = clamp(i32(U.p0.y), 1, 10);
  let roughness = clamp(U.p0.z, 0.0, 1.0);
  let exponent = max(U.p0.w, 1e-3);
  let aspect = U.res.x / max(U.res.y, 1.0);

  // Keep noise square regardless of output aspect.
  var p = vec3<f32>((in.uv.x - 0.5) * aspect, in.uv.y - 0.5, 0.0) / period;
  p = p + U.p1.xyz;

  let mono = U.p1.w > 0.5;
  var rgb: vec3<f32>;
  if (mono) {
    let n = fbm(p, harmonics, roughness);
    rgb = vec3<f32>(n);
  } else {
    rgb = vec3<f32>(
      fbm(p, harmonics, roughness),
      fbm(p + vec3<f32>(37.2, 11.5, 5.1), harmonics, roughness),
      fbm(p + vec3<f32>(91.7, 63.3, 19.9), harmonics, roughness),
    );
  }

  rgb = pow(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(exponent));
  rgb = rgb * U.p2.x + U.p2.y;
  return vec4<f32>(rgb, 1.0);
}
