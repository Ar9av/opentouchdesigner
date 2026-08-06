//! The cook engine: demand-driven, pull-based, lazily memoized.
//!
//! PLAN.md §2.2 / §4. The rules, in one place:
//!
//!  * Cooking is **pulled from roots**, never pushed from edits. A root is a
//!    visible viewer, an output window, or a node flagged `render`.
//!  * A node cooks iff (a) it changed since it last cooked, (b) one of its
//!    inputs produced a new output, or (c) it is **time dependent**.
//!  * Time dependence propagates downstream: a node fed by an animated node is
//!    itself animated. A branch with neither an animated parameter nor an
//!    animated input cooks once and then returns its cached output forever.
//!    This is the property that makes a big patch cheap.
//!  * Each node resolves at most once per frame, so diamonds cook their shared
//!    ancestor once.
//!
//! The engine owns *scheduling only*. Producing the actual output — a texture,
//! a channel buffer — is the [`Cooker`]'s job, which is how `otd-core` stays
//! free of any GPU dependency.

use slotmap::SecondaryMap;
use std::time::Instant;

use crate::expr::EvalContext;
use crate::graph::{Graph, NodeId};

/// Time state for one frame.
#[derive(Clone, Copy, Debug)]
pub struct CookContext {
    pub frame: i64,
    /// Component-local time, seconds.
    pub time: f64,
    /// Absolute time since the project started, seconds.
    pub abs_time: f64,
    pub fps: f64,
}

impl Default for CookContext {
    fn default() -> Self {
        CookContext {
            frame: 0,
            time: 0.0,
            abs_time: 0.0,
            fps: 60.0,
        }
    }
}

impl CookContext {
    pub fn eval_ctx(&self) -> EvalContext {
        EvalContext {
            frame: self.frame,
            time: self.time,
            abs_time: self.abs_time,
            fps: self.fps,
        }
    }

