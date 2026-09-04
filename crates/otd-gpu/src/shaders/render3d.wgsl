// The 3D pipeline: instanced geometry, one directional light, a
// Lambert-plus-Blinn shading model with a metallic/roughness feel.
//
// Instancing is per-vertex-buffer rather than per-draw: PLAN.md calls
// texture-based instancing "the signature TD trick", and the shape of it is
// that one geometry is drawn thousands of times with per-instance transforms
// coming from somewhere else in the network — a CHOP's channels or a TOP's
// pixels. Both end up in the same instance buffer.

struct Scene {
  view_proj: mat4x4<f32>,
  model: mat4x4<f32>,
  // xyz = camera position, w = unused
  camera: vec4<f32>,
  // The camera's axes in world space, for billboarding a point sprite.
  camera_right: vec4<f32>,
  camera_up: vec4<f32>,
  // xyz = direction *towards* the light, w = intensity
  light_dir: vec4<f32>,
  light_color: vec4<f32>,
  base_color: vec4<f32>,
  // What these mean depends on the shading model:
  //   PBR      x = metallic,  y = roughness,  z = emit, w = use texture
  //   Constant                                z = emit, w = use texture
  //   Phong    x = specular,  y = shininess,  z = emit, w = use texture
  //   Sprite   x = size,      y = round,      z = emit, w = use texture
  material: vec4<f32>,
  // x = ambient, y = point scale, z = shading model, w unused
  render: vec4<f32>,
};

@group(0) @binding(0) var<uniform> S: Scene;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex: texture_2d<f32>;

struct VIn {
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) uv: vec2<f32>,
  @location(3) color: vec4<f32>,
  // Per-instance, from the instance buffer.
  @location(4) i_translate: vec3<f32>,
  @location(5) i_scale: vec3<f32>,
  @location(6) i_rotate: vec3<f32>,
  @location(7) i_color: vec4<f32>,
};

struct VOut {
  @builtin(position) clip: vec4<f32>,
  @location(0) world: vec3<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) uv: vec2<f32>,
  @location(3) color: vec4<f32>,
};

fn rotate_xyz(v: vec3<f32>, deg: vec3<f32>) -> vec3<f32> {
  let r = radians(deg);
  let sx = sin(r.x); let cx = cos(r.x);
  let sy = sin(r.y); let cy = cos(r.y);
  let sz = sin(r.z); let cz = cos(r.z);
  var p = vec3<f32>(v.x, v.y * cx - v.z * sx, v.y * sx + v.z * cx);
  p = vec3<f32>(p.x * cy + p.z * sy, p.y, -p.x * sy + p.z * cy);
  return vec3<f32>(p.x * cz - p.y * sz, p.x * sz + p.y * cz, p.z);
}

@vertex
fn vs_main(in: VIn) -> VOut {
  // Instance transform first, then the object's own model matrix.
  let scaled = in.position * in.i_scale;
  let rotated = rotate_xyz(scaled, in.i_rotate);
  let local = rotated + in.i_translate;
  var world = S.model * vec4<f32>(local, 1.0);

  // A point sprite's four corners share a position and differ only in uv, so
  // the quad is built here, facing the camera, from the uv corner.
  if (i32(S.render.z + 0.5) == 3) {
    let corner = in.uv * 2.0 - vec2<f32>(1.0);
    let half = S.material.x * 0.5;
    world = vec4<f32>(
      world.xyz + S.camera_right.xyz * corner.x * half
                + S.camera_up.xyz * corner.y * half,
      1.0);
  }

  let n = normalize(rotate_xyz(in.normal, in.i_rotate));
  let world_n = normalize((S.model * vec4<f32>(n, 0.0)).xyz);

  var out: VOut;
  out.clip = S.view_proj * world;
  out.world = world.xyz;
  out.normal = world_n;
  out.uv = in.uv;
  out.color = in.color * in.i_color;
  return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  var albedo = S.base_color * in.color;
  if (S.material.w > 0.5) {
    albedo = albedo * textureSample(tex, samp, in.uv);
  }

  let model = i32(S.render.z + 0.5);

  // Point sprite: unlit, and optionally cut to a disc. Rounding in the
  // fragment stage rather than with more geometry is what keeps a sprite one
  // quad however smooth its edge looks.
  if (model == 3) {
    var alpha = albedo.a;
    if (S.material.y > 0.5) {
      let d = length(in.uv * 2.0 - vec2<f32>(1.0));
      // One pixel of feather, in uv units, so the edge is antialiased at any
      // sprite size on screen.
      alpha = alpha * (1.0 - smoothstep(1.0 - fwidth(d) * 2.0, 1.0, d));
      if (alpha <= 0.0) { discard; }
    }
    return vec4<f32>(albedo.rgb * max(S.material.z, 0.0), alpha);
  }

  // Constant: the colour is the colour. Not "PBR with the light turned off" —
  // that would still fold in ambient and the emit term twice. This is what a
  // Wireframe MAT and anything already lit want.
  if (model == 1) {
    return vec4<f32>(albedo.rgb * max(S.material.z, 0.0), albedo.a);
  }

  let n = normalize(in.normal);
  let l = normalize(S.light_dir.xyz);
  let v = normalize(S.camera.xyz - in.world);
  let h = normalize(l + v);
  let diffuse = max(dot(n, l), 0.0) * S.light_dir.w;

  var spec = 0.0;
  var spec_tint = vec3<f32>(1.0);
  if (model == 2) {
    // Phong: shininess is dialled directly, and the specular level is its own
    // number rather than being implied by roughness. Less physical than PBR
    // and easier to get a specific look out of, which is the whole reason
    // the operator exists alongside it.
    spec = pow(max(dot(n, h), 0.0), max(S.material.y, 1.0)) * S.material.x;
  } else {
    // PBR: roughness drives the highlight width; metals keep their colour in
    // the specular, dielectrics go white. Not a full BRDF, but it reads
    // correctly and costs almost nothing.
    let roughness = clamp(S.material.y, 0.05, 1.0);
    let shininess = 2.0 / (roughness * roughness) - 2.0;
    spec = pow(max(dot(n, h), 0.0), shininess) * (1.0 - roughness);
    spec_tint = mix(vec3<f32>(1.0), albedo.rgb, S.material.x);
  }

  let lit = albedo.rgb * (S.render.x + diffuse * S.light_color.rgb)
          + spec_tint * spec * S.light_color.rgb * S.light_dir.w;
  return vec4<f32>(lit + albedo.rgb * S.material.z, albedo.a);
}
