//! Undo and redo.
//!
//! PLAN.md Phase 6 asks for "undo/redo everywhere", and *everywhere* is the
//! hard part. The usual approach — one invertible command per operation — has
//! to be got right for creating, deleting, wiring, unwiring, every parameter
//! mode, custom parameters, component attachment and clone syncing, and every
//! one of those is a separate chance to write an inverse that is subtly wrong.
//! An undo that *mostly* works is worse than none, because you only find out
//! after you have lost the work.
//!
//! So this snapshots the graph instead. A graph is plain data — nodes, params,
//! wires — and cloning it costs on the order of the project size, paid only
//! when something is edited rather than per frame. The snapshot preserves node
//! ids, which is what makes it usable: the selection, the viewer and the cook
//! engine's caches all still refer to the same nodes after an undo.
//!
//! The one thing snapshots need on top is **coalescing**. Dragging a slider
//! emits an edit every frame, and sixty undo entries for one gesture is not
//! undo. Each checkpoint carries a tag naming what is being edited; while the
//! tag stays the same, the gesture is one entry.

use crate::graph::Graph;

/// How many steps back you can go. Fifty is enough to cover a session's worth
/// of "what did I just break", and the memory is a graph clone apiece.
pub const DEFAULT_LIMIT: usize = 50;

pub struct History {
    past: Vec<Graph>,
    future: Vec<Graph>,
    limit: usize,
    /// What the last checkpoint was editing, for coalescing.
    last_tag: Option<String>,
}

impl Default for History {
    fn default() -> Self {
        History::new(DEFAULT_LIMIT)
    }
}

impl History {
    pub fn new(limit: usize) -> History {
        History {
            past: Vec::new(),
            future: Vec::new(),
            limit: limit.max(1),
            last_tag: None,
        }
    }

    /// Record the state *before* a change.
    ///
    /// `tag` names what is about to be edited — `"param:/level1/brightness"`,
    /// `"move:/noise1"`. Consecutive checkpoints with the same tag are one
    /// entry, so a drag undoes as the gesture it was rather than as the sixty
    /// frames it took.
    pub fn checkpoint(&mut self, graph: &Graph, tag: &str) {
        if self.last_tag.as_deref() == Some(tag) && !self.past.is_empty() {
            // Mid-gesture: the entry already holds the state before it began,
            // which is exactly where undo should land.
            self.future.clear();
            return;
        }
        self.last_tag = Some(tag.to_string());
        self.past.push(graph.clone());
        if self.past.len() > self.limit {
            self.past.remove(0);
        }
        // A new edit is a new branch: whatever was undone is no longer ahead.
        self.future.clear();
    }

    /// End the current gesture, so the next edit starts a new undo entry even
    /// if it carries the same tag. Called on mouse-up and on focus loss.
    pub fn end_gesture(&mut self) {
        self.last_tag = None;
    }

    /// Step back. `current` is the state being left, which becomes the redo.
    pub fn undo(&mut self, current: &Graph) -> Option<Graph> {
        let previous = self.past.pop()?;
        self.future.push(current.clone());
        self.last_tag = None;
        Some(previous)
    }

    pub fn redo(&mut self, current: &Graph) -> Option<Graph> {
        let next = self.future.pop()?;
        self.past.push(current.clone());
        self.last_tag = None;
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Forget everything — opening a project, or starting a new one.
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
        self.last_tag = None;
    }

