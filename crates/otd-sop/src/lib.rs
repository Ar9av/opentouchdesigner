//! `otd-sop` — the SOP (surface operator) family: geometry.
//!
//! PLAN.md §5 Phase 4 is blunt about the strategy: geometry should be
//! **GPU-first**, because TD's SOPs are CPU-bound and reproducing that would
//! be inheriting the weakness rather than the idea. What that means in
//! practice here:
//!
//!  * Geometry is a **flat, interleaved buffer** — positions, normals, UVs
//!    and colours in one `Vec`, shaped exactly like the vertex buffer the
//!    renderer wants. There is no conversion step at upload, and a future
//!    compute-shader operator can write this layout directly.
//!  * Operators are **per-point functions**, not mesh surgery. Transform,
//!    Noise and Colour all map a point to a point, which is the form that
//!    ports to a compute shader unchanged.
//!  * The small CPU generator set (Box, Sphere, Grid, Line) exists for the
//!    familiar mental model, and because something has to make the first
//!    points.

pub mod ops;

use otd_core::{CookContext, CookError, Cooker, EvalContext, Family, Graph, NodeId};
use slotmap::SecondaryMap;

/// One vertex, in the layout the renderer consumes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl Default for Point {
    fn default() -> Self {
        Point {
            position: [0.0; 3],
            normal: [0.0, 1.0, 0.0],
            uv: [0.0; 2],
            color: [1.0; 4],
        }
    }
}

impl Point {
    pub fn at(x: f32, y: f32, z: f32) -> Self {
        Point {
            position: [x, y, z],
            ..Default::default()
        }
    }
}

/// How the points are meant to be assembled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Topology {
    #[default]
    Triangles,
    Lines,
    Points,
}

/// A SOP's output.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub points: Vec<Point>,
    /// Indices into `points`. Empty means the points are already in order.
    pub indices: Vec<u32>,
    pub topology: Topology,
}

impl Geometry {
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn num_points(&self) -> usize {
        self.points.len()
    }

    /// How many vertices a draw call would emit.
    pub fn num_vertices(&self) -> usize {
        if self.indices.is_empty() {
            self.points.len()
        } else {
            self.indices.len()
        }
    }

    pub fn num_primitives(&self) -> usize {
        let per = match self.topology {
            Topology::Triangles => 3,
            Topology::Lines => 2,
            Topology::Points => 1,
        };
        self.num_vertices() / per
    }

    /// Axis-aligned bounds, for framing a camera on the geometry.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in &self.points {
            for i in 0..3 {
                lo[i] = lo[i].min(p.position[i]);
                hi[i] = hi[i].max(p.position[i]);
            }
        }
        if self.points.is_empty() {
            return ([0.0; 3], [0.0; 3]);
        }
        (lo, hi)
    }

    /// Apply a function to every point. This is the shape every SOP filter
    /// takes, and the shape that ports to a compute shader unchanged.
    pub fn map_points(&self, mut f: impl FnMut(usize, &Point) -> Point) -> Geometry {
        Geometry {
            points: self
                .points
                .iter()
                .enumerate()
                .map(|(i, p)| f(i, p))
                .collect(),
            indices: self.indices.clone(),
            topology: self.topology,
        }
    }

    /// Concatenate, shifting the second one's indices.
    pub fn merged(&self, other: &Geometry) -> Geometry {
        if self.is_empty() {
            return other.clone();
        }
        if other.is_empty() {
            return self.clone();
        }
        let offset = self.points.len() as u32;
        let mut points = self.points.clone();
        points.extend_from_slice(&other.points);

        // A geometry with no index list is implicitly 0..n; make that explicit
        // when merging, or the two halves would run together.
        let mut indices = if self.indices.is_empty() {
            (0..offset).collect::<Vec<u32>>()
        } else {
            self.indices.clone()
        };
        if other.indices.is_empty() {
            indices.extend((0..other.points.len() as u32).map(|i| i + offset));
        } else {
            indices.extend(other.indices.iter().map(|i| i + offset));
        }
        Geometry {
            points,
            indices,
            topology: self.topology,
        }
    }

    /// The interleaved bytes a vertex buffer wants.
    pub fn vertex_bytes(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.points.len() * 12);
        for p in &self.points {
            out.extend_from_slice(&p.position);
            out.extend_from_slice(&p.normal);
            out.extend_from_slice(&p.uv);
            out.extend_from_slice(&p.color);
        }
        out
    }
}

