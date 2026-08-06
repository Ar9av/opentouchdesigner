//! The SOP operator table.
//!
//! Generators build points; filters map points to points. Keeping filters in
//! that shape is deliberate — it is the form that moves to a compute shader
//! without being rewritten (PLAN.md Phase 4, "GPU-first from day one").

use std::sync::OnceLock;

use otd_core::indexmap::IndexMap;
use otd_core::{Connector, EvalContext, Family, Node, OpDef, OpRegistry, Param, Value};

use crate::{Geometry, Point, Topology};

pub struct SopCtx<'a> {
    pub node: &'a Node,
    pub eval: &'a EvalContext<'a>,
    pub inputs: Vec<Geometry>,
}

impl SopCtx<'_> {
    fn val(&self, key: &str) -> Value {
        self.node
            .param(key)
            .map(|p| p.eval(self.eval))
            .unwrap_or(Value::Float(0.0))
    }
    fn f(&self, key: &str) -> f32 {
        self.val(key).as_f32()
    }
    fn i(&self, key: &str) -> i64 {
        self.val(key).as_i64()
    }
    fn v3(&self, key: &str) -> [f32; 3] {
        let v = self.val(key).as_vec4_f32();
        [v[0], v[1], v[2]]
    }
    fn v4(&self, key: &str) -> [f32; 4] {
        self.val(key).as_vec4_f32()
    }
    fn menu(&self, key: &str) -> usize {
        let Some(p) = self.node.param(key) else {
            return 0;
        };
        let chosen = p.eval(self.eval).as_str();
        p.menu
            .as_ref()
            .and_then(|m| m.iter().position(|i| *i == chosen))
            .unwrap_or(0)
    }
    fn input(&self, i: usize) -> Geometry {
        self.inputs.get(i).cloned().unwrap_or_default()
    }
}

pub struct SopSpec {
    pub def: OpDef,
    pub cook: fn(&mut SopCtx) -> Geometry,
}

macro_rules! params {
    ($($key:expr => $param:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m: IndexMap<String, Param> = IndexMap::new();
        $( m.insert($key.into(), $param); )*
        m
    }};
}

fn no_params() -> IndexMap<String, Param> {
    params! {}
}

// ------------------------------------------------------------------- box

fn params_box() -> IndexMap<String, Param> {
    params! {
        "size" => Param::xyz([1.0, 1.0, 1.0]).with_label("Size"),
        "center" => Param::xyz([0.0, 0.0, 0.0]).with_label("Center"),
    }
}