    pub fn depth(&self) -> (usize, usize) {
        (self.past.len(), self.future.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Connector, Family, NodeId, OpDef, OpRegistry};
    use crate::param::Param;
    use crate::value::Value;
    use indexmap::IndexMap;

    fn params() -> IndexMap<String, Param> {
        let mut m = IndexMap::new();
        m.insert("gain".into(), Param::float(1.0));
        m
    }

    fn registry() -> OpRegistry {
        let mut r = OpRegistry::new();
        r.register(OpDef {
            type_name: "pass",
            input_families: &[],
            label: "Pass",
            family: Family::Top,
            inputs: &["in"],
            summary: "",
            time_dependent: false,
            params,
            connector: Connector::None,
        });
        r
    }

    fn patch() -> (Graph, OpRegistry, NodeId, NodeId) {
        let reg = registry();
        let mut g = Graph::new();
        let root = g.root();
        let a = g.create(root, reg.get("pass").unwrap(), Some("a")).unwrap();
        let b = g.create(root, reg.get("pass").unwrap(), Some("b")).unwrap();
        (g, reg, a, b)
    }

    #[test]
    fn undo_restores_a_parameter_and_redo_puts_it_back() {
        let (mut g, _reg, a, _) = patch();
        let mut h = History::default();

        h.checkpoint(&g, "param:/a/gain");
        g.set_param(a, "gain", Value::Float(2.0)).unwrap();
        h.end_gesture();

        g = h.undo(&g).unwrap();
        assert_eq!(g.node(a).param("gain").unwrap().value, Value::Float(1.0));

        g = h.redo(&g).unwrap();
        assert_eq!(g.node(a).param("gain").unwrap().value, Value::Float(2.0));
    }

    #[test]
    fn node_ids_survive_an_undo() {
        let (mut g, _reg, a, _) = patch();
        let mut h = History::default();
        h.checkpoint(&g, "param:/a/gain");
        g.set_param(a, "gain", Value::Float(2.0)).unwrap();

        // The selection, the viewer and the cook engine's caches are all keyed
        // by NodeId. If undo minted new ids, every one of them would be
        // pointing at nothing.
        let g = h.undo(&g).unwrap();
        assert!(g.contains(a), "the same node is still there");
        assert_eq!(g.path(a), "/a");
    }

    #[test]
    fn a_drag_is_one_undo_entry_not_sixty() {
        let (mut g, _reg, a, _) = patch();
        let mut h = History::default();

        // A slider being dragged: one edit per frame, same tag throughout.
        for i in 1..=60 {
            h.checkpoint(&g, "param:/a/gain");
            g.set_param(a, "gain", Value::Float(i as f64)).unwrap();
        }
        h.end_gesture();
        assert_eq!(h.depth().0, 1, "the whole gesture is one entry");

        let g = h.undo(&g).unwrap();
        assert_eq!(
            g.node(a).param("gain").unwrap().value,
            Value::Float(1.0),
            "undo lands where the drag started, not one frame back"
        );
    }

    #[test]
    fn editing_a_different_thing_starts_a_new_entry() {
        let (mut g, _reg, a, b) = patch();
        let mut h = History::default();

        h.checkpoint(&g, "param:/a/gain");
        g.set_param(a, "gain", Value::Float(5.0)).unwrap();
        h.checkpoint(&g, "param:/b/gain");
        g.set_param(b, "gain", Value::Float(7.0)).unwrap();

        assert_eq!(h.depth().0, 2);
        let g = h.undo(&g).unwrap();
        assert_eq!(g.node(b).param("gain").unwrap().value, Value::Float(1.0));
        assert_eq!(
            g.node(a).param("gain").unwrap().value,
            Value::Float(5.0),
            "only the second edit was undone"
        );
    }

    #[test]
    fn structure_undoes_too_not_just_parameters() {
        let (mut g, reg, a, b) = patch();
        let mut h = History::default();

        h.checkpoint(&g, "connect");
        g.connect(a, b, 0).unwrap();
        h.end_gesture();

        h.checkpoint(&g, "delete");
        g.remove(a).unwrap();
        h.end_gesture();

        // Deleting a node takes its wires with it; undo has to bring both back.
        let mut g = h.undo(&g).unwrap();
        assert!(g.contains(a));
        assert_eq!(g.node(b).inputs[0], Some(a), "the wire came back too");

        // And a create.
        h.checkpoint(&g, "create");
        let c = g
            .create(g.root(), reg.get("pass").unwrap(), Some("c"))
            .unwrap();
        let g = h.undo(&g).unwrap();
        assert!(!g.contains(c));
    }

    #[test]
    fn a_new_edit_after_an_undo_discards_the_redo() {
        let (mut g, _reg, a, _) = patch();
        let mut h = History::default();

        h.checkpoint(&g, "one");
        g.set_param(a, "gain", Value::Float(2.0)).unwrap();
        g = h.undo(&g).unwrap();
        assert!(h.can_redo());

        // History is a tree collapsed to a line: editing from here abandons
        // the branch that was ahead, rather than leaving a redo that would
        // jump to a state this one never came from.
        h.checkpoint(&g, "two");
        g.set_param(a, "gain", Value::Float(3.0)).unwrap();
        assert!(!h.can_redo());
    }

    #[test]
    fn undoing_at_the_beginning_does_nothing_rather_than_panicking() {
        let (g, _reg, _, _) = patch();
        let mut h = History::default();
        assert!(!h.can_undo());
        assert!(h.undo(&g).is_none());
        assert!(h.redo(&g).is_none());
    }

    #[test]
    fn the_stack_is_bounded() {
        let (mut g, _reg, a, _) = patch();
        let mut h = History::new(3);
        for i in 0..10 {
            h.checkpoint(&g, &format!("edit{i}"));
            g.set_param(a, "gain", Value::Float(i as f64)).unwrap();
        }
        assert_eq!(h.depth().0, 3, "old entries are dropped, not accumulated");

        // And the ones kept are the most recent, so undo still goes backwards
        // one step at a time from here.
        let g = h.undo(&g).unwrap();
        assert_eq!(g.node(a).param("gain").unwrap().value, Value::Float(8.0));
    }
}
