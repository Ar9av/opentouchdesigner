// Voronoi TOP — cellular noise, as cells, edges or distance.
// p0 = (scale, speed, jitter, output)
// p1 = colour 1, p2 = colour 2
//
// A generator: nothing is wired in. Every pixel finds the nearest of a set of
// points scattered one per grid cell, and what you do with that answer is the
// `output` parameter. `cells` flat-fills each region a colour, which is the
// stained-glass look; `edges` draws where two regions meet, which is the
// cracked-glass one; `distance` is the raw field, and is what you want when
// this is feeding a Displace TOP rather than being looked at.

fn hash22(p: vec2<f32>) -> vec2<f32> {
  let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
  return fract(sin(q) * 43758.5453);
}

fn hash21(p: vec2<f32>) -> f32 {
  return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  // Square the cells regardless of the output's aspect, or a 16:9 canvas
  // gives stretched hexagons and looks like a bug.
  let aspect = U.res.x / max(U.res.y, 1.0);
  let uv = vec2<f32>(in.uv.x * aspect, in.uv.y) * max(U.p0.x, 0.001);

  let t = U.time.x * U.p0.y;
  let jitter = clamp(U.p0.z, 0.0, 1.0);

  let base = floor(uv);
  let frac = uv - base;

  // Nearest and second-nearest. The second is what makes edges possible:
  // the boundary between two regions is where the two distances are equal,
  // so their difference is a distance *to the edge* and antialiases for free.
  var nearest = 8.0;
  var second = 8.0;
  var nearest_cell = vec2<f32>(0.0);

  for (var j = -1; j <= 1; j = j + 1) {
    for (var i = -1; i <= 1; i = i + 1) {
      let cell = base + vec2<f32>(f32(i), f32(j));
      // Each cell's point wanders on its own phase, so the pattern boils
      // rather than sliding as one sheet.
      let rnd = hash22(cell);
      let offset = 0.5 + jitter * 0.5 * sin(t + 6.2831 * rnd);
      let point = vec2<f32>(f32(i), f32(j)) + offset - frac;
      let d = length(point);
      if (d < nearest) {
        second = nearest;
        nearest = d;
        nearest_cell = cell;
      } else if (d < second) {
        second = d;
      }
    }
  }

  let mode = i32(U.p0.w + 0.5);
  var v = 0.0;
  if (mode == 0) {
    // Flat fill: one value per region, stable because it is hashed from the
    // cell rather than from the distance.
    v = hash21(nearest_cell + 0.5);
  } else if (mode == 1) {
    // Distance between the two nearest points is zero exactly on the border.
    v = clamp((second - nearest) * 2.0, 0.0, 1.0);
  } else {
    v = clamp(nearest, 0.0, 1.0);
  }

  let col = mix(U.p1, U.p2, v);
  return vec4<f32>(col.rgb, col.a);
}