/// Every SOP's most recent output.
#[derive(Default)]
pub struct GeometryStore {
    data: SecondaryMap<NodeId, Geometry>,
}

impl GeometryStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get(&self, id: NodeId) -> Option<&Geometry> {
        self.data.get(id)
    }
    pub fn insert(&mut self, id: NodeId, geo: Geometry) {
        self.data.insert(id, geo);
    }
    pub fn remove(&mut self, id: NodeId) {
        self.data.remove(id);
    }
    pub fn clear(&mut self) {
        self.data.clear();
    }
}

#[derive(Default)]
pub struct SopEngine;

impl SopEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn cook_node(
        &mut self,
        graph: &Graph,
        id: NodeId,
        _ctx: &CookContext,
        eval: &EvalContext,
        store: &GeometryStore,
    ) -> Result<Geometry, CookError> {
        let node = graph.get(id).ok_or(CookError::NoSuchNode)?;
        let path = graph.path(id);

        if node.connector == otd_core::Connector::In {
            return Ok(graph
                .connector_source(id)
                .and_then(|src| store.get(src))
                .cloned()
                .unwrap_or_default());
        }

        let spec = ops::spec_for(&node.op_type)
            .ok_or_else(|| CookError::op(&path, format!("unknown SOP `{}`", node.op_type)))?;

        let inputs: Vec<Geometry> = node
            .inputs
            .iter()
            .map(|slot| {
                slot.and_then(|src| store.get(src))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();

        let eval = &EvalContext {
            path: Some(&path),
            ..*eval
        };
        let mut cx = ops::SopCtx { node, eval, inputs };
        Ok((spec.cook)(&mut cx))
    }
}

/// A SOP-only host, for tests.
#[derive(Default)]
pub struct SopHost {
    pub engine: SopEngine,
    pub store: GeometryStore,
}

impl SopHost {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn geometry(&self, id: NodeId) -> Option<&Geometry> {
        self.store.get(id)
    }
}

impl Cooker for SopHost {
    fn cook(&mut self, graph: &Graph, id: NodeId, ctx: &CookContext) -> Result<(), CookError> {
        if graph.get(id).map(|n| n.family) != Some(Family::Sop) {
            return Ok(());
        }
        let SopHost { engine, store } = self;
        let eval = ctx.eval_ctx();
        let geo = engine.cook_node(graph, id, ctx, &eval, store)?;
        store.insert(id, geo);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merging_makes_implicit_indices_explicit() {
        let a = Geometry {
            points: vec![Point::at(0.0, 0.0, 0.0); 3],
            ..Default::default()
        };
        let b = Geometry {
            points: vec![Point::at(1.0, 0.0, 0.0); 3],
            ..Default::default()
        };
        let m = a.merged(&b);
        assert_eq!(m.num_points(), 6);
        // Without explicit indices the two halves would be one run of six.
        assert_eq!(m.indices, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(m.num_primitives(), 2);
    }

    #[test]
    fn vertex_bytes_are_interleaved_in_the_renderers_layout() {
        let g = Geometry {
            points: vec![Point {
                position: [1.0, 2.0, 3.0],
                normal: [0.0, 1.0, 0.0],
                uv: [0.5, 0.25],
                color: [1.0, 0.0, 0.0, 1.0],
            }],
            ..Default::default()
        };
        let v = g.vertex_bytes();
        assert_eq!(v.len(), 12, "one point is twelve floats");
        assert_eq!(&v[0..3], &[1.0, 2.0, 3.0]);
        assert_eq!(&v[6..8], &[0.5, 0.25]);
        assert_eq!(&v[8..12], &[1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn bounds_of_nothing_are_not_infinite() {
        let g = Geometry::default();
        assert_eq!(g.bounds(), ([0.0; 3], [0.0; 3]));
    }
}