/// The six faces, each as two triangles with a flat normal.
fn cook_box(c: &mut SopCtx) -> Geometry {
    let s = c.v3("size");
    let o = c.v3("center");
    let (hx, hy, hz) = (s[0] * 0.5, s[1] * 0.5, s[2] * 0.5);

    // (normal, u axis, v axis) per face.
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ];

    let mut points = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (n, u, v) in faces {
        let base = points.len() as u32;
        let centre = [n[0] * hx, n[1] * hy, n[2] * hz];
        for (du, dv, uu, vv) in [
            (-1.0f32, -1.0f32, 0.0f32, 0.0f32),
            (1.0, -1.0, 1.0, 0.0),
            (1.0, 1.0, 1.0, 1.0),
            (-1.0, 1.0, 0.0, 1.0),
        ] {
            points.push(Point {
                position: [
                    o[0] + centre[0] + u[0] * du * hx + v[0] * dv * hx,
                    o[1] + centre[1] + u[1] * du * hy + v[1] * dv * hy,
                    o[2] + centre[2] + u[2] * du * hz + v[2] * dv * hz,
                ],
                normal: n,
                uv: [uu, vv],
                color: [1.0; 4],
            });
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Geometry {
        points,
        indices,
        topology: Topology::Triangles,
    }
}

// ---------------------------------------------------------------- sphere

fn params_sphere() -> IndexMap<String, Param> {
    params! {
        "radius" => Param::float(0.5).with_label("Radius").with_range(0.01, 10.0),
        "rows" => Param::int(16).with_label("Rows").with_range(3.0, 128.0),
        "columns" => Param::int(24).with_label("Columns").with_range(3.0, 256.0),
        "center" => Param::xyz([0.0, 0.0, 0.0]).with_label("Center"),
    }
}

fn cook_sphere(c: &mut SopCtx) -> Geometry {
    let r = c.f("radius");
    let rows = c.i("rows").clamp(3, 128) as usize;
    let cols = c.i("columns").clamp(3, 256) as usize;
    let o = c.v3("center");

    let mut points = Vec::with_capacity((rows + 1) * (cols + 1));
    for row in 0..=rows {
        let v = row as f32 / rows as f32;
        let phi = v * std::f32::consts::PI;
        for col in 0..=cols {
            let u = col as f32 / cols as f32;
            let theta = u * std::f32::consts::TAU;
            let n = [phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin()];
            points.push(Point {
                position: [o[0] + n[0] * r, o[1] + n[1] * r, o[2] + n[2] * r],
                normal: n,
                uv: [u, v],
                color: [1.0; 4],
            });
        }
    }

    let stride = cols + 1;
    let mut indices = Vec::with_capacity(rows * cols * 6);
    for row in 0..rows {
        for col in 0..cols {
            let a = (row * stride + col) as u32;
            let b = a + stride as u32;
            indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    Geometry {
        points,
        indices,
        topology: Topology::Triangles,
    }
}

// ------------------------------------------------------------------ grid

fn params_grid() -> IndexMap<String, Param> {
    params! {
        "size" => Param::new(Value::Vec2([2.0, 2.0])).with_label("Size"),
        "rows" => Param::int(10).with_label("Rows").with_range(2.0, 512.0),
        "columns" => Param::int(10).with_label("Columns").with_range(2.0, 512.0),
        "orientation" => Param::menu("xy", &["xy", "xz", "yz"]).with_label("Orientation"),
    }
}

fn cook_grid(c: &mut SopCtx) -> Geometry {
    let s = c.v4("size");
    let rows = c.i("rows").clamp(2, 512) as usize;
    let cols = c.i("columns").clamp(2, 512) as usize;
    let orient = c.menu("orientation");

    let place = |a: f32, b: f32| -> ([f32; 3], [f32; 3]) {
        match orient {
            1 => ([a, 0.0, b], [0.0, 1.0, 0.0]),
            2 => ([0.0, a, b], [1.0, 0.0, 0.0]),
            _ => ([a, b, 0.0], [0.0, 0.0, 1.0]),
        }
    };

    let mut points = Vec::with_capacity(rows * cols);
    for row in 0..rows {
        let v = row as f32 / (rows - 1) as f32;
        for col in 0..cols {
            let u = col as f32 / (cols - 1) as f32;
            let (position, normal) = place((u - 0.5) * s[0], (v - 0.5) * s[1]);
            points.push(Point {
                position,
                normal,
                uv: [u, v],
                color: [1.0; 4],
            });
        }
    }

    let mut indices = Vec::with_capacity((rows - 1) * (cols - 1) * 6);
    for row in 0..rows - 1 {
        for col in 0..cols - 1 {
            let a = (row * cols + col) as u32;
            let b = a + cols as u32;
            indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    Geometry {
        points,
        indices,
        topology: Topology::Triangles,
    }
}

// ------------------------------------------------------------------ line

fn params_line() -> IndexMap<String, Param> {
    params! {
        "from" => Param::xyz([-1.0, 0.0, 0.0]).with_label("From"),
        "to" => Param::xyz([1.0, 0.0, 0.0]).with_label("To"),
        "points" => Param::int(2).with_label("Points").with_range(2.0, 4096.0),
    }
}

fn cook_line(c: &mut SopCtx) -> Geometry {
    let a = c.v3("from");
    let b = c.v3("to");
    let n = c.i("points").clamp(2, 4096) as usize;
    let points = (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32;
            Point {
                position: [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ],
                uv: [t, 0.0],
                ..Default::default()
            }
        })
        .collect();
    Geometry {
        points,
        indices: Vec::new(),
        topology: Topology::Lines,
    }
}

// ------------------------------------------------------------- transform

fn params_transform() -> IndexMap<String, Param> {
    params! {
        "translate" => Param::xyz([0.0, 0.0, 0.0]).with_label("Translate"),
        "rotate" => Param::xyz([0.0, 0.0, 0.0]).with_label("Rotate (deg)"),
        "scale" => Param::xyz([1.0, 1.0, 1.0]).with_label("Scale"),
    }
}

/// Rotate a vector by XYZ Euler angles in degrees.
pub fn rotate_xyz(v: [f32; 3], deg: [f32; 3]) -> [f32; 3] {
    let (rx, ry, rz) = (
        deg[0].to_radians(),
        deg[1].to_radians(),
        deg[2].to_radians(),
    );
    let (sx, cx) = rx.sin_cos();
    let (sy, cy) = ry.sin_cos();
    let (sz, cz) = rz.sin_cos();
    // X, then Y, then Z.
    let (y, z) = (v[1] * cx - v[2] * sx, v[1] * sx + v[2] * cx);
    let (x, z) = (v[0] * cy + z * sy, -v[0] * sy + z * cy);
    let (x, y) = (x * cz - y * sz, x * sz + y * cz);
    [x, y, z]
}

fn cook_transform(c: &mut SopCtx) -> Geometry {
    let t = c.v3("translate");
    let r = c.v3("rotate");
    let s = c.v3("scale");
    c.input(0).map_points(|_, p| {
        let scaled = [
            p.position[0] * s[0],
            p.position[1] * s[1],
            p.position[2] * s[2],
        ];
        let rotated = rotate_xyz(scaled, r);
        Point {
            position: [rotated[0] + t[0], rotated[1] + t[1], rotated[2] + t[2]],
            // Normals rotate but do not translate or scale.
            normal: rotate_xyz(p.normal, r),
            ..*p
        }
    })
}

// ----------------------------------------------------------------- noise

fn params_noise() -> IndexMap<String, Param> {
    params! {
        "amplitude" => Param::float(0.2).with_label("Amplitude").with_range(0.0, 4.0),
        "period" => Param::float(1.0).with_label("Period").with_range(0.01, 10.0),
        "offset" => Param::xyz([0.0, 0.0, 0.0]).with_label("Offset"),
        "along" => Param::menu("normal", &["normal", "xyz"]).with_label("Displace Along"),
    }
}

fn hash3(x: i32, y: i32, z: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9e3779b9)
        ^ (y as u32).wrapping_mul(0x85ebca6b)
        ^ (z as u32).wrapping_mul(0xc2b2ae35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb352d);
    h ^= h >> 15;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

fn value_noise(p: [f32; 3]) -> f32 {
    let i = [p[0].floor(), p[1].floor(), p[2].floor()];
    let f = [p[0] - i[0], p[1] - i[1], p[2] - i[2]];
    let u = [
        f[0] * f[0] * (3.0 - 2.0 * f[0]),
        f[1] * f[1] * (3.0 - 2.0 * f[1]),
        f[2] * f[2] * (3.0 - 2.0 * f[2]),
    ];
    let mut acc = 0.0;
    for (dz, wz) in [(0, 1.0 - u[2]), (1, u[2])] {
        for (dy, wy) in [(0, 1.0 - u[1]), (1, u[1])] {
            for (dx, wx) in [(0, 1.0 - u[0]), (1, u[0])] {
                let h = hash3(i[0] as i32 + dx, i[1] as i32 + dy, i[2] as i32 + dz);
                acc += h * wx * wy * wz;
            }
        }
    }
    acc
}

fn cook_noise(c: &mut SopCtx) -> Geometry {
    let amp = c.f("amplitude");
    let period = c.f("period").max(1e-4);
    let off = c.v3("offset");
    let along_normal = c.menu("along") == 0;

    c.input(0).map_points(|_, p| {
        let sample = [
            p.position[0] / period + off[0],
            p.position[1] / period + off[1],
            p.position[2] / period + off[2],
        ];
        let d = if along_normal {
            let n = value_noise(sample) * amp;
            [p.normal[0] * n, p.normal[1] * n, p.normal[2] * n]
        } else {
            [
                value_noise(sample) * amp,
                value_noise([sample[0] + 17.3, sample[1] + 5.1, sample[2] + 91.7]) * amp,
                value_noise([sample[0] + 41.9, sample[1] + 63.2, sample[2] + 7.4]) * amp,
            ]
        };
        Point {
            position: [
                p.position[0] + d[0],
                p.position[1] + d[1],
                p.position[2] + d[2],
            ],
            ..*p
        }
    })
}

// ---------------------------------------------------------------- colour

fn params_color() -> IndexMap<String, Param> {
    params! {
        "color" => Param::rgba([1.0, 1.0, 1.0, 1.0]).with_label("Color"),
    }
}

fn cook_color(c: &mut SopCtx) -> Geometry {
    let rgba = c.v4("color");
    c.input(0).map_points(|_, p| Point { color: rgba, ..*p })
}

// ----------------------------------------------------------------- merge

fn cook_merge(c: &mut SopCtx) -> Geometry {
    c.input(0).merged(&c.input(1))
}

// ------------------------------------------------------------------ copy

fn params_copy() -> IndexMap<String, Param> {
    params! {
        "count" => Param::int(3).with_label("Copies").with_range(1.0, 512.0),
        "translate" => Param::xyz([1.0, 0.0, 0.0]).with_label("Translate Each"),
        "rotate" => Param::xyz([0.0, 0.0, 0.0]).with_label("Rotate Each (deg)"),
        "scale" => Param::xyz([1.0, 1.0, 1.0]).with_label("Scale Each"),
    }
}

/// Stamp copies out with a compounding transform.
///
/// This is the CPU version of what instancing does on the GPU; it exists for
/// the familiar mental model and for small counts. Anything large should be
/// instanced instead, which is why the Geometry COMP takes its transforms
/// from a CHOP rather than from geometry duplicated here.
fn cook_copy(c: &mut SopCtx) -> Geometry {
    let n = c.i("count").clamp(1, 512) as usize;
    let t = c.v3("translate");
    let r = c.v3("rotate");
    let s = c.v3("scale");
    let source = c.input(0);

    let mut out = Geometry {
        topology: source.topology,
        ..Default::default()
    };
    for copy in 0..n {
        let k = copy as f32;
        let scale = [s[0].powf(k), s[1].powf(k), s[2].powf(k)];
        let rot = [r[0] * k, r[1] * k, r[2] * k];
        let offset = [t[0] * k, t[1] * k, t[2] * k];
        let stamped = source.map_points(|_, p| {
            let scaled = [
                p.position[0] * scale[0],
                p.position[1] * scale[1],
                p.position[2] * scale[2],
            ];
            let rotated = rotate_xyz(scaled, rot);
            Point {
                position: [
                    rotated[0] + offset[0],
                    rotated[1] + offset[1],
                    rotated[2] + offset[2],
                ],
                normal: rotate_xyz(p.normal, rot),
                ..*p
            }
        });
        out = out.merged(&stamped);
    }
    out
}

// ---------------------------------------------------------------- null

fn cook_null(c: &mut SopCtx) -> Geometry {
    c.input(0)
}

// ------------------------------------------------------------- the table

pub const NULL: &str = "nullSOP";
pub const IN: &str = "inSOP";
pub const OUT: &str = "outSOP";

fn spec(
    type_name: &'static str,
    label: &'static str,
    inputs: &'static [&'static str],
    summary: &'static str,
    params: fn() -> IndexMap<String, Param>,
    cook: fn(&mut SopCtx) -> Geometry,
) -> SopSpec {
    SopSpec {
        def: OpDef {
            type_name,
            label,
            family: Family::Sop,
            inputs,
            summary,
            time_dependent: false,
            params,
            connector: Connector::None,
        },
        cook,
    }
}

fn specs() -> &'static Vec<SopSpec> {
    static SPECS: OnceLock<Vec<SopSpec>> = OnceLock::new();
    SPECS.get_or_init(|| {
        let mut v = vec![
            spec(
                "boxSOP",
                "Box",
                &[],
                "A box with flat-shaded faces.",
                params_box,
                cook_box,
            ),
            spec(
                "sphereSOP",
                "Sphere",
                &[],
                "A UV sphere.",
                params_sphere,
                cook_sphere,
            ),
            spec(
                "gridSOP",
                "Grid",
                &[],
                "A flat grid of quads — the usual thing to displace.",
                params_grid,
                cook_grid,
            ),
            spec(
                "lineSOP",
                "Line",
                &[],
                "A run of points between two positions.",
                params_line,
                cook_line,
            ),
            spec(
                "transformSOP",
                "Transform",
                &["in"],
                "Translate, rotate and scale points.",
                params_transform,
                cook_transform,
            ),
            spec(
                "noiseSOP",
                "Noise",
                &["in"],
                "Displace points by value noise, along their normals or freely.",
                params_noise,
                cook_noise,
            ),
            spec(
                "colorSOP",
                "Color",
                &["in"],
                "Set the colour carried by every point.",
                params_color,
                cook_color,
            ),
            spec(
                "mergeSOP",
                "Merge",
                &["a", "b"],
                "Combine two pieces of geometry.",
                no_params,
                cook_merge,
            ),
            spec(
                "copySOP",
                "Copy",
                &["in"],
                "Stamp copies with a compounding transform.",
                params_copy,
                cook_copy,
            ),
            spec(
                NULL,
                "Null",
                &["in"],
                "Pass-through. A stable name to reference.",
                no_params,
                cook_null,
            ),
        ];
        let mut in_sop = spec(
            IN,
            "In",
            &[],
            "A geometry input on this component's node.",
            no_params,
            cook_null,
        );
        in_sop.def.connector = Connector::In;
        v.push(in_sop);
        let mut out_sop = spec(
            OUT,
            "Out",
            &["in"],
            "This component's geometry output.",
            no_params,
            cook_null,
        );
        out_sop.def.connector = Connector::Out;
        v.push(out_sop);
        v
    })
}

pub fn spec_for(type_name: &str) -> Option<&'static SopSpec> {
    specs().iter().find(|s| s.def.type_name == type_name)
}

pub fn all() -> impl Iterator<Item = &'static SopSpec> {
    specs().iter()
}

pub fn registry() -> OpRegistry {
    let mut r = OpRegistry::new();
    for s in specs() {
        r.register(s.def.clone());
    }
    r
}
