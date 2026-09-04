// Flow TOP — push the picture along a swirling vector field.
// p0 = (amount in px, scale, speed, source)
// p1 = (curl amount, 0, 0, 0)
//
// One step of advection: each pixel reads from where the picture *was* a
// moment ago, upstream of the flow. One step on its own is a warp. Put it in
// a feedback loop — flow1 -> feedback targeting it — and the steps accumulate
// into smoke, ink in water, the whole family of looks that "flow" means in a
// node graph.
//
// The field is curl noise, which matters: the curl of a scalar field is
// divergence-free, so it swirls without ever compressing the image into a
// point or tearing a hole in it. A plain noise vector field does both, and
// looks like the picture is being eaten.

fn hash21(p: vec2<f32>) -> f32 {
  return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn value_noise(p: vec2<f32>) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let u = f * f * (3.0 - 2.0 * f);
  let a = hash21(i);
  let b = hash21(i + vec2<f32>(1.0, 0.0));
  let c = hash21(i + vec2<f32>(0.0, 1.0));
  let d = hash21(i + vec2<f32>(1.0, 1.0));
  return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Two octaves is enough to stop the swirl reading as one big rotation, and
// cheap enough to run every frame inside a feedback loop.
fn fbm(p: vec2<f32>) -> f32 {
  return value_noise(p) * 0.65 + value_noise(p * 2.03 + 11.7) * 0.35;
}

// Curl of the scalar field: rotate its gradient by ninety degrees.
fn curl(p: vec2<f32>) -> vec2<f32> {
  let e = 0.05;
  let dx = fbm(p + vec2<f32>(e, 0.0)) - fbm(p - vec2<f32>(e, 0.0));
  let dy = fbm(p + vec2<f32>(0.0, e)) - fbm(p - vec2<f32>(0.0, e));
  return vec2<f32>(dy, -dx) / (2.0 * e);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  let aspect = U.res.x / max(U.res.y, 1.0);
  let p = vec2<f32>(in.uv.x * aspect, in.uv.y) * max(U.p0.y, 0.001);
  let t = U.time.x * U.p0.z;

  var dir = curl(p + vec2<f32>(t, -t * 0.7));

  // The second input, when something is wired to it, steers the flow: red and
  // green are read as a vector field around zero. This is what lets one Flow
  // TOP be driven by a Ramp, by noise, or by a camera difference — the field
  // does not have to be the one built in.
  if (U.p0.w > 0.5) {
    let field = sample1(in.uv).rg * 2.0 - 1.0;
    dir = mix(dir, field, clamp(U.p1.x, 0.0, 1.0));
  }

  // Sample upstream: reading from where the picture came *from* is what makes
  // the content appear to travel downstream.
  let offset = dir * U.p0.x * U.res.zw;
  return sample0(in.uv - offset);
}
