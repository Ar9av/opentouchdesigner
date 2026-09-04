//! The CHOP cook backend.
//!
//! The cooked channels live in a [`ChannelStore`] that is deliberately kept
//! *outside* the engine. Parameters in Export mode read from it while the
//! engine is mutating operator state, and splitting the two apart is what
//! lets both happen in the same frame without fighting the borrow checker —
//! or worse, without a lock in the cook path.

use otd_core::{
    ChannelSource, CookContext, CookError, Cooker, EvalContext, Family, Graph, NodeId, Value,
};
use slotmap::SecondaryMap;

use crate::data::ChopData;
use crate::io::Io;
use crate::ops::{self, ChopCtx, OpState};

/// Every CHOP's most recent output.
#[derive(Default)]
pub struct ChannelStore {
    data: SecondaryMap<NodeId, ChopData>,
}

impl ChannelStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get(&self, id: NodeId) -> Option<&ChopData> {
        self.data.get(id)
    }
    pub fn insert(&mut self, id: NodeId, data: ChopData) {
        self.data.insert(id, data);
    }
    pub fn remove(&mut self, id: NodeId) {
        self.data.remove(id);
    }
    pub fn clear(&mut self) {
        self.data.clear();
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// A read-only view of the network for parameters in Export or Bind mode.
pub struct Network<'a> {
    pub graph: &'a Graph,
    pub chops: &'a ChannelStore,
}

impl ChannelSource for Network<'_> {
    fn channel(&self, op_path: &str, channel: &str) -> Option<f32> {
        let id = self.graph.find(op_path)?;
        let data = self.chops.get(id)?;
        // By name first, then by index, so `chan1`/`0` both work and an
        // export survives a channel being renamed upstream if it keeps
        // its position.
        data.value(channel).or_else(|| {
            channel
                .parse::<usize>()
                .ok()
                .and_then(|i| data.nth(i))
                .map(|c| c.last())
        })
    }

    fn param_value(&self, op_path: &str, param: &str) -> Option<Value> {
        let id = self.graph.find(op_path)?;
        let p = self.graph.get(id)?.param(param)?;
        // Evaluated without a network of its own: a Bind reads a value, it
        // does not chain into another Bind. That keeps the resolution
        // finite and the failure mode obvious.
        Some(p.eval(&EvalContext::default()))
    }
}

#[derive(Default)]
pub struct ChopEngine {
    state: SecondaryMap<NodeId, OpState>,
    pub io: Io,
}

impl ChopEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cook one CHOP and return its channels. The caller stores them — see
    /// the module docs for why the engine does not.
    pub fn cook_node(
        &mut self,
        graph: &Graph,
        id: NodeId,
        ctx: &CookContext,
        eval: &EvalContext,
        chops: &ChannelStore,
    ) -> Result<ChopData, CookError> {
        let node = graph.get(id).ok_or(CookError::NoSuchNode)?;
        let path = graph.path(id);
        let spec = ops::spec_for(&node.op_type)
            .ok_or_else(|| CookError::op(&path, format!("unknown CHOP `{}`", node.op_type)))?;

        // Inputs are copied rather than borrowed: at control rate that is a
        // handful of floats, and at audio rate it is one frame of mono.
        let inputs: Vec<ChopData> = node
            .inputs
            .iter()
            .map(|slot| {
                slot.and_then(|src| chops.get(src))
                    .cloned()
                    .unwrap_or_else(ChopData::empty)
            })
            .collect();

        let mut cctx = ChopCtx {
            node,
            eval,
            time: ctx,
            inputs,
            state: self.state.entry(id).unwrap().or_default(),
            io: &mut self.io,
            path: &path,
        };
        Ok((spec.cook)(&mut cctx))
    }

    /// A device or network message for this node, shown on the node body.
    pub fn status(&self, path: &str) -> Option<&str> {
        self.io.status_of(path)
    }

    pub fn forget(&mut self, id: NodeId) {
        self.state.remove(id);
    }

    pub fn reset(&mut self) {
        self.state.clear();
        self.io.reset();
    }
}

/// A CHOP-only host: the engine plus its store, implementing [`Cooker`].
///
/// The editor uses the cross-family router in `otd-engine` instead; this
/// exists for tests and for a headless control-only runtime.
#[derive(Default)]
pub struct ChopHost {
    pub engine: ChopEngine,
    pub channels: ChannelStore,
}

impl ChopHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn data(&self, id: NodeId) -> Option<&ChopData> {
        self.channels.get(id)
    }

    /// The current value of a channel, by node and channel name.
    pub fn value(&self, graph: &Graph, path: &str, channel: &str) -> Option<f32> {
        Network {
            graph,
            chops: &self.channels,
        }
        .channel(path, channel)
    }
}

impl Cooker for ChopHost {
    fn cook(&mut self, graph: &Graph, id: NodeId, ctx: &CookContext) -> Result<(), CookError> {
        if graph.get(id).map(|n| n.family) != Some(Family::Chop) {
            return Ok(());
        }
        let ChopHost { engine, channels } = self;
        let data = {
            let net = Network {
                graph,
                chops: channels,
            };
            let eval = ctx.eval_ctx_with(&net);
            engine.cook_node(graph, id, ctx, &eval, channels)?
        };
        channels.insert(id, data);
        Ok(())
    }
}