    /// Advance by one frame of wall time.
    pub fn advance(&mut self, dt: f64) {
        self.frame += 1;
        self.time += dt;
        self.abs_time += dt;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CookError {
    #[error("{path}: {message}")]
    Op { path: String, message: String },
    #[error("node vanished mid-cook")]
    NoSuchNode,
}

impl CookError {
    pub fn op(path: impl Into<String>, message: impl Into<String>) -> Self {
        CookError::Op {
            path: path.into(),
            message: message.into(),
        }
    }
}

/// Implemented by whatever actually produces operator output.
pub trait Cooker {
    /// Cook one node. Its inputs are guaranteed to have been cooked already
    /// this frame.
    fn cook(&mut self, graph: &Graph, id: NodeId, ctx: &CookContext) -> Result<(), CookError>;

    /// Called when a node is skipped because its cached output is still
    /// valid. Default: nothing.
    fn skipped(&mut self, _graph: &Graph, _id: NodeId) {}

    /// Dependencies a node has that are not wires.
    ///
    /// Some operators name their source by parameter rather than by input —
    /// a Select TOP pointing at `/blur1`, and later a parameter expression
    /// referencing another operator. Those still have to cook first and still
    /// have to propagate dirtiness and time dependence, so the backend
    /// declares them here and the engine treats them exactly like inputs.
    ///
    /// A Feedback TOP deliberately does *not* declare its target: reading
    /// last frame's result is the whole point, and declaring it would be a
    /// cycle.
    fn extra_inputs(&self, _graph: &Graph, _id: NodeId) -> Vec<NodeId> {
        Vec::new()
    }
}

#[derive(Clone, Debug, Default)]
struct NodeCookState {
    /// The graph revision this node was cooked at.
    revision: u64,
    /// Output versions of each input at the last cook.
    input_versions: Vec<u64>,
    /// Bumped every time the node actually cooks. Downstream nodes compare
    /// against it to decide whether their input changed.
    output_version: u64,
    /// Last frame this node was *resolved* (cooked or confirmed still valid).
    resolved_frame: i64,
    time_dependent: bool,
    cook_count: u64,
    last_cook_us: u64,
    has_cooked: bool,
    /// Set while this node is on the pull stack. Wired cycles are impossible
    /// (rejected at connect time) but a parameter-named reference can form
    /// one — two Select TOPs pointing at each other — and that must degrade
    /// to a stale read, not a stack overflow.
    visiting: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    /// Nodes that actually ran their operator this frame.
    pub cooked: u32,
    /// Nodes that were reached but served from cache.
    pub cached: u32,
    pub total_cook_us: u64,
}

#[derive(Default)]
pub struct CookEngine {
    state: SecondaryMap<NodeId, NodeCookState>,
    pub stats: FrameStats,
}

impl CookEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cook everything the given roots depend on, for one frame.
    pub fn cook_frame(
        &mut self,
        graph: &Graph,
        roots: &[NodeId],
        ctx: &CookContext,
        cooker: &mut dyn Cooker,
    ) -> Result<(), CookError> {
        self.stats = FrameStats::default();
        for root in roots {
            self.pull(graph, *root, ctx, cooker)?;
        }
        Ok(())
    }

    /// Resolve one node for this frame, cooking it and its dependencies only
    /// as far as necessary. Returns the node's output version.
    pub fn pull(
        &mut self,
        graph: &Graph,
        id: NodeId,
        ctx: &CookContext,
        cooker: &mut dyn Cooker,
    ) -> Result<u64, CookError> {
        let node = graph.get(id).ok_or(CookError::NoSuchNode)?;

        // Already settled this frame (diamond inputs, or a second root), or
        // already on the stack via a parameter-named reference cycle.
        if let Some(st) = self.state.get(id) {
            if st.visiting || (st.has_cooked && st.resolved_frame == ctx.frame) {
                return Ok(st.output_version);
            }
        }
        {
            let st = self.state.entry(id).unwrap().or_default();
            st.resolved_frame = ctx.frame;
            st.visiting = true;
        }

        // Pull inputs first, collecting their versions and time dependence.
        let mut input_versions = Vec::with_capacity(node.inputs.len());
        let mut inputs_time_dependent = false;
        for slot in &node.inputs {
            match slot {
                Some(src) => {
                    let v = self.pull(graph, *src, ctx, cooker)?;
                    inputs_time_dependent |= self
                        .state
                        .get(*src)
                        .map(|s| s.time_dependent)
                        .unwrap_or(false);
                    input_versions.push(v);
                }
                // An unconnected input is version 0 — stable, so it never
                // forces a re-cook on its own.
                None => input_versions.push(0),
            }
        }

        // Non-wire dependencies (a Select TOP's target, later a parameter
        // expression's `op()` reference) are pulled and versioned exactly like
        // wired inputs, so they dirty and animate this node the same way.
        for src in cooker.extra_inputs(graph, id) {
            if src == id || !graph.contains(src) {
                continue;
            }
            let v = self.pull(graph, src, ctx, cooker)?;
            inputs_time_dependent |= self
                .state
                .get(src)
                .map(|s| s.time_dependent)
                .unwrap_or(false);
            input_versions.push(v);
        }

        let self_time_dependent =
            node.intrinsically_time_dependent || node.has_time_dependent_param();
        let time_dependent = self_time_dependent || inputs_time_dependent;

        // Every recursive pull for this node is done; it is no longer on the
        // stack.
        let st = self.state.entry(id).unwrap().or_default();
        st.visiting = false;
        let inputs_changed = st.input_versions != input_versions;
        let edited = st.revision != node.revision();
        let needs_cook = !st.has_cooked || edited || inputs_changed || time_dependent;

        st.time_dependent = time_dependent;
        st.input_versions = input_versions;
        st.revision = node.revision();

        if !needs_cook {
            self.stats.cached += 1;
            cooker.skipped(graph, id);
            return Ok(self.state[id].output_version);
        }

        // Bypassed nodes pass input 0 straight through without running.
        if node.flags.bypass {
            let passthrough = node.inputs.first().and_then(|s| *s);
            let version = passthrough
                .and_then(|src| self.state.get(src).map(|s| s.output_version))
                .unwrap_or(0);
            let st = &mut self.state[id];
            st.output_version = version;
            st.has_cooked = true;
            self.stats.cached += 1;
            return Ok(version);
        }

        let started = Instant::now();
        cooker.cook(graph, id, ctx)?;
        let elapsed = started.elapsed().as_micros() as u64;

        let st = &mut self.state[id];
        st.output_version += 1;
        st.cook_count += 1;
        st.last_cook_us = elapsed;
        st.has_cooked = true;
        self.stats.cooked += 1;
        self.stats.total_cook_us += elapsed;
        Ok(st.output_version)
    }

    /// Forget all cached state. Call after loading a project.
    pub fn reset(&mut self) {
        self.state.clear();
        self.stats = FrameStats::default();
    }

    pub fn forget(&mut self, id: NodeId) {
        self.state.remove(id);
    }

    /// How many times this node has actually run its operator.
    pub fn cook_count(&self, id: NodeId) -> u64 {
        self.state.get(id).map(|s| s.cook_count).unwrap_or(0)
    }

    /// Microseconds spent in the most recent cook — the per-node number the
    /// performance monitor shows (PLAN.md §5, Phase 6).
    pub fn last_cook_us(&self, id: NodeId) -> u64 {
        self.state.get(id).map(|s| s.last_cook_us).unwrap_or(0)
    }

    pub fn is_time_dependent(&self, id: NodeId) -> bool {
        self.state
            .get(id)
            .map(|s| s.time_dependent)
            .unwrap_or(false)
    }

    pub fn output_version(&self, id: NodeId) -> u64 {
        self.state.get(id).map(|s| s.output_version).unwrap_or(0)
    }

    pub fn has_cooked(&self, id: NodeId) -> bool {
        self.state.get(id).map(|s| s.has_cooked).unwrap_or(false)
    }
}

/// Follow bypass flags to the node that actually produced the output that
/// `id` presents. Returns `None` if a bypassed node has nothing on input 0.
pub fn resolve_bypass(graph: &Graph, id: NodeId) -> Option<NodeId> {
    let mut cur = id;
    for _ in 0..64 {
        let node = graph.get(cur)?;
        if !node.flags.bypass {
            return Some(cur);
        }
        cur = (*node.inputs.first()?)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;
    use crate::test_ops::{CountingCooker, TEST_PASS, registry};
    use crate::value::Value;

    /// noise -> level -> null
    fn chain() -> (Graph, Vec<NodeId>) {
        let mut g = Graph::new();
        let reg = registry();
        let root = g.root();
        let a = g.create(root, reg.get("pass").unwrap(), Some("a")).unwrap();
        let b = g.create(root, reg.get("pass").unwrap(), Some("b")).unwrap();
        let c = g.create(root, reg.get("pass").unwrap(), Some("c")).unwrap();
        g.connect(a, b, 0).unwrap();
        g.connect(b, c, 0).unwrap();
        (g, vec![a, b, c])
    }

    #[test]
    fn a_static_graph_cooks_once_and_then_caches() {
        let (g, n) = chain();
        let mut e = CookEngine::new();
        let mut cooker = CountingCooker::default();
        let mut ctx = CookContext::default();

        for _ in 0..10 {
            e.cook_frame(&g, &[n[2]], &ctx, &mut cooker).unwrap();
            ctx.advance(1.0 / 60.0);
        }
        assert_eq!(e.cook_count(n[0]), 1);
        assert_eq!(e.cook_count(n[1]), 1);
        assert_eq!(e.cook_count(n[2]), 1);
    }

    #[test]
    fn editing_a_node_recooks_it_and_everything_downstream_only() {
        let (mut g, n) = chain();
        let mut e = CookEngine::new();
        let mut cooker = CountingCooker::default();
        let mut ctx = CookContext::default();
        e.cook_frame(&g, &[n[2]], &ctx, &mut cooker).unwrap();

        ctx.advance(1.0 / 60.0);
        g.set_param(n[1], TEST_PASS, Value::Float(0.5)).unwrap();
        e.cook_frame(&g, &[n[2]], &ctx, &mut cooker).unwrap();

        assert_eq!(e.cook_count(n[0]), 1, "upstream must not re-cook");
        assert_eq!(e.cook_count(n[1]), 2);
        assert_eq!(e.cook_count(n[2]), 2, "downstream must re-cook");
    }

    #[test]
    fn time_dependence_propagates_downstream_but_not_up() {
        let (mut g, n) = chain();
        // Animate the middle node's parameter.
        g.set_expression(n[1], TEST_PASS, "sin(absTime)").unwrap();

        let mut e = CookEngine::new();
        let mut cooker = CountingCooker::default();
        let mut ctx = CookContext::default();
        for _ in 0..5 {
            e.cook_frame(&g, &[n[2]], &ctx, &mut cooker).unwrap();
            ctx.advance(1.0 / 60.0);
        }
        assert_eq!(e.cook_count(n[0]), 1, "static upstream stays cached");
        assert_eq!(e.cook_count(n[1]), 5);
        assert_eq!(e.cook_count(n[2]), 5);
        assert!(!e.is_time_dependent(n[0]));
        assert!(e.is_time_dependent(n[1]));
        assert!(e.is_time_dependent(n[2]));
    }

    #[test]
    fn a_shared_ancestor_cooks_once_per_frame() {
        let mut g = Graph::new();
        let reg = registry();
        let root = g.root();
        let src = g.create(root, reg.get("pass").unwrap(), None).unwrap();
        let l = g.create(root, reg.get("pass").unwrap(), None).unwrap();
        let r = g.create(root, reg.get("pass").unwrap(), None).unwrap();
        let join = g.create(root, reg.get("comp2").unwrap(), None).unwrap();
        g.connect(src, l, 0).unwrap();
        g.connect(src, r, 0).unwrap();
        g.connect(l, join, 0).unwrap();
        g.connect(r, join, 1).unwrap();
        // Force every frame to re-cook so the per-frame guard is what's tested.
        g.set_expression(src, TEST_PASS, "absTime").unwrap();

        let mut e = CookEngine::new();
        let mut cooker = CountingCooker::default();
        let mut ctx = CookContext::default();
        for _ in 0..4 {
            e.cook_frame(&g, &[join], &ctx, &mut cooker).unwrap();
            ctx.advance(1.0 / 60.0);
        }
        assert_eq!(e.cook_count(src), 4, "once per frame, not twice");
        assert_eq!(e.cook_count(join), 4);
    }

    #[test]
    fn unreached_branches_never_cook() {
        let (mut g, n) = chain();
        let reg = registry();
        let root = g.root();
        let orphan = g.create(root, reg.get("pass").unwrap(), None).unwrap();
        g.connect(n[0], orphan, 0).unwrap();

        let mut e = CookEngine::new();
        let mut cooker = CountingCooker::default();
        e.cook_frame(&g, &[n[2]], &CookContext::default(), &mut cooker)
            .unwrap();
        assert_eq!(e.cook_count(orphan), 0);
    }

    #[test]
    fn bypass_passes_through_without_cooking() {
        let (mut g, n) = chain();
        g.node_mut(n[1]).flags.bypass = true;
        let mut e = CookEngine::new();
        let mut cooker = CountingCooker::default();
        e.cook_frame(&g, &[n[2]], &CookContext::default(), &mut cooker)
            .unwrap();
        assert_eq!(e.cook_count(n[1]), 0);
        assert_eq!(e.cook_count(n[0]), 1);
        assert_eq!(resolve_bypass(&g, n[1]), Some(n[0]));
    }

    #[test]
    fn a_parameter_named_reference_cooks_and_animates_like_a_wire() {
        let (mut g, n) = chain();
        let reg = registry();
        let root = g.root();
        // `selector` names n[1] by parameter rather than wiring to it.
        let selector = g
            .create(root, reg.get("pass").unwrap(), Some("sel"))
            .unwrap();
        let mut cooker = CountingCooker {
            references: vec![(selector, n[1])],
            ..Default::default()
        };
        g.set_expression(n[0], TEST_PASS, "absTime").unwrap();

        let mut e = CookEngine::new();
        let mut ctx = CookContext::default();
        for _ in 0..3 {
            e.cook_frame(&g, &[selector], &ctx, &mut cooker).unwrap();
            ctx.advance(1.0 / 60.0);
        }
        // Pulling only `selector` must have dragged the referenced branch in.
        assert_eq!(e.cook_count(n[0]), 3);
        assert_eq!(e.cook_count(n[1]), 3);
        assert_eq!(e.cook_count(selector), 3);
        assert!(e.is_time_dependent(selector), "animation must propagate");
    }

    #[test]
    fn a_reference_cycle_degrades_to_a_stale_read_not_a_stack_overflow() {
        let mut g = Graph::new();
        let reg = registry();
        let root = g.root();
        let a = g.create(root, reg.get("pass").unwrap(), Some("a")).unwrap();
        let b = g.create(root, reg.get("pass").unwrap(), Some("b")).unwrap();
        let mut cooker = CountingCooker {
            references: vec![(a, b), (b, a)],
            ..Default::default()
        };
        let mut e = CookEngine::new();
        e.cook_frame(&g, &[a], &CookContext::default(), &mut cooker)
            .unwrap();
        assert_eq!(e.cook_count(a), 1);
        assert_eq!(e.cook_count(b), 1);
    }

    #[test]
    fn an_intrinsically_animated_source_drives_the_chain() {
        let mut g = Graph::new();
        let reg = registry();
        let root = g.root();
        let mov = g.create(root, reg.get("movie").unwrap(), None).unwrap();
        let out = g.create(root, reg.get("pass").unwrap(), None).unwrap();
        g.connect(mov, out, 0).unwrap();

        let mut e = CookEngine::new();
        let mut cooker = CountingCooker::default();
        let mut ctx = CookContext::default();
        for _ in 0..3 {
            e.cook_frame(&g, &[out], &ctx, &mut cooker).unwrap();
            ctx.advance(1.0 / 60.0);
        }
        assert_eq!(e.cook_count(mov), 3);
        assert_eq!(e.cook_count(out), 3);
    }
}
